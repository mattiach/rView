//! Pure viewer math: fit, zoom, pan, original size, view rotation → screen transform.
//! All sizes are physical pixels.

use rv_core::orient::Orient;
use rv_core::settings::{CropMode, Settings};

/// Affine 2×3: x' = a·x + b·y + c, y' = d·x + e·y + f.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub m: [f32; 6],
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        m: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
    };

    /// `self ∘ inner`: `inner` first.
    pub fn compose(&self, inner: &Affine) -> Affine {
        let [a, b, c, d, e, f] = self.m;
        let [p, q, r, s, t, u] = inner.m;
        Affine {
            m: [
                a * p + b * s,
                a * q + b * t,
                a * r + b * u + c,
                d * p + e * s,
                d * q + e * t,
                d * r + e * u + f,
            ],
        }
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.m;
        (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
    }

    pub fn inverse(&self) -> Affine {
        let [a, b, c, d, e, f] = self.m;
        let det = a * e - b * d;
        let (ia, ib, id, ie) = (e / det, -b / det, -d / det, a / det);
        Affine {
            m: [ia, ib, -(ia * c + ib * f), id, ie, -(id * c + ie * f)],
        }
    }
}

const MIN_REL: f32 = 0.01;
const MAX_REL: f32 = 500.0;

#[derive(Clone, Debug)]
pub struct View {
    pub orient: Orient,
    /// Screen pixels per image pixel.
    pub scale: f32,
    /// Offset of the image centre from the viewport centre (screen px).
    pub pan: (f32, f32),
    /// True while the view follows the window (fit); false after manual zoom.
    pub fitted: bool,
    pub original_size: bool,
    img: (u32, u32),
    vp: (u32, u32),
    /// Part of the image shown (x, y, w, h in image px): the Markup document's crop.
    region: Option<(f32, f32, f32, f32)>,
}

impl Default for View {
    fn default() -> Self {
        View::new()
    }
}

impl View {
    pub fn new() -> View {
        View {
            orient: Orient::IDENTITY,
            scale: 1.0,
            pan: (0.0, 0.0),
            fitted: true,
            original_size: false,
            img: (1, 1),
            vp: (1, 1),
            region: None,
        }
    }

    /// The shown part of the image: the region, or all of it.
    fn shown(&self) -> (f32, f32, f32, f32) {
        self.region.unwrap_or((0.0, 0.0, self.img.0 as f32, self.img.1 as f32))
    }

    /// Shown size after the view orientation.
    fn oriented(&self) -> (f32, f32) {
        let (_, _, w, h) = self.shown();
        if self.orient.swaps_axes() { (h, w) } else { (w, h) }
    }

    /// Show only this part of the image (None = all), then fit.
    pub fn set_region(&mut self, region: Option<(f32, f32, f32, f32)>, s: &Settings) {
        self.region = region;
        self.fit(s);
    }

    pub fn region(&self) -> Option<(f32, f32, f32, f32)> {
        self.region
    }

    pub fn fit_scale(&self, s: &Settings) -> f32 {
        let (w, h) = self.oriented();
        // ~1 px margin like the original
        let sx = (self.vp.0 as f32 - 1.0).max(1.0) / w;
        let sy = (self.vp.1 as f32 - 1.0).max(1.0) / h;
        let fit = match s.crop_mode {
            CropMode::Whole => sx.min(sy),
            CropMode::Height => sy,
            CropMode::Width => sx,
        };
        if s.past_actual_size_enabled { fit } else { fit.min(1.0) }
    }

    pub fn set_image(&mut self, w: u32, h: u32, s: &Settings) {
        self.img = (w.max(1), h.max(1));
        self.fit(s);
    }

    pub fn set_viewport(&mut self, w: u32, h: u32, s: &Settings) {
        self.vp = (w.max(1), h.max(1));
        if self.fitted && !self.original_size {
            self.fit(s);
        } else {
            self.clamp_pan();
        }
    }

