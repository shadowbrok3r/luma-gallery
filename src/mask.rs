//! Adapted from comfyui-android/src/mask.rs. Inpaint mask canvas: feathered UV-space brush strokes baked into an image alpha channel. Pure.

use std::io::Cursor;

/// A grayscale mask, 0 (unmasked) .. 255 (fully masked), row-major `w`x`h`.
pub struct MaskCanvas {
    pub w: u32,
    pub h: u32,
    pub buf: Vec<u8>,
}

impl MaskCanvas {
    pub fn new(w: u32, h: u32) -> Self {
        Self {
            w,
            h,
            buf: vec![0u8; w as usize * h as usize],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.buf.iter().all(|&v| v == 0)
    }

    /// Stamp feathered discs from `from` to `to` (uv 0..1) at ~radius/2 spacing.
    /// Paint max-blends toward `intensity`*255; erase min-blends toward 0. `soft` is the feather
    /// fraction; `intensity` (0..1) scales coverage for pressure-sensitive strokes.
    pub fn stroke(
        &mut self,
        from: (f32, f32),
        to: (f32, f32),
        radius_uv: f32,
        soft: f32,
        intensity: f32,
        erase: bool,
    ) {
        let r = radius_uv.max(0.0);
        if r <= 0.0 || self.w == 0 || self.h == 0 {
            return;
        }
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        let unit = self.w.min(self.h).max(1) as f32;
        let len =
            ((dx * self.w as f32 / unit).powi(2) + (dy * self.h as f32 / unit).powi(2)).sqrt();
        let step = (r * 0.5).max(1e-4);
        let count = (len / step).ceil().max(1.0) as u32;
        for i in 0..=count {
            let t = i as f32 / count as f32;
            self.stamp(from.0 + dx * t, from.1 + dy * t, r, soft, intensity, erase);
        }
    }

    /// One feathered disc centered at (cx, cy) in uv space, coverage scaled by `intensity`.
    fn stamp(&mut self, cx: f32, cy: f32, r: f32, soft: f32, intensity: f32, erase: bool) {
        let (w, h) = (self.w as f32, self.h as f32);
        let k = intensity.clamp(0.0, 1.0);
        let radius = r * w.min(h);
        let inner = radius * (1.0 - soft.clamp(0.0, 1.0));
        let min_x = ((cx * w - radius).floor() as i64).clamp(0, self.w as i64 - 1) as u32;
        let max_x = ((cx * w + radius).ceil() as i64).clamp(0, self.w as i64 - 1) as u32;
        let min_y = ((cy * h - radius).floor() as i64).clamp(0, self.h as i64 - 1) as u32;
        let max_y = ((cy * h + radius).ceil() as i64).clamp(0, self.h as i64 - 1) as u32;
        for py in min_y..=max_y {
            let v = (py as f32 + 0.5) / h;
            for px in min_x..=max_x {
                let u = (px as f32 + 0.5) / w;
                let d = (((u - cx) * w).powi(2) + ((v - cy) * h).powi(2)).sqrt();
                if d > radius {
                    continue;
                }
                let cov = if d <= inner || radius <= inner {
                    1.0
                } else {
                    1.0 - (d - inner) / (radius - inner)
                };
                let a = cov * k;
                let idx = py as usize * self.w as usize + px as usize;
                if erase {
                    self.buf[idx] = self.buf[idx].min(((1.0 - a) * 255.0).round() as u8);
                } else {
                    self.buf[idx] = self.buf[idx].max((a * 255.0).round() as u8);
                }
            }
        }
    }

    /// Bilinearly sampled mask intensity at uv (u, v).
    fn sample_bilinear(&self, u: f32, v: f32) -> u8 {
        if self.w == 0 || self.h == 0 {
            return 0;
        }
        let fx = (u * self.w as f32 - 0.5).clamp(0.0, (self.w - 1) as f32);
        let fy = (v * self.h as f32 - 0.5).clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let g = |x: u32, y: u32| self.buf[y as usize * self.w as usize + x as usize] as f32;
        let top = g(x0, y0) * (1.0 - tx) + g(x1, y0) * tx;
        let bot = g(x0, y1) * (1.0 - tx) + g(x1, y1) * tx;
        (top * (1.0 - ty) + bot * ty).round().clamp(0.0, 255.0) as u8
    }
}

/// A replayable stroke record; a mask is the deterministic rasterization of an ordered list.
pub struct StrokeRec {
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub radius_uv: f32,
    pub soft: f32,
    pub intensity: f32,
    pub erase: bool,
}

/// Rasterize an ordered stroke list into a fresh canvas (undo = rasterize minus the last stroke).
pub fn rasterize(w: u32, h: u32, strokes: &[StrokeRec]) -> MaskCanvas {
    let mut canvas = MaskCanvas::new(w, h);
    for s in strokes {
        canvas.stroke(s.from, s.to, s.radius_uv, s.soft, s.intensity, s.erase);
    }
    canvas
}

/// A brush shaped by pointer pressure: `radius_uv` and mask `intensity` (0..1).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Brush {
    pub radius_uv: f32,
    pub intensity: f32,
}

