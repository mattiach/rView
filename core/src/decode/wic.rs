//! Windows Imaging Component: HEIF/AVIF/JXR/RAW and fallback for formats the Rust decoders reject.
//! Also the scaled (DCT 1/2…1/8) JPEG preview used by the loader for > 24 MP files.

use super::{DecodeError, DecodedImage, Pixels, SizeU, SourceColor, SourceMeta, check_size};
use crate::formats::Format;
use crate::orient::{Orient, apply_orient};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, StructuredStorage::PROPVARIANT,
};
use windows::Win32::System::Variant::VT_UI2;
use windows::core::{Interface, w};

const WINCODEC_ERR_COMPONENTNOTFOUND: i32 = 0x88982F50_u32 as i32;

fn win_err(format: Format, e: windows::core::Error) -> DecodeError {
    if e.code().0 == WINCODEC_ERR_COMPONENTNOTFOUND {
        let (codec, store_url) = match format {
            Format::Heif => (
                "HEIF Image Extensions + HEVC Video Extensions",
                "ms-windows-store://search/?query=HEIF%20Image%20Extensions",
            ),
            Format::Avif => (
                "AV1 Video Extension",
                "ms-windows-store://search/?query=AV1%20Video%20Extension",
            ),
            Format::Raw => (
                "Raw Image Extension",
                "ms-windows-store://search/?query=Raw%20Image%20Extension",
            ),
            _ => return DecodeError::Unsupported,
        };
        return DecodeError::CodecMissing { codec, store_url };
    }
    DecodeError::Corrupt(e.message())
}

struct Wic {
    factory: IWICImagingFactory,
    frame: IWICBitmapFrameDecode,
    // The stream reads from the caller's buffer; keep it alive with the decoder.
    _stream: IWICStream,
}

fn open(bytes: &[u8]) -> windows::core::Result<Wic> {
    // SAFETY: plain COM calls; `bytes` outlives the returned objects because they never leave the
    // decode functions below, which borrow `bytes` for their whole duration.
    unsafe {
        // Worker threads use the MTA. S_FALSE / RPC_E_CHANGED_MODE are fine: COM is usable.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let stream = factory.CreateStream()?;
        stream.InitializeFromMemory(bytes)?;
        let decoder = factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        Ok(Wic {
            factory,
            frame,
            _stream: stream,
        })
    }
}

impl Wic {
    fn size(&self) -> windows::core::Result<(u32, u32)> {
        let (mut w, mut h) = (0, 0);
        // SAFETY: valid out pointers.
        unsafe { self.frame.GetSize(&mut w, &mut h)? };
        Ok((w, h))
    }

    /// `System.Photo.Orientation` (EXIF semantics), 1 if absent.
    fn orientation(&self) -> u32 {
        // SAFETY: PROPVARIANT is read only when its tag says VT_UI2.
        unsafe {
            let Ok(q) = self.frame.GetMetadataQueryReader() else {
                return 1;
            };
            let mut pv = PROPVARIANT::default();
            if q.GetMetadataByName(w!("System.Photo.Orientation"), &mut pv).is_err() {
                return 1;
            }
            let inner = &pv.Anonymous.Anonymous;
            if inner.vt == VT_UI2 {
                inner.Anonymous.uiVal as u32
            } else {
                1
            }
        }
    }

    fn icc(&self) -> Option<Vec<u8>> {
        // SAFETY: standard two-call pattern for GetColorContexts / GetProfileBytes.
        unsafe {
            let mut count = 0;
            self.frame.GetColorContexts(&mut [], &mut count).ok()?;
            let mut ctxs: Vec<Option<IWICColorContext>> =
                (0..count).map(|_| self.factory.CreateColorContext().ok()).collect();
            self.frame.GetColorContexts(&mut ctxs, &mut count).ok()?;
            for c in ctxs.into_iter().flatten() {
                if c.GetType().ok()? != WICColorContextProfile {
                    continue;
                }
                let mut len = 0;
                c.GetProfileBytes(&mut [], &mut len).ok()?;
                let mut buf = vec![0u8; len as usize];
                c.GetProfileBytes(&mut buf, &mut len).ok()?;
                return Some(buf);
            }
            None
        }
    }

    fn bits_per_channel(&self) -> u32 {
        // SAFETY: COM calls on valid objects.
        unsafe {
            let info = || -> windows::core::Result<u32> {
                let fmt = self.frame.GetPixelFormat()?;
                let pf: IWICPixelFormatInfo = self.factory.CreateComponentInfo(&fmt)?.cast()?;
                Ok(pf.GetBitsPerPixel()? / pf.GetChannelCount()?.max(1))
            };
            info().unwrap_or(8)
        }
    }
}

/// Displayed (oriented) size without decoding pixels.
pub fn probe(bytes: &[u8]) -> Option<(u32, u32)> {
    let wic = open(bytes).ok()?;
    let (w, h) = wic.size().ok()?;
    Some(Orient::from_exif(wic.orientation()).apply_size(w, h))
}