    pub fn fit(&mut self, s: &Settings) {
        self.scale = self.fit_scale(s);
        self.pan = (0.0, 0.0);
        self.fitted = true;
        self.original_size = false;
    }

    /// Toggle 1 image px = 1 physical px.
    pub fn toggle_original_size(&mut self, s: &Settings) {
        if self.original_size {
            self.fit(s);
        } else {
            self.scale = 1.0;
            self.pan = (0.0, 0.0);
            self.fitted = false;
            self.original_size = true;
            self.clamp_pan();
        }
    }

    /// Multiply the zoom by `factor`, anchored at `cursor` (screen px) when allowed (CursorZoom).
    pub fn zoom_by(&mut self, factor: f32, cursor: Option<(f32, f32)>, s: &Settings) {
        let fit = self.fit_scale(s);
        let new = (self.scale * factor).clamp(fit * MIN_REL, fit * MAX_REL);
        if new == self.scale {
            return;
        }
        let ratio = new / self.scale;
        let anchor = cursor.filter(|_| s.cursor_zoom && new > fit);
        match anchor {
            Some((cx, cy)) => {
                // keep the image point under the cursor fixed
                let (ox, oy) = (cx - self.vp.0 as f32 / 2.0, cy - self.vp.1 as f32 / 2.0);
                self.pan = (ox - (ox - self.pan.0) * ratio, oy - (oy - self.pan.1) * ratio);
            }
            None => self.pan = (self.pan.0 * ratio, self.pan.1 * ratio),
        }
        self.scale = new;
        self.fitted = false;
        self.original_size = false;
        self.clamp_pan();
    }

    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.pan = (self.pan.0 + dx, self.pan.1 + dy);
        self.clamp_pan();
    }

    /// An axis smaller than the viewport is centred; a larger one can't leave a gap at the edges.
    fn clamp_pan(&mut self) {
        let (w, h) = self.oriented();
        let ex = ((w * self.scale - self.vp.0 as f32) / 2.0).max(0.0);
        let ey = ((h * self.scale - self.vp.1 as f32) / 2.0).max(0.0);
        self.pan = (self.pan.0.clamp(-ex, ex), self.pan.1.clamp(-ey, ey));
    }

    /// View rotation/mirror/flip: change orientation, then fit.
    pub fn set_orient(&mut self, o: Orient, s: &Settings) {
        self.orient = o;
        self.fit(s);
    }

    /// Image pixel coordinates → screen pixels.
    pub fn screen_from_image(&self) -> Affine {
        let m = self.orient.matrix();
        // the centre of the shown part goes to the viewport centre (+ pan)
        let (rx, ry, rw, rh) = self.shown();
        let (cx, cy) = (rx + rw / 2.0, ry + rh / 2.0);
        let s = self.scale;
        let mut tx = self.vp.0 as f32 / 2.0 + self.pan.0 - s * (m[0][0] * cx + m[0][1] * cy);
        let mut ty = self.vp.1 as f32 / 2.0 + self.pan.1 - s * (m[1][0] * cx + m[1][1] * cy);
        // Integer scales: snap to whole pixels so 100 % is pixel-exact.
        if s.fract() == 0.0 {
            tx = tx.round();
            ty = ty.round();
        }
        Affine {
            m: [s * m[0][0], s * m[0][1], tx, s * m[1][0], s * m[1][1], ty],
        }
    }

    /// Screen pixels → normalised device coordinates.
    pub fn ndc_from_image(&self) -> Affine {
        let a = self.screen_from_image().m;
        let (sx, sy) = (2.0 / self.vp.0 as f32, -2.0 / self.vp.1 as f32);
        Affine {
            m: [
                a[0] * sx,
                a[1] * sx,
                a[2] * sx - 1.0,
                a[3] * sy,
                a[4] * sy,
                a[5] * sy + 1.0,
            ],
        }
    }
}

