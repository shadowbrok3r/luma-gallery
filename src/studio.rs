//! Video edit projects: ordered segments of one source plus overlay layers above it.
use crate::edit::Crop;
use serde::{Deserialize, Serialize};

pub const MIN_CLIP: i64 = 100;
/// Slide distance as a fraction of the frame.
pub const SLIDE: f32 = 0.08;
pub const PALETTE: [[u8; 3]; 7] = [
    [255, 61, 139],
    [43, 226, 214],
    [160, 110, 255],
    [255, 184, 64],
    [140, 220, 90],
    [80, 170, 255],
    [255, 120, 90],
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Motion {
    #[default]
    None,
    Fade,
    SlideUp,
    SlideDown,
    SlideLeft,
    SlideRight,
}

impl Motion {
    pub const ALL: [(Motion, &'static str); 6] = [
        (Motion::None, "None"),
        (Motion::Fade, "Fade"),
        (Motion::SlideUp, "Up"),
        (Motion::SlideDown, "Down"),
        (Motion::SlideLeft, "Left"),
        (Motion::SlideRight, "Right"),
    ];
    /// Direction of travel in frame fractions.
    pub fn travel(self) -> (f32, f32) {
        match self {
            Motion::SlideUp => (0.0, -SLIDE),
            Motion::SlideDown => (0.0, SLIDE),
            Motion::SlideLeft => (-SLIDE, 0.0),
            Motion::SlideRight => (SLIDE, 0.0),
            _ => (0.0, 0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Align {
    Left,
    #[default]
    Center,
    Right,
}

/// Text appearance; lengths are in ems unless noted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextStyle {
    pub text: String,
    pub font: String,
    /// Em size as a fraction of the frame height.
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub uppercase: bool,
    pub color: [u8; 4],
    pub align: Align,
    pub outline: f32,
    pub outline_color: [u8; 4],
    pub shadow: f32,
    pub shadow_color: [u8; 4],
    pub shadow_offset: [f32; 2],
    pub background: [u8; 4],
    pub radius: f32,
    pub padding: f32,
    pub spacing: f32,
    pub line_height: f32,
    /// Maximum line width as a fraction of the frame width.
    pub wrap: f32,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            text: "Text".into(),
            font: String::new(),
            size: 0.075,
            bold: true,
            italic: false,
            underline: false,
            uppercase: false,
            color: [255, 255, 255, 255],
            align: Align::Center,
            outline: 0.07,
            outline_color: [0, 0, 0, 255],
            shadow: 0.0,
            shadow_color: [0, 0, 0, 160],
            shadow_offset: [0.04, 0.06],
            background: [0, 0, 0, 0],
            radius: 0.25,
            padding: 0.3,
            spacing: 0.0,
            line_height: 1.0,
            wrap: 0.86,
        }
    }
}

/// Named looks that replace colors, outline, shadow and box but keep the words and size.
pub fn presets() -> Vec<(&'static str, TextStyle)> {
    let base = TextStyle::default();
    let plain = TextStyle {
        outline: 0.0,
        ..base.clone()
    };
    vec![
        ("Classic", base.clone()),
        (
            "Subtitle",
            TextStyle {
                bold: false,
                background: [0, 0, 0, 165],
                radius: 0.18,
                padding: 0.28,
                ..plain.clone()
            },
        ),
        (
            "Glass",
            TextStyle {
                background: [255, 255, 255, 52],
                radius: 0.45,
                padding: 0.42,
                shadow: 0.08,
                shadow_color: [0, 0, 0, 120],
                shadow_offset: [0.0, 0.03],
                ..plain.clone()
            },
        ),
        (
            "Yellow",
            TextStyle {
                color: [255, 221, 0, 255],
                outline: 0.09,
                ..base.clone()
            },
        ),
        (
            "Neon",
            TextStyle {
                color: [255, 214, 236, 255],
                shadow: 0.22,
                shadow_color: [255, 61, 139, 255],
                shadow_offset: [0.0, 0.0],
                ..plain.clone()
            },
        ),
        (
            "Aqua",
            TextStyle {
                color: [43, 226, 214, 255],
                shadow: 0.06,
                shadow_color: [0, 0, 0, 200],
                ..plain.clone()
            },
        ),
        (
            "Meme",
            TextStyle {
                color: [0, 0, 0, 255],
                background: [255, 255, 255, 255],
                uppercase: true,
                radius: 0.12,
                padding: 0.22,
                ..plain.clone()
            },
        ),
        (
            "Shadow",
            TextStyle {
                shadow: 0.12,
                shadow_color: [0, 0, 0, 230],
                shadow_offset: [0.05, 0.08],
                ..plain
            },
        ),
    ]
}

impl TextStyle {
    pub fn apply_preset(&mut self, preset: &TextStyle) {
        let keep = (
            std::mem::take(&mut self.text),
            std::mem::take(&mut self.font),
            self.size,
            self.align,
            self.wrap,
            self.italic,
            self.underline,
        );
        *self = preset.clone();
        (
            self.text,
            self.font,
            self.size,
            self.align,
            self.wrap,
            self.italic,
            self.underline,
        ) = keep;
    }
}

/// A still image overlay decoded to `path`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Picture {
    pub uri: String,
    pub name: String,
    pub path: String,
    /// Width as a fraction of the frame width.
    pub width: f32,
    /// Source height divided by width.
    pub aspect: f32,
    /// Corner radius as a fraction of the shorter side.
    pub radius: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Body {
    Text(TextStyle),
    Picture(Picture),
}

impl Default for Body {
    fn default() -> Self {
        Body::Text(TextStyle::default())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clip {
    pub id: u64,
    pub start: i64,
    pub end: i64,
    /// Center in frame fractions.
    pub x: f32,
    pub y: f32,
    /// Degrees clockwise.
    pub rotation: f32,
    pub opacity: f32,
    pub enter: Motion,
    pub enter_ms: i64,
    pub exit: Motion,
    pub exit_ms: i64,
    pub body: Body,
}

impl Default for Clip {
    fn default() -> Self {
        Self {
            id: 0,
            start: 0,
            end: 3000,
            x: 0.5,
            y: 0.75,
            rotation: 0.0,
            opacity: 1.0,
            enter: Motion::None,
            enter_ms: 400,
            exit: Motion::None,
            exit_ms: 400,
            body: Body::default(),
        }
    }
}

fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - p) * (1.0 - p)
}

impl Clip {
    pub fn duration(&self) -> i64 {
        self.end - self.start
    }
    pub fn kind(&self) -> LayerKind {
        match self.body {
            Body::Text(_) => LayerKind::Text,
            Body::Picture(_) => LayerKind::Picture,
        }
    }
    pub fn text(&self) -> Option<&TextStyle> {
        match &self.body {
            Body::Text(style) => Some(style),
            Body::Picture(_) => None,
        }
    }
    pub fn text_mut(&mut self) -> Option<&mut TextStyle> {
        match &mut self.body {
            Body::Text(style) => Some(style),
            Body::Picture(_) => None,
        }
    }
    pub fn label(&self) -> String {
        match &self.body {
            Body::Text(style) => style.text.lines().next().unwrap_or_default().to_owned(),
            Body::Picture(picture) => picture.name.clone(),
        }
    }
    /// Enter and exit durations, shortened to fit inside the clip.
    pub fn motion_ms(&self) -> (i64, i64) {
        let enter = if self.enter == Motion::None {
            0
        } else {
            self.enter_ms.max(0)
        };
        let exit = if self.exit == Motion::None {
            0
        } else {
            self.exit_ms.max(0)
        };
        let total = enter + exit;
        if total > self.duration() && total > 0 {
            let d = self.duration() as f64;
            let e = (enter as f64 * d / total as f64).floor() as i64;
            (e, (d as i64 - e).max(0))
        } else {
            (enter, exit)
        }
    }
    /// Alpha multiplier and offset in frame fractions at timeline time `t`; matches the export expressions.
    pub fn envelope(&self, t: i64) -> (f32, f32, f32) {
        let (enter, exit) = self.motion_ms();
        let mut alpha = 1.0;
        let (mut dx, mut dy) = (0.0, 0.0);
        if enter > 0 {
            let p = ((t - self.start) as f32 / enter as f32).clamp(0.0, 1.0);
            alpha *= p;
            let (tx, ty) = self.enter.travel();
            let rest = 1.0 - ease_out(p);
            dx -= tx * rest;
            dy -= ty * rest;
        }
        if exit > 0 {
            let q = ((t - (self.end - exit)) as f32 / exit as f32).clamp(0.0, 1.0);
            alpha *= 1.0 - q;
            let (tx, ty) = self.exit.travel();
            dx += tx * q * q;
            dy += ty * q * q;
        }
        (alpha, dx, dy)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerKind {
    #[default]
    Text,
    Picture,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layer {
    pub id: u64,
    pub kind: LayerKind,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub color: usize,
    /// Sorted by start; never overlapping.
    pub clips: Vec<Clip>,
}

impl Layer {
    pub fn rgb(&self) -> [u8; 3] {
        PALETTE[self.color % PALETTE.len()]
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Segment {
    pub id: u64,
    pub source_in: i64,
    pub source_out: i64,
    pub fade_in: i64,
    pub fade_out: i64,
    /// Fades through white instead of black.
    pub white: bool,
}

impl Segment {
    pub fn length(&self) -> i64 {
        self.source_out - self.source_in
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Audio {
    pub keep: bool,
    pub volume: f32,
    pub fade_in: i64,
    pub fade_out: i64,
}

impl Default for Audio {
    fn default() -> Self {
        Self {
            keep: true,
            volume: 1.0,
            fade_in: 0,
            fade_out: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    pub version: u32,
    pub source: String,
    pub name: String,
    pub duration: i64,
    /// Displayed source size after rotation metadata.
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_audio: bool,
    /// Timeline order; source ranges increase and never overlap.
    pub segments: Vec<Segment>,
    /// Bottom to top.
    pub layers: Vec<Layer>,
    pub crop: Crop,
    pub audio: Audio,
    pub next_id: u64,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: 1,
            source: String::new(),
            name: String::new(),
            duration: 0,
            width: 1920,
            height: 1080,
            fps: 30.0,
            has_audio: true,
            segments: vec![],
            layers: vec![],
            crop: Crop::default(),
            audio: Audio::default(),
            next_id: 0,
        }
    }
}

/// Nearest point within `tolerance` of `t`.
pub fn snap(t: i64, points: &[i64], tolerance: i64) -> Option<i64> {
    points
        .iter()
        .copied()
        .filter(|p| (p - t).abs() <= tolerance)
        .min_by_key(|p| (p - t).abs())
}

impl Project {
    pub fn new(source: &str, name: &str, duration: i64, width: u32, height: u32) -> Self {
        let mut project = Self {
            source: source.into(),
            name: name.into(),
            duration: duration.max(MIN_CLIP),
            width: width.max(2),
            height: height.max(2),
            ..Self::default()
        };
        let id = project.id();
        project.segments.push(Segment {
            id,
            source_out: project.duration,
            ..Segment::default()
        });
        project
    }

    pub fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn length(&self) -> i64 {
        self.segments.iter().map(Segment::length).sum()
    }

    /// Timeline start of every segment.
    pub fn starts(&self) -> Vec<i64> {
        self.segments
            .iter()
            .scan(0, |at, s| {
                let start = *at;
                *at += s.length();
                Some(start)
            })
            .collect()
    }

    /// Segment index and offset into it at timeline time `t`.
    pub fn locate(&self, t: i64) -> Option<(usize, i64)> {
        let mut at = 0;
        let last = self.segments.len().checked_sub(1)?;
        for (index, segment) in self.segments.iter().enumerate() {
            if t < at + segment.length() || index == last {
                return Some((index, (t - at).clamp(0, segment.length())));
            }
            at += segment.length();
        }
        None
    }

    pub fn source_time(&self, t: i64) -> i64 {
        self.locate(t)
            .map(|(i, offset)| self.segments[i].source_in + offset)
            .unwrap_or(0)
    }

    /// Timeline time showing source time `source`; a gap maps to the following segment.
    pub fn timeline_time(&self, source: i64) -> i64 {
        let mut at = 0;
        for segment in &self.segments {
            if source < segment.source_out {
                return at + (source - segment.source_in).clamp(0, segment.length());
            }
            at += segment.length();
        }
        at
    }

    /// Source ranges to play in order.
    pub fn ranges(&self) -> Vec<[i64; 2]> {
        self.segments
            .iter()
            .map(|s| [s.source_in, s.source_out])
            .collect()
    }

    pub fn split(&mut self, t: i64) -> bool {
        let Some((index, offset)) = self.locate(t) else {
            return false;
        };
        let segment = self.segments[index].clone();
        if offset < MIN_CLIP || segment.length() - offset < MIN_CLIP {
            return false;
        }
        let id = self.id();
        let cut = segment.source_in + offset;
        self.segments[index].source_out = cut;
        self.segments[index].fade_out = 0;
        self.segments.insert(
            index + 1,
            Segment {
                id,
                source_in: cut,
                fade_in: 0,
                ..segment
            },
        );
        true
    }

    /// Removes a segment and closes the gap in every overlay layer.
    pub fn remove_segment(&mut self, index: usize) -> bool {
        if self.segments.len() < 2 || index >= self.segments.len() {
            return false;
        }
        let from = self.starts()[index];
        let length = self.segments.remove(index).length();
        self.cut_overlays(from, from + length);
        true
    }

    /// Moves a segment edge by `delta` source ms within its neighbours; returns the applied delta.
    pub fn trim_segment(&mut self, index: usize, head: bool, delta: i64) -> i64 {
        let Some(segment) = self.segments.get(index) else {
            return 0;
        };
        let start = self.starts()[index];
        let applied = if head {
            let floor = index
                .checked_sub(1)
                .map_or(0, |i| self.segments[i].source_out);
            (segment.source_in + delta).clamp(floor, segment.source_out - MIN_CLIP)
                - segment.source_in
        } else {
            let ceiling = self
                .segments
                .get(index + 1)
                .map_or(self.duration, |s| s.source_in);
            (segment.source_out + delta).clamp(segment.source_in + MIN_CLIP, ceiling)
                - segment.source_out
        };
        if applied == 0 {
            return 0;
        }
        let end = start + segment.length();
        if head {
            self.segments[index].source_in += applied;
            if applied > 0 {
                self.cut_overlays(start, start + applied);
            } else {
                self.ripple(start, -applied);
            }
        } else {
            self.segments[index].source_out += applied;
            if applied > 0 {
                self.ripple(end, applied);
            } else {
                self.cut_overlays(end + applied, end);
            }
        }
        applied
    }

    /// Shifts overlay clips that start at or after `from`.
    pub fn ripple(&mut self, from: i64, delta: i64) {
        for clip in self.layers.iter_mut().flat_map(|l| l.clips.iter_mut()) {
            if clip.start >= from {
                clip.start += delta;
                clip.end += delta;
            }
        }
    }

    /// Deletes timeline range `[from, to)` from overlays, shortening or removing what it covers.
    pub fn cut_overlays(&mut self, from: i64, to: i64) {
        let length = to - from;
        if length <= 0 {
            return;
        }
        for layer in &mut self.layers {
            layer.clips.retain_mut(|clip| {
                if clip.start >= to {
                    clip.start -= length;
                    clip.end -= length;
                } else if clip.end > from {
                    let kept_before = (from - clip.start).max(0);
                    let kept_after = (clip.end - to).max(0);
                    clip.start = clip.start.min(from);
                    clip.end = clip.start + kept_before + kept_after;
                }
                clip.duration() >= MIN_CLIP
            });
        }
    }

    pub fn locate_clip(&self, id: u64) -> Option<(usize, usize)> {
        self.layers.iter().enumerate().find_map(|(l, layer)| {
            layer
                .clips
                .iter()
                .position(|c| c.id == id)
                .map(|c| (l, c))
        })
    }

    pub fn clip(&self, id: u64) -> Option<&Clip> {
        self.locate_clip(id)
            .map(|(l, c)| &self.layers[l].clips[c])
    }

    pub fn clip_mut(&mut self, id: u64) -> Option<&mut Clip> {
        self.locate_clip(id)
            .map(|(l, c)| &mut self.layers[l].clips[c])
    }

    /// Free interval around `t` in a layer, ignoring clip `exclude`.
    pub fn gap(&self, layer: usize, exclude: u64, t: i64) -> (i64, i64) {
        let mut low = 0;
        let mut high = i64::MAX;
        for clip in self.layers[layer].clips.iter().filter(|c| c.id != exclude) {
            if clip.end <= t {
                low = low.max(clip.end);
            } else if clip.start >= t {
                high = high.min(clip.start);
            } else {
                return (t, t);
            }
        }
        (low, high)
    }

    fn fits(&self, layer: usize, exclude: u64, start: i64, end: i64) -> bool {
        self.layers[layer]
            .clips
            .iter()
            .all(|c| c.id == exclude || c.end <= start || c.start >= end)
    }

    fn insert_sorted(&mut self, layer: usize, clip: Clip) {
        let clips = &mut self.layers[layer].clips;
        let at = clips.partition_point(|c| c.start < clip.start);
        clips.insert(at, clip);
    }

    pub fn add_layer(&mut self, kind: LayerKind) -> usize {
        let id = self.id();
        let count = self.layers.iter().filter(|l| l.kind == kind).count() + 1;
        let used: Vec<usize> = self.layers.iter().map(|l| l.color).collect();
        let color = (0..PALETTE.len())
            .find(|c| !used.contains(c))
            .unwrap_or(self.layers.len() % PALETTE.len());
        self.layers.push(Layer {
            id,
            kind,
            name: format!(
                "{} {count}",
                match kind {
                    LayerKind::Text => "Text",
                    LayerKind::Picture => "Overlay",
                }
            ),
            visible: true,
            locked: false,
            color,
            clips: vec![],
        });
        self.layers.len() - 1
    }

    /// Places a clip on the lowest unlocked layer of its kind with room, adding a layer if none has.
    pub fn add_clip(&mut self, mut clip: Clip) -> u64 {
        clip.id = self.id();
        clip.start = clip.start.max(0);
        clip.end = clip.end.max(clip.start + MIN_CLIP);
        let kind = clip.kind();
        let layer = (0..self.layers.len())
            .find(|&l| {
                self.layers[l].kind == kind
                    && !self.layers[l].locked
                    && self.fits(l, 0, clip.start, clip.end)
            })
            .unwrap_or_else(|| self.add_layer(kind));
        let id = clip.id;
        self.insert_sorted(layer, clip);
        id
    }

    /// Moves a clip to `start` on `layer`, clamped into the free gap nearest the request.
    pub fn move_clip(&mut self, id: u64, start: i64, layer: usize) -> bool {
        let Some((from, index)) = self.locate_clip(id) else {
            return false;
        };
        let target = if self.layers.get(layer).is_some_and(|l| {
            l.kind == self.layers[from].kind && !l.locked
        }) {
            layer
        } else {
            from
        };
        let duration = self.layers[from].clips[index].duration();
        let start = start.max(0);
        let (low, high) = self.gap(target, id, start + duration / 2);
        if high - low < duration {
            return false;
        }
        let start = start.clamp(low, high.saturating_sub(duration));
        let mut clip = self.layers[from].clips.remove(index);
        clip.start = start;
        clip.end = start + duration;
        self.insert_sorted(target, clip);
        true
    }

    /// Sets a clip edge to `t` within its gap; returns the edge actually set.
    pub fn set_clip_edge(&mut self, id: u64, head: bool, t: i64) -> i64 {
        let Some((layer, index)) = self.locate_clip(id) else {
            return t;
        };
        let clip = &self.layers[layer].clips[index];
        let (low, high) = self.gap(layer, id, (clip.start + clip.end) / 2);
        let (start, end) = (clip.start, clip.end);
        let clip = &mut self.layers[layer].clips[index];
        if head {
            clip.start = t.clamp(low.max(0), end - MIN_CLIP);
            clip.start
        } else {
            clip.end = t.clamp(start + MIN_CLIP, high);
            clip.end
        }
    }

    pub fn split_clip(&mut self, id: u64, t: i64) -> Option<u64> {
        let (layer, index) = self.locate_clip(id)?;
        let clip = &self.layers[layer].clips[index];
        if t - clip.start < MIN_CLIP || clip.end - t < MIN_CLIP {
            return None;
        }
        let mut right = clip.clone();
        right.id = self.id();
        right.start = t;
        let clip = &mut self.layers[layer].clips[index];
        clip.end = t;
        let new_id = right.id;
        self.layers[layer].clips.insert(index + 1, right);
        Some(new_id)
    }

    /// Copies a clip right after itself, or onto another layer when that space is taken.
    pub fn duplicate_clip(&mut self, id: u64) -> Option<u64> {
        let (layer, index) = self.locate_clip(id)?;
        let mut copy = self.layers[layer].clips[index].clone();
        let duration = copy.duration();
        copy.start = copy.end;
        copy.end = copy.start + duration;
        if !self.fits(layer, 0, copy.start, copy.end) {
            return Some(self.add_clip(copy));
        }
        copy.id = self.id();
        let new_id = copy.id;
        self.insert_sorted(layer, copy);
        Some(new_id)
    }

    pub fn remove_clip(&mut self, id: u64) -> Option<Clip> {
        let (layer, index) = self.locate_clip(id)?;
        Some(self.layers[layer].clips.remove(index))
    }

    pub fn move_layer(&mut self, index: usize, up: bool) -> Option<usize> {
        let target = if up {
            index + 1
        } else {
            index.checked_sub(1)?
        };
        if target >= self.layers.len() {
            return None;
        }
        self.layers.swap(index, target);
        Some(target)
    }

    /// Visible overlay clips at `t`, bottom to top.
    pub fn active(&self, t: i64) -> Vec<(usize, usize)> {
        self.layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.visible)
            .flat_map(|(l, layer)| {
                layer
                    .clips
                    .iter()
                    .enumerate()
                    .filter(move |(_, c)| c.start <= t && t < c.end)
                    .map(move |(c, _)| (l, c))
            })
            .collect()
    }

    /// Edges a dragged edge may snap to.
    pub fn snap_points(&self, exclude: u64, playhead: i64) -> Vec<i64> {
        let mut points = vec![0, self.length(), playhead];
        points.extend(self.starts());
        for clip in self.layers.iter().flat_map(|l| &l.clips) {
            if clip.id != exclude {
                points.extend([clip.start, clip.end]);
            }
        }
        points
    }

    /// Cropped output size, optionally limited to `max_height`, with even dimensions.
    pub fn output_size(&self, max_height: u32) -> (u32, u32) {
        let even = |v: f64| ((v / 2.0).trunc() as u32 * 2).max(2);
        let width = even(self.width as f64 * self.crop.width() as f64);
        let height = even(self.height as f64 * self.crop.height() as f64);
        if max_height == 0 || height <= max_height {
            return (width, height);
        }
        let scaled = ((width as f64 * max_height as f64 / height as f64 / 2.0).round() as u32
            * 2)
        .max(2);
        (scaled, max_height & !1)
    }

    /// Opacity of the fade color over the video at timeline time `t`.
    pub fn fade_cover(&self, t: i64) -> (f32, bool) {
        let Some((index, offset)) = self.locate(t) else {
            return (0.0, false);
        };
        let segment = &self.segments[index];
        let mut cover: f32 = 0.0;
        if segment.fade_in > 0 && offset < segment.fade_in {
            cover = cover.max(1.0 - offset as f32 / segment.fade_in as f32);
        }
        let left = segment.length() - offset;
        if segment.fade_out > 0 && left < segment.fade_out {
            cover = cover.max(1.0 - left as f32 / segment.fade_out as f32);
        }
        (cover.clamp(0.0, 1.0), segment.white)
    }
}

/// Snapshot history; changes are committed in batches when an edit settles.
pub struct History {
    undo: Vec<Project>,
    redo: Vec<Project>,
    committed: Project,
}

impl History {
    const LIMIT: usize = 120;

    pub fn new(project: &Project) -> Self {
        Self {
            undo: vec![],
            redo: vec![],
            committed: project.clone(),
        }
    }
    /// Records `project` as a new step if it changed; returns whether it did.
    pub fn commit(&mut self, project: &Project) -> bool {
        if *project == self.committed {
            return false;
        }
        self.undo
            .push(std::mem::replace(&mut self.committed, project.clone()));
        if self.undo.len() > Self::LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        true
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo(&mut self, project: &mut Project) -> bool {
        self.commit(project);
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.committed, previous.clone()));
        *project = previous;
        true
    }
    pub fn redo(&mut self, project: &mut Project) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo
            .push(std::mem::replace(&mut self.committed, next.clone()));
        *project = next;
        true
    }
}

/// Stable file name for a media item's project.
pub fn project_file(uri: &str, modified: u64, size: u64) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in format!("{uri}:{modified}:{size}").bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project::new("content://v", "v.mp4", 10_000, 1920, 1080)
    }

    fn text(start: i64, end: i64) -> Clip {
        Clip {
            start,
            end,
            ..Clip::default()
        }
    }

    #[test]
    fn split_and_remove_keep_timeline_and_source_in_step() {
        let mut p = project();
        assert!(p.split(4000));
        assert!(p.split(7000));
        assert!(!p.split(7050), "Too close to an existing cut");
        assert_eq!(p.ranges(), [[0, 4000], [4000, 7000], [7000, 10_000]]);
        let id = p.add_clip(text(8000, 9000));
        let inside = p.add_clip(text(4500, 5000));
        let straddle = p.add_clip(text(3000, 6000));
        assert!(p.remove_segment(1));
        assert_eq!(p.ranges(), [[0, 4000], [7000, 10_000]]);
        assert_eq!(p.length(), 7000);
        assert_eq!((p.clip(id).unwrap().start, p.clip(id).unwrap().end), (5000, 6000));
        assert!(p.clip(inside).is_none());
        assert_eq!(
            (p.clip(straddle).unwrap().start, p.clip(straddle).unwrap().end),
            (3000, 4000)
        );
        assert_eq!(p.source_time(3999), 3999);
        assert_eq!(p.source_time(4000), 7000);
        assert_eq!(p.timeline_time(7500), 4500);
        assert_eq!(p.timeline_time(5000), 4000, "A removed range maps to the next segment");
        assert_eq!(p.timeline_time(20_000), 7000);
        assert!(p.remove_segment(0));
        assert!(!p.remove_segment(0), "The last segment stays");
    }

    #[test]
    fn segment_trims_stay_inside_neighbours_and_ripple_overlays() {
        let mut p = project();
        p.split(5000);
        let after = p.add_clip(text(6000, 7000));
        assert_eq!(p.trim_segment(0, false, 2000), 0, "Next segment owns that source");
        assert_eq!(p.trim_segment(0, false, -1000), -1000);
        assert_eq!(p.ranges(), [[0, 4000], [5000, 10_000]]);
        assert_eq!(p.clip(after).unwrap().start, 5000);
        assert_eq!(p.trim_segment(1, true, -600), -600);
        assert_eq!(p.ranges(), [[0, 4000], [4400, 10_000]]);
        assert_eq!(p.clip(after).unwrap().start, 5600);
        assert_eq!(p.trim_segment(1, true, 99_999), 5500);
        assert_eq!(p.segments[1].length(), MIN_CLIP);
        assert_eq!(p.trim_segment(0, true, -50), 0);
    }

    #[test]
    fn clips_never_overlap_inside_a_layer() {
        let mut p = project();
        let a = p.add_clip(text(1000, 3000));
        let b = p.add_clip(text(2000, 4000));
        assert_eq!(p.layers.len(), 2, "Overlapping text goes to a new layer");
        let c = p.add_clip(text(3000, 5000));
        assert_eq!(p.locate_clip(c).unwrap().0, 0);
        assert!(p.move_clip(b, 0, 0) == false, "No 2 s gap on layer 0");
        assert!(p.move_clip(b, 6000, 0));
        assert_eq!(p.locate_clip(b), Some((0, 2)));
        assert_eq!(p.set_clip_edge(a, false, 4500), 3000, "Clamped to the next clip");
        assert_eq!(p.set_clip_edge(a, true, -500), 0);
        assert_eq!(p.set_clip_edge(a, true, 2990), 2900);
        assert!(p.move_clip(c, 2950, 0));
        assert_eq!(p.clip(c).unwrap().start, 3000, "Moves clamp to the gap");
        let pic = p.add_clip(Clip {
            body: Body::Picture(Picture::default()),
            ..text(0, 1000)
        });
        p.move_clip(pic, 0, 0);
        assert_eq!(p.layers[p.locate_clip(pic).unwrap().0].kind, LayerKind::Picture);
    }

    #[test]
    fn split_and_duplicate_clips() {
        let mut p = project();
        let a = p.add_clip(text(1000, 3000));
        let right = p.split_clip(a, 2000).unwrap();
        assert_eq!(p.clip(a).unwrap().end, 2000);
        assert_eq!(p.clip(right).unwrap().start, 2000);
        assert!(p.split_clip(a, 1050).is_none());
        let copy = p.duplicate_clip(right).unwrap();
        assert_eq!(
            (p.clip(copy).unwrap().start, p.clip(copy).unwrap().end),
            (3000, 4000)
        );
        let blocked = p.duplicate_clip(a).unwrap();
        assert_ne!(p.locate_clip(blocked).unwrap().0, 0);
        assert_eq!(p.clip(blocked).unwrap().start, 2000);
    }

    #[test]
    fn hidden_layers_are_not_active() {
        let mut p = project();
        let a = p.add_clip(text(0, 2000));
        p.add_clip(text(1000, 2000));
        assert_eq!(p.active(1500).len(), 2);
        p.layers[1].visible = false;
        assert_eq!(p.active(1500), [p.locate_clip(a).unwrap()]);
        assert!(p.active(2000).is_empty(), "End is exclusive");
    }

    #[test]
    fn envelope_fades_and_slides_inside_short_clips() {
        let clip = Clip {
            start: 1000,
            end: 1400,
            enter: Motion::SlideUp,
            enter_ms: 300,
            exit: Motion::Fade,
            exit_ms: 300,
            ..Clip::default()
        };
        assert_eq!(clip.motion_ms(), (200, 200));
        let (alpha, _, dy) = clip.envelope(1000);
        assert_eq!(alpha, 0.0);
        assert!((dy - SLIDE).abs() < 1e-6, "Starts below, travelling up");
        let (alpha, _, dy) = clip.envelope(1200);
        assert_eq!((alpha, dy), (1.0, 0.0));
        assert!((clip.envelope(1300).0 - 0.5).abs() < 1e-6);
        assert_eq!(clip.envelope(1400).0, 0.0);
    }

    #[test]
    fn output_size_matches_ffmpeg_even_crop() {
        let mut p = project();
        assert_eq!(p.output_size(0), (1920, 1080));
        p.crop = Crop::aspect(1920, 1080, 9.0 / 16.0);
        assert_eq!(p.output_size(0), (606, 1080));
        assert_eq!(p.output_size(720), (404, 720));
        p.crop = Crop::default();
        assert_eq!(p.output_size(720), (1280, 720));
    }

    #[test]
    fn fade_cover_follows_segment_edges() {
        let mut p = project();
        p.split(5000);
        p.segments[0].fade_out = 1000;
        p.segments[1].fade_in = 500;
        p.segments[1].white = true;
        assert_eq!(p.fade_cover(3000), (0.0, false));
        assert!((p.fade_cover(4500).0 - 0.5).abs() < 0.01);
        assert_eq!(p.fade_cover(5000), (1.0, true));
        assert!((p.fade_cover(5250).0 - 0.5).abs() < 0.01);
    }

    #[test]
    fn history_batches_and_restores() {
        let mut p = project();
        let mut h = History::new(&p);
        assert!(!h.commit(&p));
        p.split(3000);
        p.split(6000);
        assert!(h.commit(&p));
        p.add_clip(text(0, 1000));
        assert!(h.undo(&mut p), "Uncommitted change is undone first");
        assert_eq!(p.segments.len(), 3);
        assert!(p.layers.is_empty());
        assert!(h.undo(&mut p));
        assert_eq!(p.segments.len(), 1);
        assert!(!h.undo(&mut p));
        assert!(h.redo(&mut p) && h.redo(&mut p));
        assert_eq!(p.layers.len(), 1);
        assert!(!h.can_redo());
    }

    #[test]
    fn projects_load_with_missing_fields() {
        let p: Project = serde_json::from_str(r#"{"source":"x","duration":5000,
            "segments":[{"source_out":5000}],"layers":[{"clips":[{"body":{"Text":{"text":"Hi"}}}]}]}"#)
        .unwrap();
        assert_eq!(p.layers[0].clips[0].text().unwrap().text, "Hi");
        assert_eq!(p.layers[0].clips[0].text().unwrap().size, TextStyle::default().size);
        assert!(p.audio.keep);
        let round: Project = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(round, p);
        assert_ne!(project_file("a", 1, 2), project_file("a", 1, 3));
    }

    #[test]
    fn presets_keep_words_and_size() {
        let mut style = TextStyle {
            text: "Keep".into(),
            size: 0.1,
            ..TextStyle::default()
        };
        style.apply_preset(&presets()[2].1);
        assert_eq!((style.text.as_str(), style.size), ("Keep", 0.1));
        assert_eq!(style.background[3], 52);
    }
}
