//! Lightroom-style color adjustments in linear light, shared by the editor preview and exports.
use serde::{Deserialize, Serialize};

/// Grid points per axis of exported and previewed lookup tables.
pub const LUT_SIZE: usize = 33;
const GREY: f32 = 0.18;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Adjust {
    /// Stops, -3 to 3.
    pub exposure: f32,
    /// The rest run from -1 to 1.
    pub brightness: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    /// Warmer above 0.
    pub temperature: f32,
    /// Magenta above 0, green below.
    pub tint: f32,
    pub vibrance: f32,
    pub saturation: f32,
}

/// Slider label, field and range for each control, in panel order.
pub const CONTROLS: [(&str, fn(&mut Adjust) -> &mut f32, f32, f32); 11] = [
    ("Exposure", |a| &mut a.exposure, -3.0, 3.0),
    ("Brightness", |a| &mut a.brightness, -1.0, 1.0),
    ("Contrast", |a| &mut a.contrast, -1.0, 1.0),
    ("Highlights", |a| &mut a.highlights, -1.0, 1.0),
    ("Shadows", |a| &mut a.shadows, -1.0, 1.0),
    ("Whites", |a| &mut a.whites, -1.0, 1.0),
    ("Blacks", |a| &mut a.blacks, -1.0, 1.0),
    ("Temperature", |a| &mut a.temperature, -1.0, 1.0),
    ("Tint", |a| &mut a.tint, -1.0, 1.0),
    ("Vibrance", |a| &mut a.vibrance, -1.0, 1.0),
    ("Saturation", |a| &mut a.saturation, -1.0, 1.0),
];

