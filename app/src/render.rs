//! GPU rendering: shared DX12 device, image textures with linear-light mipmaps,
//! tiling above the texture limit, per-window uniforms. egui is drawn on top by the window.

use crate::viewer::Affine;
use wgpu::util::DeviceExt;

pub const IMAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub struct Gpu {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub max_tex: u32,
    globals_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    pipelines: std::sync::Mutex<Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>>,
    mip_pipeline: wgpu::RenderPipeline,
    mip_layout: wgpu::BindGroupLayout,
    pub samplers: Samplers,
    /// Markup preview overlay objects, created at the first use.
    pub overlay: std::sync::OnceLock<crate::markup::overlay::Shared>,
}

/// Size of the D3D12 shader-visible descriptor heap (see `Gpu::new`).
const MAX_LIVE_DESCRIPTORS: u32 = 100_000;

/// Surface format used by every window when available (non-sRGB).
pub const SURFACE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

pub struct Samplers {
    pub nearest: wgpu::Sampler,
    pub linear: wgpu::Sampler,
    pub linear_mip: wgpu::Sampler,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Filter {
    Nearest,
    Linear,
    LinearMip,
}

impl Gpu {
    /// Runs on a background thread. On DX12 every adapter can present: no surface needed.
    pub fn new(software: bool) -> anyhow::Result<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            // Indirect-call validation (on by default, also in release builds) compiles compute
            // pipelines when the device is created (34 → 17 ms here). rView makes no indirect
            // draw/dispatch call, so there is nothing to validate.
            flags: wgpu::InstanceFlags::from_build_config().difference(wgpu::InstanceFlags::VALIDATION_INDIRECT_CALL),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface: Option<&wgpu::Surface<'_>> = None;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            force_fallback_adapter: software,
            compatible_surface: surface,
            apply_limit_buckets: false,
        }))
        .or_else(|_| {
            // No usable hardware adapter: WARP.
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::None,
                force_fallback_adapter: true,
                compatible_surface: surface,
                apply_limit_buckets: false,
            }))
        })?;
        log::info!("adapter: {:?}", adapter.get_info());
        // D3D12 allocates its shader-visible descriptor heap up front with `max_non_sampler_bindings`
        // entries (wgpu warns about it). The adapter's 1 000 000 commit ~30 MB, resident on
        // integrated GPUs (measured: +30 MB, vs +3 MB for 100 000). rView keeps tens of live
        // descriptors per window (globals, one per image tile, mip passes, overlay, egui textures).
        let limits = adapter.limits();
        let required_limits = wgpu::Limits {
            max_non_sampler_bindings: limits.max_non_sampler_bindings.min(MAX_LIVE_DESCRIPTORS),
            ..limits
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("rview"),
            required_limits,
            ..Default::default()
        }))?;
        let max_tex = device.limits().max_texture_dimension_2d;
        let shader = device.create_shader_module(wgpu::include_wgsl!("image.wgsl"));

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                sampler_entry(1),
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[texture_entry(0)],
        });
        let mip_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("mip"),
            entries: &[texture_entry(0), sampler_entry(1)],
        });
        let mip_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mip"),
            bind_group_layouts: &[Some(&mip_layout)],
            immediate_size: 0,
        });
        let mip_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("mip"),
            layout: Some(&mip_pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("mip_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("mip_fs"),
                compilation_options: Default::default(),
                targets: &[Some(IMAGE_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let sampler = |filter, mip, lod_max| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: mip,
                lod_max_clamp: lod_max,
                ..Default::default()
            })
        };
        let samplers = Samplers {
            nearest: sampler(wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest, 0.0),
            linear: sampler(wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Nearest, 0.0),
            linear_mip: sampler(wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear, 32.0),
        };

        Ok(Gpu {
            instance,
            adapter,
            device,
            queue,
            max_tex,
            globals_layout,
            texture_layout,
            shader,
            pipelines: Default::default(),
            mip_pipeline,
            mip_layout,
            samplers,
            overlay: std::sync::OnceLock::new(),
        })
    }

    /// Image pipeline for a target format (one per surface format, created on demand).
    pub fn image_pipeline(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        if let Some((_, p)) = self.pipelines.lock().unwrap().iter().find(|(f, _)| *f == format) {
            return p.clone();
        }
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image"),
            bind_group_layouts: &[Some(&self.globals_layout), Some(&self.texture_layout)],
            immediate_size: 0,
        });
        let p = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                })],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(format.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.lock().unwrap().push((format, p.clone()));
        p
    }

    pub fn sampler(&self, f: Filter) -> &wgpu::Sampler {
        match f {
            Filter::Nearest => &self.samplers.nearest,
            Filter::Linear => &self.samplers.linear,
            Filter::LinearMip => &self.samplers.linear_mip,
        }
    }

    /// Per-window uniforms + sampler bind group.
    pub fn globals(&self) -> Globals {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let groups = [Filter::Nearest, Filter::Linear, Filter::LinearMip].map(|f| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("globals"),
                layout: &self.globals_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(self.sampler(f)),
                    },
                ],
            })
        });
        Globals { buffer, groups }
    }

    /// Upload a view buffer (RGBA8 sRGB, premultiplied in linear light). Tiles only above the
    /// texture limit.
    /// `logical` is the size the image is drawn at in image coordinates: the full image's size for
    /// a reduced preview, (w, h) otherwise.
    pub fn upload(&self, w: u32, h: u32, rgba: &[u8], mips: bool, logical: (u32, u32)) -> GpuImage {
        let scale = (logical.0 as f32 / w as f32, logical.1 as f32 / h as f32);
        let t = self.max_tex;
        let mut tiles = Vec::new();
        for ty in (0..h).step_by(t as usize) {
            for tx in (0..w).step_by(t as usize) {
                let (tw, th) = ((w - tx).min(t), (h - ty).min(t));
                let texture = self.create_texture(tw, th, mips);
                self.write_tile(&texture, tx, ty, tw, th, w, rgba);
                tiles.push(self.make_tile(texture, tx, ty, tw, th, scale));
            }
        }
        let img = GpuImage {
            w,
            h,
            tiles,
            mips,
            scale,
        };
        if mips {
            self.generate_mips(&img);
        }
        img
    }

    #[allow(clippy::too_many_arguments)]
    fn write_tile(&self, texture: &wgpu::Texture, x: u32, y: u32, w: u32, h: u32, row_px: u32, rgba: &[u8]) {
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: (y as u64 * row_px as u64 + x as u64) * 4,
                bytes_per_row: Some(row_px * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// New content for an existing image of the same size (animation frame): same textures,
    /// mips regenerated if present.
    pub fn rewrite(&self, img: &GpuImage, rgba: &[u8]) {
        for t in &img.tiles {
            self.write_tile(&t.texture, t.x, t.y, t.w, t.h, img.w, rgba);
        }
        if img.mips {
            self.generate_mips(img);
        }
    }

    /// Overwrite the image's pixels in (x, y, w, h) (image texels) with `rgba` (a w×h view buffer):
    /// the Markup preview of redactions. Mips regenerated if present.
    pub fn write_region(&self, img: &GpuImage, x: u32, y: u32, w: u32, h: u32, rgba: &[u8]) {
        for t in &img.tiles {
            let (x0, y0) = (x.max(t.x), y.max(t.y));
            let (x1, y1) = ((x + w).min(t.x + t.w), (y + h).min(t.y + t.h));
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &t.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: x0 - t.x,
                        y: y0 - t.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: ((y0 - y) as u64 * w as u64 + (x0 - x) as u64) * 4,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: None,
                },
                wgpu::Extent3d {
                    width: x1 - x0,
                    height: y1 - y0,
                    depth_or_array_layers: 1,
                },
            );
        }
        if img.mips {
            self.generate_mips(img);
        }
    }

    /// Recreate the textures with a mip chain (first zoom below 100 % of an image that fit).
    pub fn add_mips(&self, img: &GpuImage) -> GpuImage {
        let mut enc = self.device.create_command_encoder(&Default::default());
        let tiles = img
            .tiles
            .iter()
            .map(|t| {
                let texture = self.create_texture(t.w, t.h, true);
                enc.copy_texture_to_texture(
                    t.texture.as_image_copy(),
                    texture.as_image_copy(),
                    wgpu::Extent3d {
                        width: t.w,
                        height: t.h,
                        depth_or_array_layers: 1,
                    },
                );
                self.make_tile(texture, t.x, t.y, t.w, t.h, img.scale)
            })
            .collect();
        self.queue.submit([enc.finish()]);
        let out = GpuImage {
            w: img.w,
            h: img.h,
            tiles,
            mips: true,
            scale: img.scale,
        };
        self.generate_mips(&out);
        out
    }

    fn create_texture(&self, w: u32, h: u32, mips: bool) -> wgpu::Texture {
        let levels = if mips { 32 - w.max(h).leading_zeros() } else { 1 };
        self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: IMAGE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    }

    fn make_tile(&self, texture: wgpu::Texture, x: u32, y: u32, w: u32, h: u32, scale: (f32, f32)) -> Tile {
        let view = texture.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tile"),
            layout: &self.texture_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let (sx, sy) = scale;
        let (x0, y0, x1, y1) = (x as f32 * sx, y as f32 * sy, (x + w) as f32 * sx, (y + h) as f32 * sy);
        let verts: [f32; 16] = [x0, y0, 0.0, 0.0, x1, y0, 1.0, 0.0, x0, y1, 0.0, 1.0, x1, y1, 1.0, 1.0];
        let vertices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tile"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Tile {
            texture,
            bind,
            vertices,
            x,
            y,
            w,
            h,
        }
    }

    fn generate_mips(&self, img: &GpuImage) {
        let mut enc = self.device.create_command_encoder(&Default::default());
        for t in &img.tiles {
            for level in 1..t.texture.mip_level_count() {
                let view = |l| {
                    t.texture.create_view(&wgpu::TextureViewDescriptor {
                        base_mip_level: l,
                        mip_level_count: Some(1),
                        ..Default::default()
                    })
                };
                let (src, dst) = (view(level - 1), view(level));
                let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("mip"),
                    layout: &self.mip_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&src),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&self.samplers.linear),
                        },
                    ],
                });
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("mip"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &dst,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                pass.set_pipeline(&self.mip_pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit([enc.finish()]);
    }
}