/// Zoom factor for one step: 1 + ScaleFactor/100.
pub fn step_factor(s: &Settings) -> f32 {
    1.0 + s.scale_factor as f32 / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(img: (u32, u32), vp: (u32, u32)) -> (View, Settings) {
        let s = Settings::default();
        let mut v = View::new();
        v.vp = vp;
        v.set_image(img.0, img.1, &s);
        (v, s)
    }

    #[test]
    fn fit_modes() {
        let (mut v, mut s) = view((2000, 1000), (1001, 1001));
        assert!((v.scale - 0.5).abs() < 1e-6);
        s.crop_mode = CropMode::Height;
        v.fit(&s);
        assert!((v.scale - 1.0).abs() < 1e-6);
        // small image: upscaled unless past-actual-size is off
        let (mut v, mut s2) = view((100, 100), (1001, 501));
        assert!((v.scale - 5.0).abs() < 1e-6);
        s2.past_actual_size_enabled = false;
        v.fit(&s2);
        assert_eq!(v.scale, 1.0);
    }

    #[test]
    fn zoom_limits_and_cursor_anchor() {
        let (mut v, s) = view((1000, 1000), (1001, 1001));
        let fit = v.scale;
        for _ in 0..100 {
            v.zoom_by(step_factor(&s), None, &s);
        }
        assert!((v.scale / fit - 500.0).abs() < 1e-3);
        for _ in 0..200 {
            v.zoom_by(1.0 / step_factor(&s), None, &s);
        }
        assert!((v.scale / fit - 0.01).abs() < 1e-5);

        // Anchor: the image point under the cursor stays there.
        v.fit(&s);
        v.zoom_by(2.0, None, &s); // above fit → cursor zoom allowed
        let cursor = (800.0, 300.0);
        let inv = v.screen_from_image().inverse();
        let p = inv.apply(cursor.0, cursor.1);
        v.zoom_by(1.5, Some(cursor), &s);
        let q = v.screen_from_image().apply(p.0, p.1);
        // ≤ 1 px: integer scales snap the translation to whole pixels.
        assert!((q.0 - cursor.0).abs() <= 1.0 && (q.1 - cursor.1).abs() <= 1.0, "{q:?}");
    }

    #[test]
    fn below_fit_zoom_stays_centred() {
        let (mut v, s) = view((1000, 500), (1001, 1001));
        v.zoom_by(0.5, Some((10.0, 10.0)), &s);
        assert_eq!(v.pan, (0.0, 0.0));
    }

    #[test]
    fn pan_clamped_and_small_axis_centred() {
        let (mut v, s) = view((1000, 100), (500, 500));
        v.toggle_original_size(&s);
        v.pan_by(10_000.0, 10_000.0);
        assert_eq!(v.pan, (250.0, 0.0));
        v.toggle_original_size(&s);
        assert!(v.fitted && v.pan == (0.0, 0.0));
    }

    #[test]
    fn rotation_swaps_fit_and_maps_corners() {
        let (mut v, s) = view((2000, 1000), (1001, 1001));
        v.set_orient(Orient::IDENTITY.rotated(1), &s);
        assert!((v.scale - 0.5).abs() < 1e-6);
        // 90° CW: image TL (0,0) appears at the screen top-right of the image rect.
        let t = v.screen_from_image();
        let (x, y) = t.apply(0.0, 0.0);
        assert!(
            (x - (500.5 + 250.0)).abs() < 0.01 && (y - (500.5 - 500.0)).abs() < 0.01,
            "{x},{y}"
        );
        let back = t.inverse().apply(x, y);
        assert!(back.0.abs() < 1e-3 && back.1.abs() < 1e-3);
    }

    #[test]
    fn original_size_is_pixel_exact() {
        let (mut v, s) = view((301, 201), (1000, 800));
        v.toggle_original_size(&s);
        let m = v.screen_from_image().m;
        assert_eq!((m[0], m[4]), (1.0, 1.0));
        assert_eq!((m[2].fract(), m[5].fract()), (0.0, 0.0));
    }
}