fn decode(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn encode(v: f32) -> f32 {
    if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn bell(x: f32, center: f32, width: f32) -> f32 {
    (-((x - center) / width).powi(2)).exp()
}

impl Adjust {
    pub fn is_neutral(&self) -> bool {
        *self == Self::default()
    }

    /// Channel gains for temperature and tint, normalized to keep luminance.
    fn balance(&self) -> [f32; 3] {
        let warm = 0.35 * self.temperature;
        let gains = [warm.exp2(), (-0.3 * self.tint).exp2(), (-warm).exp2()];
        let scale = luma(gains);
        gains.map(|g| g / scale)
    }

    /// Stops from middle grey after contrast, shadows and highlights.
    fn tone(&self, stops: f32) -> f32 {
        let mut s = stops * (0.6 * self.contrast).exp2();
        s += 1.2 * self.shadows * bell(s, -2.5, 1.8);
        s += self.highlights * bell(s, 1.5, 1.2);
        s
    }

    /// Adjusts scene-linear RGB, which may exceed 1, and returns display-encoded RGB in 0..=1.
    pub fn develop(&self, linear: [f32; 3]) -> [f32; 3] {
        let gains = self.balance();
        let exposure = self.exposure.exp2();
        let mut c = [0, 1, 2].map(|i| (linear[i] * gains[i] * exposure).max(0.0));
        let y = luma(c);
        if y > 1e-7 {
            let stops = (y / GREY).log2();
            let gain = (self.tone(stops) - stops).exp2();
            c = c.map(|v| v * gain);
        }
        let mut d = c.map(encode);
        // Whites and blacks move the display white and black points.
        let (black, white) = (-0.08 * self.blacks, 1.0 - 0.08 * self.whites);
        d = d.map(|v| (v - black) / (white - black));
        let y = luma(d);
        let (high, low) = (d[0].max(d[1]).max(d[2]), d[0].min(d[1]).min(d[2]));
        let chroma = if high > 1e-6 { ((high - low) / high).clamp(0.0, 1.0) } else { 0.0 };
        let amount = (1.0 + self.saturation) * (1.0 + self.vibrance * (1.0 - chroma));
        d = d.map(|v| y + (v - y) * amount);
        let gamma = (-0.6 * self.brightness).exp2();
        d.map(|v| v.clamp(0.0, 1.0).powf(gamma))
    }

    /// Adjusts display-encoded RGB in 0..=1.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        self.develop(rgb.map(decode))
    }

    /// Output for every grid point, red varying fastest, then green, then blue.
    fn grid(&self, size: usize) -> Vec<[f32; 3]> {
        let step = 1.0 / (size - 1) as f32;
        let mut out = Vec::with_capacity(size * size * size);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    out.push(self.apply([r as f32 * step, g as f32 * step, b as f32 * step]));
                }
            }
        }
        out
    }

    /// Adobe .cube text, as FFmpeg's lut3d filter reads it.
    pub fn cube(&self) -> String {
        let mut text = format!("TITLE \"Luma\"\nLUT_3D_SIZE {LUT_SIZE}\n");
        for [r, g, b] in self.grid(LUT_SIZE) {
            text.push_str(&format!("{r:.6} {g:.6} {b:.6}\n"));
        }
        text
    }

    /// RGBA8 strip `LUT_SIZE` squared wide and `LUT_SIZE` high: x is red plus blue times the size, y is green.
    pub fn strip(&self) -> Vec<u8> {
        let n = LUT_SIZE;
        let grid = self.grid(n);
        let mut pixels = vec![0u8; n * n * n * 4];
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let c = grid[(b * n + g) * n + r];
                    let at = (g * n * n + b * n + r) * 4;
                    for i in 0..3 {
                        pixels[at + i] = (c[i] * 255.0).round() as u8;
                    }
                    pixels[at + 3] = 255;
                }
            }
        }
        pixels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3], tolerance: f32) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() <= tolerance)
    }

    #[test]
    fn neutral_settings_change_nothing() {
        let neutral = Adjust::default();
        for c in [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [0.2, 0.5, 0.9], [0.73, 0.1, 0.33]] {
            assert!(close(neutral.apply(c), c, 1e-4), "{c:?} -> {:?}", neutral.apply(c));
        }
    }

    #[test]
    fn exposure_doubles_linear_light() {
        let up = Adjust { exposure: 1.0, ..Adjust::default() };
        let out = up.apply([0.4, 0.4, 0.4]);
        assert!((decode(out[0]) - 2.0 * decode(0.4)).abs() < 1e-3, "{out:?}");
    }

    #[test]
    fn every_control_keeps_grey_ramps_in_order() {
        for (name, field, low, high) in CONTROLS {
            for value in [low, high] {
                let mut adjust = Adjust::default();
                *field(&mut adjust) = value;
                let ramp: Vec<f32> = (0..=64).map(|i| adjust.apply([i as f32 / 64.0; 3])[1]).collect();
                assert!(ramp.windows(2).all(|w| w[1] >= w[0] - 1e-5), "{name} {value}: {ramp:?}");
            }
        }
    }

    #[test]
    fn controls_move_the_tones_they_name() {
        let grey = |a: Adjust, v: f32| a.apply([v; 3])[0];
        let set = |f: fn(&mut Adjust) -> &mut f32, v: f32| {
            let mut a = Adjust::default();
            *f(&mut a) = v;
            a
        };
        assert!(grey(set(|a| &mut a.shadows, 1.0), 0.15) > 0.2, "Shadows lift dark tones");
        assert!((grey(set(|a| &mut a.shadows, 1.0), 0.9) - 0.9).abs() < 0.03, "and leave highlights");
        assert!(grey(set(|a| &mut a.highlights, -1.0), 0.9) < 0.8, "Highlights pull bright tones down");
        assert!((grey(set(|a| &mut a.highlights, -1.0), 0.12) - 0.12).abs() < 0.02, "and leave shadows");
        let contrast = set(|a| &mut a.contrast, 1.0);
        assert!(grey(contrast, 0.25) < 0.22 && grey(contrast, 0.8) > 0.83, "Contrast spreads around middle grey");
        assert!(grey(set(|a| &mut a.blacks, 1.0), 0.0) > 0.05, "Blacks lift the black point");
        assert!(grey(set(|a| &mut a.whites, 1.0), 0.93) > 0.99, "Whites push the white point");
        assert!(grey(set(|a| &mut a.brightness, 1.0), 0.5) > 0.6, "Brightness lifts midtones");
        let warm = set(|a| &mut a.temperature, 1.0).apply([0.5; 3]);
        assert!(warm[0] > warm[1] && warm[1] > warm[2], "Warm grey turns orange: {warm:?}");
        let magenta = set(|a| &mut a.tint, 1.0).apply([0.5; 3]);
        assert!(magenta[1] < magenta[0] && magenta[1] < magenta[2], "Tint toward magenta: {magenta:?}");
        let pale = [0.6, 0.5, 0.45];
        let gray = set(|a| &mut a.saturation, -1.0).apply(pale);
        assert!(close(gray, [gray[0]; 3], 1e-4), "Saturation -1 is greyscale: {gray:?}");
        let vivid = set(|a| &mut a.vibrance, 0.5);
        let boost = |c: [f32; 3]| {
            let out = vivid.apply(c);
            (out[0] - out[2]) / (c[0] - c[2])
        };
        let strong = [0.7, 0.3, 0.1];
        assert!(boost(pale) > boost(strong) && boost(strong) > 1.0, "Vibrance favors muted colors: {} {}", boost(pale), boost(strong));
    }

    #[test]
    fn develop_recovers_linear_values_above_white() {
        let hot = [1.6, 1.6, 1.6];
        assert_eq!(Adjust::default().develop(hot), [1.0; 3], "Clipped without an adjustment");
        let down = Adjust { exposure: -1.0, ..Adjust::default() }.develop(hot);
        assert!(down[0] < 0.95, "Exposure brings RAW headroom back: {down:?}");
    }

    #[test]
    fn cube_and_strip_sample_the_same_grid() {
        let adjust = Adjust { exposure: 0.5, temperature: 0.4, saturation: 0.3, ..Adjust::default() };
        let cube = adjust.cube();
        let rows: Vec<&str> = cube.lines().skip(2).collect();
        assert_eq!(rows.len(), LUT_SIZE.pow(3));
        assert!(cube.starts_with("TITLE \"Luma\"\nLUT_3D_SIZE 33\n"));
        let n = LUT_SIZE;
        let (r, g, b) = (5, 20, 31);
        let row: Vec<f32> = rows[(b * n + g) * n + r].split(' ').map(|v| v.parse().unwrap()).collect();
        let strip = adjust.strip();
        let at = (g * n * n + b * n + r) * 4;
        for i in 0..3 {
            assert!((strip[at + i] as f32 / 255.0 - row[i]).abs() < 0.003, "{i}: {} vs {}", strip[at + i], row[i]);
        }
        let step = 1.0 / (n - 1) as f32;
        assert!(close([row[0], row[1], row[2]], adjust.apply([r as f32 * step, g as f32 * step, b as f32 * step]), 1e-5));
    }
}