fn sampler_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

fn texture_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub struct Tile {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    vertices: wgpu::Buffer,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

pub struct GpuImage {
    pub w: u32,
    pub h: u32,
    tiles: Vec<Tile>,
    pub mips: bool,
    /// Logical (image-coordinate) size / texture size; 1 except for previews.
    scale: (f32, f32),
}

impl GpuImage {
    /// Same textures could hold an image of this shape (reuse instead of a new allocation).
    pub fn fits(&self, w: u32, h: u32, mips: bool, logical: (u32, u32)) -> bool {
        self.w == w
            && self.h == h
            && self.mips == mips
            && self.scale == (logical.0 as f32 / w as f32, logical.1 as f32 / h as f32)
    }
}

pub struct Globals {
    buffer: wgpu::Buffer,
    groups: [wgpu::BindGroup; 3],
}

impl Globals {
    pub fn write(&self, queue: &wgpu::Queue, ndc: &Affine, bg_linear: [f32; 3]) {
        let m = ndc.m;
        let data: [f32; 12] = [
            m[0],
            m[1],
            m[2],
            0.0,
            m[3],
            m[4],
            m[5],
            0.0,
            bg_linear[0],
            bg_linear[1],
            bg_linear[2],
            1.0,
        ];
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&data));
    }
}

/// Draw the image into an already-begun pass.
pub fn draw_image(
    pass: &mut wgpu::RenderPass<'_>,
    pipeline: &wgpu::RenderPipeline,
    g: &Globals,
    filter: Filter,
    img: &GpuImage,
) {
    pass.set_pipeline(pipeline);
    let i = match filter {
        Filter::Nearest => 0,
        Filter::Linear => 1,
        Filter::LinearMip => 2,
    };
    pass.set_bind_group(0, &g.groups[i], &[]);
    for t in &img.tiles {
        pass.set_bind_group(1, &t.bind, &[]);
        pass.set_vertex_buffer(0, t.vertices.slice(..));
        pass.draw(0..4, 0..1);
    }
}

/// Sampler choice from settings and current scale.
pub fn choose_filter(s: &rv_core::settings::Settings, scale: f32, has_mips: bool) -> Filter {
    if !s.filtering_enabled || (scale > 1.0 && !s.scaling_two_enabled) {
        Filter::Nearest
    } else if scale < 1.0 && s.scaling_enabled && has_mips {
        Filter::LinearMip
    } else {
        Filter::Linear
    }
}
