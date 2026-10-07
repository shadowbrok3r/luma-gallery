//! Non-destructive edit recipes. Crop follows EXIF orientation, rotation and flip.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            left: 0.0,
            top: 0.0,
            right: 1.0,
            bottom: 1.0,
        }
    }
}

impl Crop {
    pub fn full(self) -> bool {
        self == Self::default()
    }
    pub fn width(self) -> f32 {
        self.right - self.left
    }
    pub fn height(self) -> f32 {
        self.bottom - self.top
    }
    pub fn aspect(width: u32, height: u32, ratio: f32) -> Self {
        if width == 0 || height == 0 || ratio <= 0.0 {
            return Self::default();
        }
        let source = width as f32 / height as f32;
        let (w, h) = if source > ratio {
            (ratio / source, 1.0)
        } else {
            (1.0, source / ratio)
        };
        Self {
            left: (1.0 - w) * 0.5,
            top: (1.0 - h) * 0.5,
            right: (1.0 + w) * 0.5,
            bottom: (1.0 + h) * 0.5,
        }
    }
    pub fn pixels(self, width: u32, height: u32) -> (u32, u32, u32, u32) {
        if width == 0 || height == 0 {
            return (0, 0, 0, 0);
        }
        let x = ((self.left * width as f32).round() as u32).min(width.saturating_sub(1));
        let y = ((self.top * height as f32).round() as u32).min(height.saturating_sub(1));
        let right = ((self.right * width as f32).round() as u32).clamp(x + 1, width.max(x + 1));
        let bottom = ((self.bottom * height as f32).round() as u32).clamp(y + 1, height.max(y + 1));
        (x, y, right - x, bottom - y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub crop: Crop,
    pub rotation: u8,
    pub flip: bool,
    pub exposure: f32,
    pub contrast: f32,
    pub saturation: f32,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            crop: Crop::default(),
            rotation: 0,
            flip: false,
            exposure: 0.0,
            contrast: 1.0,
            saturation: 1.0,
        }
    }
}

impl Recipe {
    /// Matches PhotoEdits.colorMatrix: saturation, exposure, then centered contrast in sRGB.
    pub fn preview(self, source: &image::RgbaImage) -> image::RgbaImage {
        let mut pixels = source.clone();
        let gain = self.exposure.exp2() * self.contrast;
        let offset = 127.5 * (1.0 - self.contrast);
        for p in pixels.pixels_mut() {
            let luma = p[0] as f32 * 0.213 + p[1] as f32 * 0.715 + p[2] as f32 * 0.072;
            for channel in &mut p.0[..3] {
                *channel = ((luma + (*channel as f32 - luma) * self.saturation) * gain + offset)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
        pixels = match self.rotation % 4 {
            1 => image::imageops::rotate90(&pixels),
            2 => image::imageops::rotate180(&pixels),
            3 => image::imageops::rotate270(&pixels),
            _ => pixels,
        };
        if self.flip {
            image::imageops::flip_horizontal_in_place(&mut pixels);
        }
        pixels
    }
    pub fn output_size(self, width: u32, height: u32) -> (u32, u32) {
        let (width, height) = if self.rotation % 2 == 1 {
            (height, width)
        } else {
            (width, height)
        };
        let (_, _, w, h) = self.crop.pixels(width, height);
        (w, h)
    }
}

#[derive(Default)]
pub struct Waveform {
    pub peaks: Vec<f32>,
    pub step_ms: f64,
    pub status: String,
}

impl Waveform {
    /// Use the peak, not a decimated sample, so short transients survive timeline zoom-out.
    pub fn peak(&self, from_ms: f64, to_ms: f64) -> f32 {
        if self.step_ms <= 0.0 || self.peaks.is_empty() {
            return 0.0;
        }
        let from = (from_ms.max(0.0) / self.step_ms).floor() as usize;
        let to = ((to_ms.max(0.0) / self.step_ms).ceil() as usize)
            .max(from + 1)
            .min(self.peaks.len());
        self.peaks
            .get(from..to)
            .unwrap_or_default()
            .iter()
            .copied()
            .fold(0.0, f32::max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_crop_has_exact_pixel_dimensions_after_float_roundtrip() {
        for (w, h) in [(900, 600), (4032, 3024), (3060, 4080), (7028, 4688)] {
            let crop = Crop::aspect(w, h, 1.0);
            let roundtrip: Crop =
                serde_json::from_str(&serde_json::to_string(&crop).unwrap()).unwrap();
            let (x, y, cw, ch) = roundtrip.pixels(w, h);
            assert_eq!((cw, ch), (w.min(h), w.min(h)));
            assert!(x + cw <= w && y + ch <= h);
        }
    }

    #[test]
    fn clockwise_rotation_then_flip_has_correct_pixel_order() {
        let source = image::RgbaImage::from_fn(3, 2, |x, y| {
            image::Rgba([(x + y * 3) * 30, 0, 0, 255].map(|v| v as u8))
        });
        let recipe = Recipe {
            rotation: 1,
            flip: true,
            ..Default::default()
        };
        let result = recipe.preview(&source);
        assert_eq!(result.dimensions(), (2, 3));
        assert_eq!(result.get_pixel(0, 0)[0], 0);
        assert_eq!(result.get_pixel(1, 0)[0], 90);
        assert_eq!(result.get_pixel(1, 2)[0], 150);
        assert_eq!(recipe.output_size(3, 2), (2, 3));
    }

    #[test]
    fn audio_pooling_keeps_short_transients_at_every_zoom() {
        let w = Waveform {
            peaks: vec![0.0, 0.0, 0.95, 0.1, 0.0],
            step_ms: 20.0,
            status: String::new(),
        };
        assert_eq!(w.peak(0.0, 100.0), 0.95);
        assert_eq!(w.peak(40.0, 41.0), 0.95);
        assert_eq!(w.peak(0.0, 40.0), 0.0);
        assert_eq!(w.peak(100.0, 120.0), 0.0);
    }
}