pub fn decode(bytes: &[u8], format: Format) -> Result<DecodedImage, DecodeError> {
    let e = |err| win_err(format, err);
    let wic = open(bytes).map_err(e)?;
    let (w, h) = wic.size().map_err(e)?;
    check_size(w, h)?;
    let deep = wic.bits_per_channel() > 8;
    let dst_fmt = if deep {
        GUID_WICPixelFormat64bppRGBA
    } else {
        GUID_WICPixelFormat32bppRGBA
    };
    let bpp = if deep { 8 } else { 4 };
    let stride = w as usize * bpp;
    let mut buf = vec![0u8; stride * h as usize];
    // SAFETY: buffer is exactly stride × height bytes.
    unsafe {
        let conv = wic.factory.CreateFormatConverter().map_err(e)?;
        conv.Initialize(
            &wic.frame,
            &dst_fmt,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
        .map_err(e)?;
        conv.CopyPixels(std::ptr::null(), stride as u32, &mut buf).map_err(e)?;
    }
    let o = Orient::from_exif(wic.orientation());
    let (pixels, (ow, oh)) = if deep {
        let v: Vec<u16> = buf.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        (Pixels::Rgba16(apply_orient(v, w, h, o)), o.apply_size(w, h))
    } else {
        (Pixels::Rgba8(apply_orient(buf, w, h, o)), o.apply_size(w, h))
    };
    let icc = wic.icc();
    Ok(DecodedImage {
        format,
        size: SizeU { w: ow, h: oh },
        pixels,
        color: SourceColor {
            assumed: icc.is_none(),
            icc: icc.map(Into::into),
            hdr: false,
        },
        meta: container_exif(bytes),
    })
}

/// EXIF of the containers kamadak-exif reads (HEIF/AVIF, TIFF-based RAW, JPEG, PNG, WebP), as a
/// TIFF block; other containers (JPEG XR) are left unread.
fn container_exif(bytes: &[u8]) -> SourceMeta {
    match exif::Reader::new().read_from_container(&mut std::io::Cursor::new(bytes)) {
        Ok(e) => SourceMeta {
            exif_read: true,
            exif: Some(e.buf().to_vec()),
            ..Default::default()
        },
        Err(exif::Error::NotFound(_)) => SourceMeta {
            exif_read: true,
            ..Default::default()
        },
        Err(_) => SourceMeta::default(),
    }
}

/// Reduced JPEG decode using the codec's DCT scaling: the smallest 1/2^k scale (k ≤ 3) whose
/// longest side is still ≥ `min_side`. Returns 8-bit RGBA with orientation applied.
pub fn decode_jpeg_scaled(bytes: &[u8], min_side: u32) -> Result<DecodedImage, DecodeError> {
    let e = |err| win_err(Format::Jpeg, err);
    let wic = open(bytes).map_err(e)?;
    let (w, h) = wic.size().map_err(e)?;
    check_size(w, h)?;
    let mut s = 8;
    while s > 1 && w.max(h) / s < min_side {
        s /= 2;
    }
    let (mut tw, mut th) = (w.div_ceil(s), h.div_ceil(s));
    // SAFETY: COM calls; buffer sized from the snapped dimensions and the native pixel format.
    let (rgba, tw, th) = unsafe {
        let t: IWICBitmapSourceTransform = wic.frame.cast().map_err(e)?;
        t.GetClosestSize(&mut tw, &mut th).map_err(e)?;
        let fmt = wic.frame.GetPixelFormat().map_err(e)?;
        let ch = match fmt {
            f if f == GUID_WICPixelFormat24bppBGR => 3,
            f if f == GUID_WICPixelFormat8bppGray => 1,
            _ => return Err(DecodeError::Unsupported),
        };
        let stride = (tw as usize * ch).next_multiple_of(4);
        let mut buf = vec![0u8; stride * th as usize];
        t.CopyPixels(
            std::ptr::null(),
            tw,
            th,
            &fmt,
            WICBitmapTransformRotate0,
            stride as u32,
            &mut buf,
        )
        .map_err(e)?;
        let row_px = tw as usize;
        let mut rgba = vec![255u8; row_px * th as usize * 4];
        for (src, dst) in buf.chunks_exact(stride).zip(rgba.chunks_exact_mut(row_px * 4)) {
            for (p, d) in src[..row_px * ch].chunks_exact(ch).zip(dst.chunks_exact_mut(4)) {
                if ch == 3 {
                    (d[0], d[1], d[2]) = (p[2], p[1], p[0]);
                } else {
                    (d[0], d[1], d[2]) = (p[0], p[0], p[0]);
                }
            }
        }
        (rgba, tw, th)
    };
    let o = Orient::from_exif(wic.orientation());
    let (ow, oh) = o.apply_size(tw, th);
    let icc = wic.icc();
    Ok(DecodedImage {
        format: Format::Jpeg,
        size: SizeU { w: ow, h: oh },
        pixels: Pixels::Rgba8(apply_orient(rgba, tw, th, o)),
        color: SourceColor {
            assumed: icc.is_none(),
            icc: icc.map(Into::into),
            hdr: false,
        },
        meta: SourceMeta::default(),
    })
}
