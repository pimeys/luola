//! Software framebuffer and 2D vector rasterizer.
//!
//! Pixels are `0x00RRGGBB` — the layout `softbuffer` hands to the window — and
//! live in one `Vec<u32>` that the caller blits wholesale once a frame. The
//! renderer runs at a small internal resolution (typically 960x540 or 640x360),
//! so every primitive can afford to work in plain scalar code.
//!
//! Two rules hold for the whole module:
//!
//! * **Everything clips.** Drawing never indexes out of bounds, whatever the
//!   coordinates are, and every call is O(visible pixels), not O(requested area).
//! * **Non-finite input is a silent no-op.** Camera math routinely hands over
//!   NaN or ±inf; those calls simply draw nothing. Nothing here panics, and no
//!   drawing entry point allocates (except `poly_fill`'s intersection scratch).
//!
//! Colours carry a fully saturated alpha channel (`rgb` packs `0x00RRGGBB`),
//! so `add`/`glow` saturate at 255 per channel and `blend` is a plain src-over.

use super::font;
use super::font::{GLYPH_H, GLYPH_SPACING, GLYPH_W};

const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;

/// Packs three channels into the `0x00RRGGBB` layout the framebuffer stores.
pub const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// Opaque black, the cleared framebuffer.
pub const BLACK: u32 = 0;

/// A CPU-side `u32` pixel buffer.
///
/// `w`/`h` are always positive and `px.len() == w * h`.
pub struct Framebuffer {
    w: i32,
    h: i32,
    px: Vec<u32>,
}

impl Framebuffer {
    /// Allocates a cleared framebuffer. Panics only if a dimension is not positive.
    pub fn new(w: i32, h: i32) -> Self {
        assert!(
            w > 0 && h > 0,
            "framebuffer size must be positive, got {w}x{h}"
        );
        Framebuffer {
            w,
            h,
            px: vec![0; (w as usize) * (h as usize)],
        }
    }

    pub fn width(&self) -> i32 {
        self.w
    }

    pub fn height(&self) -> i32 {
        self.h
    }

