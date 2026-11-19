//! Preview of the shapes: tiny-skia rasterises them — with the same paths as the
//! export (`core::edit::render::draw_shapes`) — into viewport-sized premultiplied layers, drawn
//! over the image with source-over on encoded values. Two layers: "static" (every
//! shape not being changed) and "active" (the shapes of the running gesture), so a drag only
//! re-rasterises what moves. While the view pans or zooms the layers already rasterised are only
//! transformed by the shader; they are rasterised again once the view has been still for 100 ms.

use crate::render::Gpu;
use crate::viewer::Affine;
use rv_core::edit::render::{Base, ColorMap, draw_shapes_banded};
use rv_core::edit::tiny_skia::{Pixmap, Transform};
use rv_core::edit::{Shape, ShapeKind};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The view must be still this long before the layers are rasterised again at the new zoom.
const SETTLE: Duration = Duration::from_millis(100);

const SHADER: &str = r#"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
// texel of the layer = m * (screen pixel, 1); size = layer size in texels
struct U { m0: vec4<f32>, m1: vec4<f32>, size: vec4<f32> };
@group(0) @binding(2) var<uniform> u: U;
struct V { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
    var p = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    var o: V;
    o.pos = vec4(p[i], 0.0, 1.0);
    o.uv = vec2((p[i].x + 1.0) * 0.5, (1.0 - p[i].y) * 0.5);
    return o;
}
@fragment fn fs(v: V) -> @location(0) vec4<f32> {
    let p = v.pos.xy;
    let q = vec2(u.m0.x * p.x + u.m0.y * p.y + u.m0.z, u.m1.x * p.x + u.m1.y * p.y + u.m1.z);
    let uv = q / u.size.xy;
    let c = textureSampleLevel(tex, smp, uv, 0.0);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return vec4(0.0);
    }
    return c;
}
"#;

/// Device objects shared by every window (created at the first use).
pub struct Shared {
    layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    pipelines: std::sync::Mutex<Vec<(wgpu::TextureFormat, wgpu::RenderPipeline)>>,
}

impl Shared {
    pub fn new(device: &wgpu::Device) -> Shared {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        Shared {
            shader: device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("overlay"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            }),
            // one texel per pixel when the view hasn't moved; smooth while it is transformed
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            layout,
            pipelines: Default::default(),
        }
    }

    fn pipeline(&self, device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        if let Some((_, p)) = self.pipelines.lock().unwrap().iter().find(|(f, _)| *f == format) {
            return p.clone();
        }
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[Some(&self.layout)],
            immediate_size: 0,
        });
        let p = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("overlay"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &self.shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &self.shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    // source-over, premultiplied, on encoded values
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        self.pipelines.lock().unwrap().push((format, p.clone()));
        p
    }
}

/// One viewport-sized layer.
pub struct Layer {
    texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    uniform: wgpu::Buffer,
    pub size: (u32, u32),
    /// What it shows (shape pointers, transform, size): re-rasterised only when it changes.
    key: Option<LayerKey>,
    empty: bool,
    /// The view transform last asked for and since when (a view change waits for `SETTLE`).
    view: Option<([f32; 6], Instant)>,
}

#[derive(PartialEq)]
struct LayerKey {
    shapes: Vec<usize>,
    transform: [f32; 6],
    size: (u32, u32),
    extra: u64,
}