/// Shape `base_radius_uv` by pointer `force` (0..1, from `egui::Event::Touch`). No force keeps the
/// full radius and intensity; light contact paints thinner and fainter down to a fixed floor.
pub fn pressure_brush(base_radius_uv: f32, force: Option<f32>) -> Brush {
    match force {
        Some(f) => {
            let p = f.clamp(0.0, 1.0);
            Brush {
                radius_uv: base_radius_uv * (0.4 + 0.6 * p),
                intensity: 0.35 + 0.65 * p,
            }
        }
        None => Brush {
            radius_uv: base_radius_uv,
            intensity: 1.0,
        },
    }
}

/// Maximum inpaint zoom, as a multiple of the fitted (1x) view.
pub const MAX_ZOOM: f32 = 8.0;

/// Presentation-only zoom/pan for the paint surface. `zoom` is a multiple of the fitted size
/// (1 = fit), `pan` is the on-screen offset of the image center from the centered fit, in points.
/// Purely a view transform: it maps the image, overlay, and pointer coords, never the baked mask.
/// The fit rect is passed as `(center_x, center_y, w, h)` in screen points.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ViewXform {
    pub zoom: f32,
    pub pan: (f32, f32),
}

impl ViewXform {
    /// The fitted, un-panned view.
    pub const FIT: Self = Self {
        zoom: 1.0,
        pan: (0.0, 0.0),
    };

    /// On-screen image rect as `(min_x, min_y, w, h)`.
    pub fn view_rect(&self, fit: (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
        let (cx, cy, fw, fh) = fit;
        let (w, h) = (fw * self.zoom, fh * self.zoom);
        (cx + self.pan.0 - w * 0.5, cy + self.pan.1 - h * 0.5, w, h)
    }

    /// Image uv (0..1, clamped) under screen point `p`.
    pub fn screen_to_uv(&self, fit: (f32, f32, f32, f32), p: (f32, f32)) -> (f32, f32) {
        let (mx, my, w, h) = self.view_rect(fit);
        let u = if w > 0.0 { (p.0 - mx) / w } else { 0.0 };
        let v = if h > 0.0 { (p.1 - my) / h } else { 0.0 };
        (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0))
    }

    /// Screen point of image uv (inverse of `screen_to_uv`, unclamped).
    pub fn uv_to_screen(&self, fit: (f32, f32, f32, f32), uv: (f32, f32)) -> (f32, f32) {
        let (mx, my, w, h) = self.view_rect(fit);
        (mx + uv.0 * w, my + uv.1 * h)
    }

    /// Clamp zoom to `1..MAX_ZOOM` and pan so the image can't be dragged off-screen. `fit_size`
    /// is the fitted image size, `area_size` the full paint region; the fit is centered in it.
    pub fn clamped(mut self, fit_size: (f32, f32), area_size: (f32, f32)) -> Self {
        self.zoom = self.zoom.clamp(1.0, MAX_ZOOM);
        let mx = (fit_size.0 * self.zoom - area_size.0).abs() * 0.5;
        let my = (fit_size.1 * self.zoom - area_size.1).abs() * 0.5;
        self.pan = (self.pan.0.clamp(-mx, mx), self.pan.1.clamp(-my, my));
        self
    }