    pub fn pixels(&self) -> &[u32] {
        &self.px
    }

    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.px
    }

    /// Reallocates to a new size and clears to black.
    pub fn resize(&mut self, w: i32, h: i32) {
        assert!(
            w > 0 && h > 0,
            "framebuffer size must be positive, got {w}x{h}"
        );
        self.w = w;
        self.h = h;
        let n = (w as usize) * (h as usize);
        if self.px.len() != n {
            self.px.clear();
            self.px.resize(n, 0);
        }
        self.px.fill(0);
    }

    /// Fills every pixel with `c`.
    pub fn clear(&mut self, c: u32) {
        self.px.fill(pack24(c));
    }

    /// Writes `c`, clipped.
    #[inline]
    pub fn set(&mut self, x: i32, y: i32, c: u32) {
        let (w, h) = (self.w, self.h);
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        self.px[y as usize * w as usize + x as usize] = pack24(c);
    }

    /// Adds `c` per channel, saturating at 255. Clipped.
    #[inline]
    pub fn add(&mut self, x: i32, y: i32, c: u32) {
        let (w, h) = (self.w, self.h);
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        let i = y as usize * w as usize + x as usize;
        self.px[i] = add24(self.px[i], pack24(c));
    }

    /// Src-over blend of `c` at coverage `a` (clamped to `0..=1`). Clipped.
    #[inline]
    pub fn blend(&mut self, x: i32, y: i32, c: u32, a: f32) {
        let (w, h) = (self.w, self.h);
        if x < 0 || y < 0 || x >= w || y >= h || !a.is_finite() {
            return;
        }
        let sa = (a.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
        if sa == 0 {
            return;
        }
        let i = y as usize * w as usize + x as usize;
        if sa >= 255 {
            self.px[i] = pack24(c);
            return;
        }
        let s = pack24(c);
        let d = self.px[i];
        let ia = 255 - sa;
        let r = ((s >> 16 & 0xff) * sa + (d >> 16 & 0xff) * ia + 127) / 255;
        let g = ((s >> 8 & 0xff) * sa + (d >> 8 & 0xff) * ia + 127) / 255;
        let b = ((s & 0xff) * sa + (d & 0xff) * ia + 127) / 255;
        self.px[i] = (r << 16) | (g << 8) | b;
    }

    /// Bresenham line, both endpoints included.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) {
        if let Some((ax, ay, bx, by)) = self.clip_seg(x0 as i64, y0 as i64, x1 as i64, y1 as i64) {
            self.bresenham(ax, ay, bx, by, pack24(c), 1.0);
        }
    }

    /// `line` for float endpoints; non-finite endpoints draw nothing.
    pub fn line_f(&mut self, a: (f32, f32), b: (f32, f32), c: u32) {
        if !finite2(a) || !finite2(b) {
            return;
        }
        self.line(
            a.0.round() as i32,
            a.1.round() as i32,
            b.0.round() as i32,
            b.1.round() as i32,
            c,
        );
    }

    /// `line_f` drawn with src-over coverage `alpha`.
    pub fn line_fa(&mut self, a: (f32, f32), b: (f32, f32), c: u32, alpha: f32) {
        if !finite2(a) || !finite2(b) || !alpha.is_finite() {
            return;
        }
        let a2 = alpha.clamp(0.0, 1.0);
        if a2 <= 0.0 {
            return;
        }
        if let Some((ax, ay, bx, by)) = self.clip_seg(
            a.0.round() as i64,
            a.1.round() as i64,
            b.0.round() as i64,
            b.1.round() as i64,
        ) {
            self.bresenham(ax, ay, bx, by, pack24(c), a2);
        }
    }

    /// Filled axis-aligned rectangle, both edges inclusive. Clipped.
    pub fn rect_fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) {
        let (x0, x1) = minmax(x0, x1);
        let (y0, y1) = minmax(y0, y1);
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.w - 1);
        let y1 = y1.min(self.h - 1);
        if x0 > x1 || y0 > y1 {
            return;
        }
        let cc = pack24(c);
        let stride = self.w as usize;
        let run = (x1 - x0 + 1) as usize;
        for y in y0..=y1 {
            let base = y as usize * stride + x0 as usize;
            self.px[base..base + run].fill(cc);
        }
    }

    /// One pixel wide rectangle outline, both edges inclusive. Clipped.
    pub fn rect_stroke(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) {
        let (x0, x1) = minmax(x0, x1);
        let (y0, y1) = minmax(y0, y1);
        self.line(x0, y0, x1, y0, c);
        self.line(x1, y0, x1, y1, c);
        self.line(x1, y1, x0, y1, c);
        self.line(x0, y1, x0, y0, c);
    }

    /// Scanline fill with the nonzero winding rule, so concave and
    /// self-overlapping outlines stay solid. Non-finite vertices draw nothing.
    pub fn poly_fill(&mut self, pts: &[(f32, f32)], c: u32) {
        if pts.len() < 3 {
            return;
        }
        let mut ymin = f32::INFINITY;
        let mut ymax = f32::NEG_INFINITY;
        for p in pts {
            if !finite2(*p) {
                return;
            }
            ymin = ymin.min(p.1);
            ymax = ymax.max(p.1);
        }
        let ry0 = clamp_i(ymin.floor(), 0, self.h - 1);
        let ry1 = clamp_i(ymax.ceil(), 0, self.h - 1);
        if ry1 < ry0 {
            return;
        }
        let cc = pack24(c);
        let mut xs: Vec<(f32, i32)> = Vec::with_capacity(pts.len() + 4);
        for y in ry0..=ry1 {
            let sy = y as f32 + 0.5;
            xs.clear();
            for i in 0..pts.len() {
                let (px0, py0) = pts[i];
                let (px1, py1) = pts[(i + 1) % pts.len()];
                // Half-open in y: a vertex shared by two edges crosses once.
                let (lo, xl, dy, dx, dir) = if py0 <= py1 {
                    (py0, px0, py1 - py0, px1 - px0, 1)
                } else {
                    (py1, px1, py0 - py1, px0 - px1, -1)
                };
                if dy <= 0.0 || sy < lo || sy >= lo + dy {
                    continue;
                }
                let x = xl + dx * (sy - lo) / dy;
                if x.is_finite() {
                    xs.push((x, dir));
                }
            }
            if xs.len() < 2 {
                continue;
            }
            xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
            let mut winding = 0i32;
            let mut span = 0.0f32;
            for &(x, dir) in xs.iter() {
                let prev = winding;
                winding += dir;
                if prev == 0 {
                    if winding != 0 {
                        span = x;
                    }
                } else if winding == 0 {
                    // Pixel centres inside [span, x).
                    let xa = clamp_i((span - 0.5).ceil(), 0, self.w);
                    let xb = clamp_i((x - 0.5).ceil(), 0, self.w) - 1;
                    if xb >= xa {
                        self.hspan(xa, xb, y, cc);
                    }
                }
            }
        }
    }

    /// Filled circle; the edge is antialiased.
    pub fn circle_fill(&mut self, cx: f32, cy: f32, r: f32, c: u32) {
        if !cx.is_finite() || !cy.is_finite() || !r.is_finite() || r <= 0.0 {
            return;
        }
        self.disc(cx, cy, r, pack24(c));
    }

    /// One pixel wide circle outline; the edge is antialiased.
    pub fn circle_stroke(&mut self, cx: f32, cy: f32, r: f32, c: u32) {
        if !cx.is_finite() || !cy.is_finite() || !r.is_finite() || r <= 0.0 {
            return;
        }
        self.annulus(cx, cy, r, 1.0, pack24(c), None);
    }

    /// Arc of `thickness` pixels: `0 = +x`, angles grow clockwise (y is down).
    /// Rounded caps; a sweep of a full turn or more draws the whole ring.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, cx: f32, cy: f32, r: f32, thickness: f32, start: f32, end: f32, c: u32) {
        if !cx.is_finite()
            || !cy.is_finite()
            || !r.is_finite()
            || !thickness.is_finite()
            || !start.is_finite()
            || !end.is_finite()
            || r <= 0.0
            || thickness <= 0.0
        {
            return;
        }
        let cc = pack24(c);
        let sweep = end - start;
        if sweep.abs() >= TAU {
            self.annulus(cx, cy, r, thickness, cc, None);
            return;
        }
        self.annulus(cx, cy, r, thickness, cc, Some((start, sweep)));
        let half = thickness * 0.5;
        let (sa, ca) = start.sin_cos();
        let (sb, cb) = end.sin_cos();
        self.disc(cx + r * ca, cy + r * sa, half, cc);
        self.disc(cx + r * cb, cy + r * sb, half, cc);
    }

    /// Vertical run from `y0` to `y1`, both included, clipped. Terrain hot path.
    pub fn vspan(&mut self, x: i32, y0: i32, y1: i32, c: u32) {
        if x < 0 || x >= self.w {
            return;
        }
        let (y0, y1) = minmax(y0, y1);
        let y0 = y0.max(0);
        let y1 = y1.min(self.h - 1);
        if y0 > y1 {
            return;
        }
        let cc = pack24(c);
        let stride = self.w as usize;
        let x = x as usize;
        let start = y0 as usize * stride;
        let rows = (y1 - y0 + 1) as usize;
        for row in self.px[start..].chunks_exact_mut(stride).take(rows) {
            row[x] = cc;
        }
    }

    /// Multiplies every channel of a region by `factor` (clamped to `0..=1`).
    pub fn dim(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, factor: f32) {
        if !factor.is_finite() {
            return;
        }
        let fi = (factor.clamp(0.0, 1.0) * 4096.0 + 0.5) as u32;
        if fi >= 4096 {
            return;
        }
        let (x0, x1) = minmax(x0, x1);
        let (y0, y1) = minmax(y0, y1);
        let x0 = x0.max(0);
        let y0 = y0.max(0);
        let x1 = x1.min(self.w - 1);
        let y1 = y1.min(self.h - 1);
        if x0 > x1 || y0 > y1 {
            return;
        }
        let stride = self.w as usize;
        for y in y0..=y1 {
            let base = y as usize * stride + x0 as usize;
            for p in &mut self.px[base..base + (x1 - x0 + 1) as usize] {
                let v = *p;
                let r = ((v >> 16 & 0xff) * fi + 2048) >> 12;
                let g = ((v >> 8 & 0xff) * fi + 2048) >> 12;
                let b = ((v & 0xff) * fi + 2048) >> 12;
                *p = (r << 16) | (g << 8) | b;
            }
        }
    }

    /// Additive radial light: full `intensity` at the centre, fading to nothing
    /// at `radius`, saturating per channel. Clipped.
    pub fn glow(&mut self, x: i32, y: i32, radius: f32, c: u32, intensity: f32) {
        if !radius.is_finite() || !intensity.is_finite() || radius <= 0.0 || intensity <= 0.0 {
            return;
        }
        let (wf, hf) = ((self.w - 1) as f32, (self.h - 1) as f32);
        let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
        let (re, ie) = (radius.clamp(0.0, 1.0e18), intensity.clamp(0.0, 1.0e18));
        if cx + re < 0.0 || cx - re > wf || cy + re < 0.0 || cy - re > hf {
            return;
        }
        let x0 = clamp_i(cx - re, 0, self.w - 1);
        let x1 = clamp_i(cx + re, 0, self.w - 1);
        let y0 = clamp_i(cy - re, 0, self.h - 1);
        let y1 = clamp_i(cy + re, 0, self.h - 1);
        let r2 = re * re;
        if r2 <= 0.0 {
            return;
        }
        let inv_r2 = 1.0 / r2;
        let s = pack24(c);
        let (sr, sg, sb) = (s >> 16 & 0xff, s >> 8 & 0xff, s & 0xff);
        let stride = self.w as usize;
        for y in y0..=y1 {
            let dy = y as f32 + 0.5 - cy;
            let base = y as usize * stride;
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let d2 = dx * dx + dy * dy;
                if d2 > r2 {
                    continue;
                }
                let t = 1.0 - d2 * inv_r2;
                let f = t * t * ie;
                let fa = ((f * 255.0) as u32).min(255);
                if fa == 0 {
                    continue;
                }
                let i = base + x as usize;
                let add = ((sr * fa + 127) / 255) << 16
                    | ((sg * fa + 127) / 255) << 8
                    | ((sb * fa + 127) / 255);
                self.px[i] = add24(self.px[i], add);
            }
        }
    }

    /// Draws `s` with the 5x7 font, `(x, y)` at the glyph cell's top-left corner.
    pub fn text(&mut self, x: i32, y: i32, s: &str, c: u32) {
        self.text_scaled(x, y, s, 1, c);
    }

    /// `text` with every glyph pixel drawn as a `scale`x`scale` block.
    pub fn text_scaled(&mut self, x: i32, y: i32, s: &str, scale: i32, c: u32) {
        if scale <= 0 {
            return;
        }
        let cc = pack24(c);
        let sc = scale as i64;
        let adv = (GLYPH_W + GLYPH_SPACING) as i64 * sc;
        let mut cx = x as i64;
        let cy = y as i64;
        for ch in s.chars() {
            if let Some(g) = glyph_of(ch) {
                for (row, bits) in g.iter().enumerate() {
                    let mut col = 0usize;
                    let bits = *bits as u32;
                    while col < GLYPH_W as usize {
                        if bits & (1 << (GLYPH_W as usize - 1 - col)) == 0 {
                            col += 1;
                            continue;
                        }
                        let run0 = col;
                        while col < GLYPH_W as usize
                            && bits & (1 << (GLYPH_W as usize - 1 - col)) != 0
                        {
                            col += 1;
                        }
                        let rx0 = cx + run0 as i64 * sc;
                        let rx1 = cx + col as i64 * sc - 1;
                        let ry0 = cy + row as i64 * sc;
                        let ry1 = cy + (row as i64 + 1) * sc - 1;
                        self.rect_fill(rx0 as i32, ry0 as i32, rx1 as i32, ry1 as i32, cc);
                    }
                }
            }
            cx = cx.saturating_add(adv);
        }
    }

    /// Width in pixels of the ink `text_scaled` would draw for `s`.
    pub fn text_width(&self, s: &str, scale: i32) -> i32 {
        if scale <= 0 {
            return 0;
        }
        let n = s.chars().count() as i64;
        if n == 0 {
            return 0;
        }
        let w = n
            .saturating_mul((GLYPH_W + GLYPH_SPACING) as i64)
            .saturating_sub(GLYPH_SPACING as i64)
            .saturating_mul(scale as i64);
        w.clamp(0, i32::MAX as i64) as i32
    }

    /// Clips a segment to the framebuffer rect (Liang–Barsky); `None` if hidden.
    fn clip_seg(&self, x0: i64, y0: i64, x1: i64, y1: i64) -> Option<(i64, i64, i64, i64)> {
        let xmin = 0.0f64;
        let xmax = (self.w - 1) as f64;
        let ymin = 0.0f64;
        let ymax = (self.h - 1) as f64;
        let dx = (x1 - x0) as f64;
        let dy = (y1 - y0) as f64;
        let (fx0, fy0) = (x0 as f64, y0 as f64);
        let mut t0 = 0.0f64;
        let mut t1 = 1.0f64;
        for (p, q) in [
            (-dx, fx0 - xmin),
            (dx, xmax - fx0),
            (-dy, fy0 - ymin),
            (dy, ymax - fy0),
        ] {
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    if r > t1 {
                        return None;
                    }
                    if r > t0 {
                        t0 = r;
                    }
                } else {
                    if r < t0 {
                        return None;
                    }
                    if r < t1 {
                        t1 = r;
                    }
                }
            }
        }
        if t0 > t1 {
            return None;
        }
        let ax = (fx0 + t0 * dx).round().clamp(xmin, xmax) as i64;
        let ay = (fy0 + t0 * dy).round().clamp(ymin, ymax) as i64;
        let bx = (fx0 + t1 * dx).round().clamp(xmin, xmax) as i64;
        let by = (fy0 + t1 * dy).round().clamp(ymin, ymax) as i64;
        Some((ax, ay, bx, by))
    }

    /// Integer Bresenham over already-clipped endpoints.
    fn bresenham(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, c: u32, alpha: f32) {
        let solid = alpha >= 1.0;
        let mut x = x0;
        let mut y = y0;
        let dx = (x1 - x0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let dy = -(y1 - y0).abs();
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            if solid {
                self.set(x as i32, y as i32, c);
            } else {
                self.blend(x as i32, y as i32, c, alpha);
            }
            if x == x1 && y == y1 {
                return;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// Horizontal run, endpoints inclusive and assumed pre-clipped.
    #[inline]
    fn hspan(&mut self, x0: i32, x1: i32, y: i32, c: u32) {
        let stride = self.w as usize;
        let base = y as usize * stride + x0 as usize;
        self.px[base..base + (x1 - x0 + 1) as usize].fill(c);
    }

    /// Antialiased filled disc, bounding box already checked by the caller.
    fn disc(&mut self, cx: f32, cy: f32, r: f32, c: u32) {
        let ext = r + 1.0;
        let (wf, hf) = ((self.w - 1) as f32, (self.h - 1) as f32);
        if cx + ext < 0.0 || cx - ext > wf || cy + ext < 0.0 || cy - ext > hf {
            return;
        }
        let x0 = clamp_i(cx - ext, 0, self.w - 1);
        let x1 = clamp_i(cx + ext, 0, self.w - 1);
        let y0 = clamp_i(cy - ext, 0, self.h - 1);
        let y1 = clamp_i(cy + ext, 0, self.h - 1);
        for y in y0..=y1 {
            let dy = y as f32 + 0.5 - cy;
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let a = r + 0.5 - (dx * dx + dy * dy).sqrt();
                if a > 0.0 {
                    self.blend(x, y, c, a);
                }
            }
        }
    }

    /// Antialiased ring of `thickness` pixels, optionally restricted to a wedge
    /// `(start, sweep)`; the wedge is not feathered, the caller caps it.
    fn annulus(
        &mut self,
        cx: f32,
        cy: f32,
        r: f32,
        thickness: f32,
        c: u32,
        wedge: Option<(f32, f32)>,
    ) {
        let half = (thickness * 0.5) as f64;
        let (cx, cy, r) = (cx as f64, cy as f64, r as f64);
        let outer = r + half + 0.5;
        let inner = (r - half - 0.5).max(0.0);
        let (wf, hf) = ((self.w - 1) as f64, (self.h - 1) as f64);
        if cx + outer < 0.0 || cx - outer > wf || cy + outer < 0.0 || cy - outer > hf {
            return;
        }
        let y0 = clamp_d(cy - outer, 0.0, hf) as i32;
        let y1 = clamp_d(cy + outer, 0.0, hf) as i32;
        let t2 = half + 0.5;
        for y in y0..=y1 {
            let dy = y as f64 + 0.5 - cy;
            let o2 = outer * outer - dy * dy;
            if o2 <= 0.0 {
                continue;
            }
            let dox = o2.sqrt();
            let din = if dy.abs() >= inner {
                0.0
            } else {
                (inner * inner - dy * dy).sqrt()
            };
            for (xl, xr) in [(cx - dox, cx - din), (cx + din, cx + dox)] {
                let xa = clamp_d(xl, 0.0, wf) as i32;
                let xb = clamp_d(xr, 0.0, wf) as i32;
                if xa > xb {
                    continue;
                }
                for x in xa..=xb {
                    let dx = x as f64 + 0.5 - cx;
                    let a = t2 - ((dx * dx + dy * dy).sqrt() - r).abs();
                    if a <= 0.0 {
                        continue;
                    }
                    if let Some((start, sweep)) = wedge {
                        let th = (dy.atan2(dx)) as f32;
                        if !in_wedge(th, start, sweep) {
                            continue;
                        }
                    }
                    self.blend(x, y, c, (a as f32).min(1.0));
                }
            }
        }
    }
}

/// The glyph for `c`, falling back to the upper-case shape for lower case and
/// other mappings. Unknown characters have no glyph and just advance.
fn glyph_of(c: char) -> Option<[u8; GLYPH_H as usize]> {
    if let Some(g) = font::glyph(c) {
        return Some(g);
    }
    let mut it = c.to_uppercase();
    let up = it.next()?;
    if it.next().is_some() {
        return None;
    }
    font::glyph(up)
}

/// True for `theta` measured clockwise from `start` within a sweep of `sweep`.
fn in_wedge(theta: f32, start: f32, sweep: f32) -> bool {
    let rel = wrap_pi(theta as f64 - start as f64);
    if sweep >= 0.0 {
        rel >= 0.0 && rel <= sweep as f64
    } else {
        rel <= 0.0 && rel >= sweep as f64
    }
}

/// Wraps an angle in radians into `(-PI, PI]`, in f64 so wide differences keep
/// their sign even when the inputs are large.
fn wrap_pi(a: f64) -> f64 {
    let mut a = a % (TAU as f64);
    if a > PI as f64 {
        a -= TAU as f64;
    } else if a < -(PI as f64) {
        a += TAU as f64;
    }
    a
}

#[inline]
fn finite2(p: (f32, f32)) -> bool {
    p.0.is_finite() && p.1.is_finite()
}

#[inline]
fn minmax(a: i32, b: i32) -> (i32, i32) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Masks a colour to the `0x00RRGGBB` layout.
#[inline]
const fn pack24(c: u32) -> u32 {
    c & 0x00FF_FFFF
}

/// Saturating float→i32 clamp; NaN collapses onto `lo`.
#[inline]
fn clamp_i(v: f32, lo: i32, hi: i32) -> i32 {
    (v as i32).clamp(lo, hi)
}

#[inline]
fn clamp_d(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Per-channel saturating add of two packed colours.
#[inline]
fn add24(d: u32, s: u32) -> u32 {
    let r = ((d >> 16 & 0xff) + (s >> 16 & 0xff)).min(255);
    let g = ((d >> 8 & 0xff) + (s >> 8 & 0xff)).min(255);
    let b = ((d & 0xff) + (s & 0xff)).min(255);
    (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink(fb: &Framebuffer) -> usize {
        fb.pixels().iter().filter(|&&p| p != 0).count()
    }

    #[test]
    fn vspan_and_rect_fill_clip_at_edges() {
        let mut fb = Framebuffer::new(8, 8);
        let white = rgb(255, 255, 255);

        fb.vspan(4, -100, 100, white);
        assert_eq!(ink(&fb), 8, "span crossing both edges fills its column");

        fb.clear(BLACK);
        fb.vspan(-1, 0, 7, white);
        fb.vspan(8, 0, 7, white);
        assert_eq!(ink(&fb), 0, "off-screen columns draw nothing");

        fb.vspan(3, 6, 3, white);
        assert_eq!(
            ink(&fb),
            4,
            "inverted endpoints are ordered, ends inclusive"
        );

        fb.clear(BLACK);
        fb.rect_fill(-5, -5, 2, 2, white);
        assert_eq!(ink(&fb), 9, "corner rect clips to 3x3");
        fb.clear(BLACK);
        fb.rect_fill(10, 10, 20, 20, white);
        assert_eq!(ink(&fb), 0, "rect past the far edge draws nothing");
        fb.rect_fill(6, 6, 100, 100, white);
        assert_eq!(ink(&fb), 4, "rect hanging over the far edge clips");

        fb.line_f((f32::NAN, 0.0), (5.0, 5.0), white);
        fb.line(0, 0, i32::MAX, i32::MIN, white);
        fb.circle_fill(f32::INFINITY, 0.0, 3.0, white);
        fb.arc(f32::NAN, 0.0, 5.0, 2.0, 0.0, 1.0, white);
        fb.poly_fill(&[(0.0, 0.0), (f32::NAN, 1.0), (1.0, 1.0)], white);
        fb.text(-1000, -1000, "CLIPPED", white);
        assert_eq!(
            fb.pixels().len(),
            64,
            "non-finite input never resizes or panics"
        );
    }

    #[test]
    fn poly_fill_concave_area() {
        let mut fb = Framebuffer::new(16, 16);
        let white = rgb(255, 255, 255);
        // L shape: an 8x8 square with a 4x4 bite out of the top right.
        let l = [
            (0.0f32, 0.0f32),
            (8.0, 0.0),
            (8.0, 4.0),
            (4.0, 4.0),
            (4.0, 8.0),
            (0.0, 8.0),
        ];
        fb.poly_fill(&l, white);
        assert_eq!(ink(&fb), 48, "concave polygon keeps its exact area");
        assert!(ink(&fb) < 64, "the bite stays unfilled");

        // Tracing the same square twice must not punch a hole (nonzero winding).
        let twice = [
            (0.0f32, 0.0f32),
            (8.0, 0.0),
            (8.0, 8.0),
            (0.0, 8.0),
            (0.0, 0.0),
            (8.0, 0.0),
            (8.0, 8.0),
            (0.0, 8.0),
        ];
        fb.clear(BLACK);
        fb.poly_fill(&twice, white);
        assert_eq!(ink(&fb), 64, "self-overlapping outline stays solid");
    }

    #[test]
    fn text_width_grows_with_text() {
        let fb = Framebuffer::new(4, 4);
        let s = "HELLO, world! 0123";
        assert_eq!(fb.text_width("", 1), 0);
        let mut prev = 0;
        for i in 1..=s.len() {
            let w = fb.text_width(&s[..i], 1);
            assert!(w > prev, "appending a glyph must widen the run ({i})");
            prev = w;
        }
        assert_eq!(fb.text_width("AB", 1), 11);
        assert!(fb.text_width("ABC", 2) > fb.text_width("ABC", 1));
        assert_eq!(fb.text_width("ABC", 0), 0);
    }

    #[test]
    fn rgb_packs_channels() {
        assert_eq!(rgb(0x12, 0x34, 0x56), 0x0012_3456);
        assert_eq!(rgb(255, 255, 255), 0x00FF_FFFF);
        assert_eq!(rgb(1, 0, 0), 0x0001_0000);
        assert_eq!(rgb(0, 0, 1), 1);
        assert_eq!(BLACK, 0);
    }
}
