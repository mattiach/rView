//! D4 orientation group: rotation by quarter turns (clockwise) after an optional horizontal mirror.
//! Used for the viewer's view rotation and the Markup document orientation.

/// Transform = Rotate(quarter_turns · 90° CW) ∘ MirrorH^flip_h  (mirror applied first).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Orient {
    pub quarter_turns: u8,
    pub flip_h: bool,
}

impl Orient {
    pub const IDENTITY: Orient = Orient {
        quarter_turns: 0,
        flip_h: false,
    };

    /// EXIF Orientation tag (1..=8). Unknown values → identity.
    pub fn from_exif(v: u32) -> Orient {
        let (q, f) = match v {
            2 => (0, true),
            3 => (2, false),
            4 => (2, true),
            5 => (3, true),
            6 => (1, false),
            7 => (1, true),
            8 => (3, false),
            _ => (0, false),
        };
        Orient {
            quarter_turns: q,
            flip_h: f,
        }
    }

    /// Rotate the result by `k` quarter turns clockwise (negative = counter-clockwise).
    pub fn rotated(self, k: i8) -> Orient {
        Orient {
            quarter_turns: (self.quarter_turns as i8 + k).rem_euclid(4) as u8,
            ..self
        }
    }

    /// Mirror the result horizontally (M ∘ R^q ∘ M^f = R^-q ∘ M^(1-f)).
    pub fn mirrored(self) -> Orient {
        Orient {
            quarter_turns: (4 - self.quarter_turns) % 4,
            flip_h: !self.flip_h,
        }
    }

    /// Mirror the result vertically (= mirror then rotate 180°).
    pub fn flipped(self) -> Orient {
        self.mirrored().rotated(2)
    }

    pub fn swaps_axes(self) -> bool {
        self.quarter_turns % 2 == 1
    }

    pub fn apply_size(self, w: u32, h: u32) -> (u32, u32) {
        if self.swaps_axes() { (h, w) } else { (w, h) }
    }

    /// 2×2 matrix (row-major) for centred coordinates with y pointing down: p' = M·p.
    pub fn matrix(self) -> [[f32; 2]; 2] {
        let m = if self.flip_h {
            [[-1.0, 0.0], [0.0, 1.0]]
        } else {
            [[1.0, 0.0], [0.0, 1.0]]
        };
        // 90° CW with y down: (x, y) → (−y, x)
        let r: [[f32; 2]; 2] = match self.quarter_turns {
            0 => [[1.0, 0.0], [0.0, 1.0]],
            1 => [[0.0, -1.0], [1.0, 0.0]],
            2 => [[-1.0, 0.0], [0.0, -1.0]],
            _ => [[0.0, 1.0], [-1.0, 0.0]],
        };
        mul(r, m)
    }
}

/// Exact pixel transform of an RGBA buffer (4 samples per pixel), no resampling.
// Scattered writes, fine for one-off decode/export; tile it if it shows up in a profile.
pub fn apply_orient<T: Copy>(src: Vec<T>, w: u32, h: u32, o: Orient) -> Vec<T> {
    if o == Orient::IDENTITY {
        return src; // no copy in the common case
    }
    let (w, h) = (w as usize, h as usize);
    let dw = if o.swaps_axes() { h } else { w };
    let mut dst = src.to_vec();
    for y in 0..h {
        for x in 0..w {
            let x1 = if o.flip_h { w - 1 - x } else { x };
            let (dx, dy) = match o.quarter_turns {
                0 => (x1, y),
                1 => (h - 1 - y, x1),
                2 => (w - 1 - x1, h - 1 - y),
                _ => (y, w - 1 - x1),
            };
            let (s, d) = ((y * w + x) * 4, (dy * dw + dx) * 4);
            dst[d..d + 4].copy_from_slice(&src[s..s + 4]);
        }
    }
    dst
}

fn mul(a: [[f32; 2]; 2], b: [[f32; 2]; 2]) -> [[f32; 2]; 2] {
    let mut o = [[0.0; 2]; 2];
    for i in 0..2 {
        for j in 0..2 {
            o[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j];
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(o: Orient, p: [f32; 2]) -> [f32; 2] {
        let m = o.matrix();
        [m[0][0] * p[0] + m[0][1] * p[1], m[1][0] * p[0] + m[1][1] * p[1]]
    }

    #[test]
    fn group_identities() {
        let o = Orient::IDENTITY;
        assert_eq!(o.rotated(1).rotated(1).rotated(1).rotated(1), o);
        assert_eq!(o.rotated(-1), o.rotated(3));
        assert_eq!(o.mirrored().mirrored(), o);
        assert_eq!(o.flipped().flipped(), o);
        let x = Orient::from_exif(7);
        assert_eq!(x.mirrored().mirrored(), x);
    }

    #[test]
    fn pixel_transform_matches_matrix() {
        // 3×2 image, pixel value = index. Corners must move like the matrix says.
        let src: Vec<u8> = (0..6u8).flat_map(|i| [i, 0, 0, 0]).collect();
        for v in 1..=8 {
            let o = Orient::from_exif(v);
            let (dw, dh) = o.apply_size(3, 2);
            let dst = apply_orient(src.clone(), 3, 2, o);
            let at = |x: usize, y: usize| dst[(y * dw as usize + x) * 4];
            // original corners TL=0, TR=2, BL=3, BR=5 mapped by the matrix to display corners
            for (orig, p) in [(0u8, [-1.0, -1.0]), (2, [1.0, -1.0]), (3, [-1.0, 1.0]), (5, [1.0, 1.0])] {
                let q = apply(o, p);
                let x = if q[0] < 0.0 { 0 } else { dw as usize - 1 };
                let y = if q[1] < 0.0 { 0 } else { dh as usize - 1 };
                assert_eq!(at(x, y), orig, "exif {v}");
            }
        }
    }

    #[test]
    fn composition_matches_matrices() {
        let mirror = [[-1.0, 0.0], [0.0, 1.0]];
        let rot = Orient::IDENTITY.rotated(1).matrix();
        for v in 1..=8 {
            let o = Orient::from_exif(v);
            assert_eq!(o.mirrored().matrix(), mul(mirror, o.matrix()), "mirror {v}");
            assert_eq!(o.rotated(1).matrix(), mul(rot, o.matrix()), "rotate {v}");
        }
    }

    #[test]
    fn exif_corner_mapping() {
        // Corners in centred coords: TL(-1,-1) TR(1,-1) BL(-1,1) BR(1,1).
        // For each EXIF value: which ORIGINAL corner ends up at displayed TL and TR.
        let (tl, tr, bl, br) = ([-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.0, 1.0]);
        let table = [
            (1, tl, tr),
            (2, tr, tl),
            (3, br, bl),
            (4, bl, br),
            (5, tl, bl),
            (6, bl, tl),
            (7, br, tr),
            (8, tr, br),
        ];
        for (v, orig_at_tl, orig_at_tr) in table {
            let o = Orient::from_exif(v);
            assert_eq!(apply(o, orig_at_tl), tl, "exif {v} TL");
            assert_eq!(apply(o, orig_at_tr), tr, "exif {v} TR");
        }
    }
}