    /// Zoom by `factor` about screen focus `p` (image content under `p` stays put), then pan by
    /// `translate`, then clamp. `fit` is `(center_x, center_y, w, h)`, `area_size` the paint region.
    pub fn pinch(
        self,
        fit: (f32, f32, f32, f32),
        area_size: (f32, f32),
        factor: f32,
        p: (f32, f32),
        translate: (f32, f32),
    ) -> Self {
        let (fcx, fcy, fw, fh) = fit;
        let (u, v) = self.screen_to_uv(fit, p);
        let zoom = (self.zoom * factor).clamp(1.0, MAX_ZOOM);
        let (hw, hh) = (fw * zoom * 0.5, fh * zoom * 0.5);
        let cx = p.0 - (2.0 * u - 1.0) * hw + translate.0;
        let cy = p.1 - (2.0 * v - 1.0) * hh + translate.1;
        Self {
            zoom,
            pan: (cx - fcx, cy - fcy),
        }
        .clamped((fw, fh), area_size)
    }
}

/// Pointer tool as seen by the paint surface, surfaced from the android-activity motion-event
/// side channel (`getToolType`). `Unknown` covers pre-first-event and non-touch pointers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Finger,
    Stylus,
    Eraser,
    Palm,
    Mouse,
    Unknown,
}

/// Whether a paint stroke from `kind` is accepted under `stylus_only`. Rejects finger and palm
/// contacts; stylus, eraser, mouse, and unknown are accepted.
pub fn accept_paint(kind: PointerKind, stylus_only: bool) -> bool {
    if !stylus_only {
        return true;
    }
    !matches!(kind, PointerKind::Finger | PointerKind::Palm)
}

/// Decode `src_image` (PNG/JPEG), set each pixel's alpha to 255 minus the sampled mask, re-encode PNG.
pub fn bake_alpha(src_image: &[u8], mask: &MaskCanvas) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(src_image).map_err(|e| e.to_string())?;
    let mut rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    for y in 0..h {
        let v = (y as f32 + 0.5) / h as f32;
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32;
            let m = mask.sample_bilinear(u, v);
            rgba.get_pixel_mut(x, y)[3] = 255u8 - m;
        }
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn painted_png_keeps_hidden_rgb_and_uses_comfy_alpha_direction() {
        let mut source = Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(100, 50, image::Rgba([190, 75, 31, 255]))
            .write_to(&mut source, image::ImageFormat::Png)
            .unwrap();
        let mut mask = MaskCanvas::new(100, 50);
        mask.stroke((0.5, 0.5), (0.5, 0.5), 0.15, 0.0, 1.0, false);
        let decoded = image::load_from_memory(&bake_alpha(source.get_ref(), &mask).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.get_pixel(50, 25).0, [190, 75, 31, 0]);
        assert_eq!(decoded.get_pixel(0, 0).0, [190, 75, 31, 255]);
    }

    #[test]
    fn circular_brush_is_round_on_a_wide_image() {
        let mut canvas = MaskCanvas::new(400, 100);
        canvas.stroke((0.5, 0.5), (0.5, 0.5), 0.2, 0.0, 1.0, false);
        let row = canvas.buf[50 * 400..51 * 400]
            .iter()
            .filter(|&&v| v > 0)
            .count();
        let column = (0..100).filter(|&y| canvas.buf[y * 400 + 200] > 0).count();
        assert_eq!(row, column);
        assert!((39..=41).contains(&row));
    }

    #[test]
    fn pressure_erase_and_undo_preserve_independent_strokes() {
        let b = pressure_brush(0.1, Some(1.0));
        let light = pressure_brush(0.1, Some(0.1));
        assert!(light.radius_uv < b.radius_uv && light.intensity < b.intensity);
        let strokes = vec![
            StrokeRec {
                from: (0.2, 0.5),
                to: (0.8, 0.5),
                radius_uv: b.radius_uv,
                soft: 0.0,
                intensity: 1.0,
                erase: false,
            },
            StrokeRec {
                from: (0.5, 0.5),
                to: (0.5, 0.5),
                radius_uv: b.radius_uv,
                soft: 0.0,
                intensity: 1.0,
                erase: true,
            },
        ];
        assert_eq!(rasterize(100, 100, &strokes).buf[50 * 100 + 50], 0);
        assert_eq!(rasterize(100, 100, &strokes[..1]).buf[50 * 100 + 50], 255);
        assert!(!accept_paint(PointerKind::Finger, true));
        assert!(!accept_paint(PointerKind::Palm, true));
        assert!(accept_paint(PointerKind::Stylus, true));
        assert!(accept_paint(PointerKind::Eraser, true));
    }
}