impl Layer {
    fn new(gpu: &Gpu, shared: &Shared, (w, h): (u32, u32)) -> Layer {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay"),
            size: wgpu::Extent3d {
                width: w.max(1),
                height: h.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay"),
            size: 48,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay"),
            layout: &shared.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&shared.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        Layer {
            texture,
            bind,
            uniform,
            size: (w.max(1), h.max(1)),
            key: None,
            empty: true,
            view: None,
        }
    }

    /// Map the current screen to the texels rasterised with the layer's own transform.
    fn place(&self, gpu: &Gpu, t: &Affine) {
        let m = match &self.key {
            Some(k) => Affine { m: k.transform }.compose(&t.inverse()).m,
            None => Affine::IDENTITY.m,
        };
        let (w, h) = (self.size.0 as f32, self.size.1 as f32);
        let data: [f32; 12] = [m[0], m[1], m[2], 0.0, m[3], m[4], m[5], 0.0, w, h, 0.0, 0.0];
        gpu.queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&data));
    }

    /// Rasterise `shapes` with `screen_from_image` unless the layer already shows exactly that.
    /// `extra` = anything else the picture depends on (the colour mapping). A change of the view
    /// alone is drawn by transforming the current raster until the view has been still for
    /// `SETTLE` (unless `now`): returns when it is due then.
    #[allow(clippy::too_many_arguments)]
    fn update(
        &mut self,
        gpu: &Gpu,
        shapes: &[Arc<Shape>],
        t: &Affine,
        scale: f32,
        colors: ColorMap,
        base: Base,
        extra: u64,
        now: bool,
    ) -> Option<Instant> {
        let key = LayerKey {
            shapes: shapes.iter().map(|s| Arc::as_ptr(s) as usize).collect(),
            transform: t.m,
            size: self.size,
            extra,
        };
        let at = Instant::now();
        if self.view.is_none_or(|(m, _)| m != t.m) {
            self.view = Some((t.m, at));
        }
        if self.key.as_ref() == Some(&key) {
            self.place(gpu, t);
            return None;
        }
        let view_only = self
            .key
            .as_ref()
            .is_some_and(|k| k.shapes == key.shapes && k.size == key.size && k.extra == key.extra);
        let still_since = self.view.map_or(at, |(_, since)| since);
        if view_only && !now && at < still_since + SETTLE {
            self.place(gpu, t);
            return Some(still_since + SETTLE);
        }
        self.empty = shapes.is_empty();
        let pixmap = rasterize(shapes, t, scale, self.size, colors, base);
        gpu.queue.write_texture(
            self.texture.as_image_copy(),
            pixmap.data(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.size.0 * 4),
                rows_per_image: None,
            },
            wgpu::Extent3d {
                width: self.size.0,
                height: self.size.1,
                depth_or_array_layers: 1,
            },
        );
        self.key = Some(key);
        self.place(gpu, t);
        None
    }
}

/// Shapes rasterised at screen resolution: a premultiplied RGBA8 pixmap of `size`.
pub fn rasterize(
    shapes: &[Arc<Shape>],
    t: &Affine,
    scale: f32,
    size: (u32, u32),
    colors: ColorMap,
    base: Base,
) -> Pixmap {
    let [a, b, c, d, e, f] = t.m;
    // Affine is x' = a x + b y + c, y' = d x + e y + f; tiny-skia's row is (sx, ky, kx, sy, tx, ty)
    let ts = Transform::from_row(a, d, b, e, c, f);
    draw_shapes_banded(size.0.max(1), size.1.max(1), shapes, ts, scale, colors, base).expect("overlay size")
}

/// The two layers of a window.
pub struct Overlay {
    pub stat: Layer,
    pub active: Layer,
}

impl Overlay {
    /// Bring both layers up to date: `active` ids are drawn in the active layer (on top while the
    /// gesture lasts), every other shape in the static one; `pending` = a shape being drawn that
    /// is not in the document yet.
    #[allow(clippy::too_many_arguments)]
    pub fn sync(
        this: &mut Option<Overlay>,
        gpu: &Gpu,
        size: (u32, u32),
        shapes: &[Arc<Shape>],
        active: &[rv_core::edit::ObjectId],
        pending: Option<&Shape>,
        t: &Affine,
        scale: f32,
        colors: ColorMap,
        base: Base,
        colors_key: u64,
        now: bool,
    ) -> Option<Instant> {
        let shared = gpu.overlay.get_or_init(|| Shared::new(&gpu.device));
        if this.as_ref().is_none_or(|o| o.stat.size != size) {
            *this = Some(Overlay {
                stat: Layer::new(gpu, shared, size),
                active: Layer::new(gpu, shared, size),
            });
        }
        let o = this.as_mut().unwrap();
        let (moving, still): (Vec<Arc<Shape>>, Vec<Arc<Shape>>) =
            shapes.iter().cloned().partition(|s| active.contains(&s.id));
        let mut moving = moving;
        if let Some(p) = pending {
            moving.push(Arc::new(p.clone()));
        }
        let a = o.stat.update(gpu, &still, t, scale, colors, base, colors_key, now);
        // a pending shape is a fresh Arc every frame: the active layer is redrawn while drawing
        let b = o.active.update(gpu, &moving, t, scale, colors, base, colors_key, now);
        a.into_iter().chain(b).min()
    }

    pub fn draw(&self, gpu: &Gpu, pass: &mut wgpu::RenderPass<'_>, format: wgpu::TextureFormat) {
        let shared = gpu.overlay.get_or_init(|| Shared::new(&gpu.device));
        let p = shared.pipeline(&gpu.device, format);
        pass.set_pipeline(&p);
        for l in [&self.stat, &self.active] {
            if !l.empty {
                pass.set_bind_group(0, &l.bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
    }
}

/// Default kind for the drawing tool keys.
pub fn kind_for_key(k: &str) -> Option<ShapeKind> {
    Some(match k {
        "R" => ShapeKind::Rect,
        "E" => ShapeKind::Ellipse,
        "L" => ShapeKind::Line,
        "A" => ShapeKind::Arrow,
        "P" => ShapeKind::Pen,
        _ => return None,
    })
}
