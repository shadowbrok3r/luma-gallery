use super::*;
use crate::caption;
use crate::studio::Align as TextAlign;
use egui::Align;
use crate::compose;
use crate::studio::*;
use image::RgbaImage;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Arc, mpsc};

const HEADER: f32 = 50.0;
const RULER: f32 = 20.0;
const LANE: f32 = 34.0;
const FILM: f32 = 56.0;
const TOOLS: f32 = 62.0;
const TRANSPORT: f32 = 40.0;
const SWATCHES: [[u8; 4]; 10] = [
    [255, 255, 255, 255],
    [0, 0, 0, 255],
    [255, 221, 0, 255],
    [255, 61, 139, 255],
    [43, 226, 214, 255],
    [255, 72, 72, 255],
    [255, 150, 40, 255],
    [110, 220, 90, 255],
    [70, 140, 255, 255],
    [170, 110, 255, 255],
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Panel {
    None,
    Text,
    Style,
    Motion,
    Fade,
    Audio,
    Canvas,
    Layers,
    Export,
    Photos,
    Debug,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Pick {
    Clip(u64),
    Segment(usize),
}

#[derive(Clone, Copy, Debug)]
enum Grab {
    Move { id: u64, x: f32, y: f32 },
    Turn { id: u64, angle: f32, distance: f32, rotation: f32, size: f32 },
    Scrub { playhead: i64 },
    Rows { offset: f32 },
    Body { id: u64, start: i64 },
    Edge { id: u64, head: bool, at: i64 },
    Cut { index: usize, head: bool, delta: i64 },
}

#[derive(Clone, Copy, PartialEq)]
enum Lane {
    Layer(usize),
    Film,
}

struct Preview {
    key: u64,
    texture: egui::TextureHandle,
    size: Vec2,
    content: [f32; 4],
}

/// Selection box of an overlay on the stage, in points.
#[derive(Clone, Copy)]
struct Hit {
    id: u64,
    center: Pos2,
    half: Vec2,
    angle: f32,
}

impl Hit {
    fn local(&self, p: Pos2) -> Vec2 {
        let d = p - self.center;
        let (sin, cos) = (-self.angle).sin_cos();
        vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
    }
    fn contains(&self, p: Pos2, margin: f32) -> bool {
        let l = self.local(p);
        l.x.abs() <= self.half.x + margin && l.y.abs() <= self.half.y + margin
    }
    fn corner(&self, sx: f32, sy: f32) -> Pos2 {
        let (sin, cos) = self.angle.sin_cos();
        let v = vec2(self.half.x * sx, self.half.y * sy);
        self.center + vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
    }
    /// Delete, edit, copy and resize handles.
    fn handles(&self) -> [(Pos2, &'static str); 4] {
        [
            (self.corner(-1.0, -1.0), "close"),
            (self.corner(1.0, -1.0), "edit"),
            (self.corner(-1.0, 1.0), "copy"),
            (self.corner(1.0, 1.0), "rotate"),
        ]
    }
}

pub(super) struct Studio {
    pub item: MediaItem,
    pub project: Project,
    history: History,
    shadow: Project,
    changed: f64,
    pub pick: Option<Pick>,
    pub panel: Panel,
    pub playhead: i64,
    scale: f32,
    rows: f32,
    grab: Option<(Grab, Pos2)>,
    pinching: bool,
    armed: Option<u64>,
    crop_drag: Option<editor::CropDrag>,
    snapped: Option<i64>,
    guides: [bool; 2],
    hits: Vec<Hit>,
    frame: Rect,
    track: Rect,
    lanes: Vec<(Lane, Rect)>,
    previews: HashMap<u64, Preview>,
    tiles: HashMap<String, egui::TextureHandle>,
    pictures: HashMap<String, Arc<RgbaImage>>,
    waiting: HashMap<String, u64>,
    export: Option<mpsc::Receiver<Result<Value, String>>>,
    pub height: u32,
    pub crf: u8,
    fps: String,
    dir: Option<PathBuf>,
    sent: Option<Vec<[i64; 2]>>,
    reported: Option<(i64, Instant)>,
    seeking: Option<(i64, Instant)>,
    last_seek: Instant,
    focus_text: bool,
    haptic: Option<egui_mobile::Haptic>,
    hit_boxes: bool,
    render_ms: f32,
    renders: u32,
}

impl Studio {
    pub fn new(item: MediaItem, fps: f64, fps_text: String, has_audio: bool, dir: Option<PathBuf>) -> Self {
        let mut project = Project::new(&item.uri, &item.name, item.duration, item.width, item.height);
        if let Some(saved) = dir
            .as_ref()
            .and_then(|d| std::fs::read(d.join(project_file(&item.uri, 0, item.size))).ok())
            .and_then(|bytes| serde_json::from_slice::<Project>(&bytes).ok())
            .filter(|p| {
                p.source == item.uri
                    && (p.duration - project.duration).abs() < 250
                    && !p.segments.is_empty()
            })
        {
            project = Project {
                width: project.width,
                height: project.height,
                duration: project.duration,
                ..saved
            };
        }
        project.fps = fps;
        project.has_audio = has_audio;
        if !has_audio {
            project.audio.keep = false;
        }
        Self {
            item,
            history: History::new(&project),
            shadow: project.clone(),
            changed: 0.0,
            project,
            pick: None,
            panel: Panel::None,
            playhead: 0,
            scale: 0.0,
            rows: f32::MAX,
            grab: None,
            pinching: false,
            armed: None,
            crop_drag: None,
            snapped: None,
            guides: [false; 2],
            hits: vec![],
            frame: Rect::NOTHING,
            track: Rect::NOTHING,
            lanes: vec![],
            previews: HashMap::new(),
            tiles: HashMap::new(),
            pictures: HashMap::new(),
            waiting: HashMap::new(),
            export: None,
            height: 0,
            crf: 18,
            fps: fps_text,
            dir,
            sent: None,
            reported: None,
            seeking: None,
            last_seek: Instant::now(),
            focus_text: false,
            haptic: None,
            hit_boxes: false,
            render_ms: 0.0,
            renders: 0,
        }
    }

    fn file(&self) -> Option<PathBuf> {
        self.dir
            .as_ref()
            .map(|d| d.join(project_file(&self.item.uri, 0, self.item.size)))
    }

    pub fn save(&self) {
        if let (Some(dir), Some(file)) = (&self.dir, self.file())
            && std::fs::create_dir_all(dir).is_ok()
            && let Ok(bytes) = serde_json::to_vec_pretty(&self.project)
        {
            let _ = std::fs::write(file, bytes);
        }
    }

    /// Closes the innermost open thing; returns true when the editor itself should close.
    pub fn back(&mut self) -> bool {
        if self.grab.take().is_some() {
            return false;
        }
        if self.panel != Panel::None {
            self.panel = Panel::None;
            return false;
        }
        if self.pick.take().is_some() {
            return false;
        }
        true
    }

    pub fn event(&mut self, e: &Value) {
        let Some(session) = e["session"].as_str() else {
            return;
        };
        let Some(id) = self.waiting.remove(session) else {
            return;
        };
        let source = e["path"].as_str().unwrap_or_default();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let stored = self.dir.as_ref().map(|d| d.join("media").join(format!("{id}-{stamp}.png")));
        let path = match stored {
            Some(target)
                if e["type"] == "edit_ready"
                    && std::fs::create_dir_all(target.parent().expect("media dir")).is_ok()
                    && std::fs::copy(source, &target).is_ok() =>
            {
                target.to_string_lossy().into_owned()
            }
            _ if e["type"] == "edit_ready" => source.to_owned(),
            _ => {
                self.project.remove_clip(id);
                return;
            }
        };
        let (w, h) = (e["width"].as_f64().unwrap_or(1.0), e["height"].as_f64().unwrap_or(1.0));
        if let Some(Body::Picture(picture)) = self.project.clip_mut(id).map(|c| &mut c.body) {
            picture.path = path;
            picture.aspect = (h / w.max(1.0)) as f32;
        }
    }

    fn clip(&self) -> Option<&Clip> {
        match self.pick {
            Some(Pick::Clip(id)) => self.project.clip(id),
            _ => None,
        }
    }

    fn clip_mut(&mut self) -> Option<&mut Clip> {
        match self.pick {
            Some(Pick::Clip(id)) => self.project.clip_mut(id),
            _ => None,
        }
    }

    fn picture(&mut self, path: &str) -> Option<Arc<RgbaImage>> {
        if path.is_empty() {
            return None;
        }
        if let Some(image) = self.pictures.get(path) {
            return Some(image.clone());
        }
        let image = Arc::new(image::open(path).ok()?.to_rgba8());
        self.pictures.insert(path.to_owned(), image.clone());
        Some(image)
    }

    /// Texture of a clip rendered for a frame of `frame` pixels, its pixel size and content box.
    fn preview(&mut self, ctx: &egui::Context, clip: &Clip, frame: Vec2) -> Option<(egui::TextureId, Vec2, [f32; 4])> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        serde_json::to_string(&clip.body).unwrap_or_default().hash(&mut hasher);
        (frame.x.round() as i32, frame.y.round() as i32).hash(&mut hasher);
        let key = hasher.finish();
        if self.previews.get(&clip.id).is_none_or(|p| p.key != key) {
            let started = Instant::now();
            let rendered = match &clip.body {
                Body::Text(style) => caption::render_text(style, frame.x, frame.y),
                Body::Picture(picture) => {
                    let source = self.picture(&picture.path)?;
                    caption::render_picture(picture, &source, frame.x)
                }
            };
            self.render_ms = started.elapsed().as_secs_f32() * 1000.0;
            self.renders += 1;
            let (w, h) = rendered.image.dimensions();
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                rendered.image.as_raw(),
            );
            let size = vec2(w as f32, h as f32);
            match self.previews.get_mut(&clip.id) {
                Some(preview) => {
                    preview.texture.set(image, egui::TextureOptions::LINEAR);
                    (preview.key, preview.size, preview.content) = (key, size, rendered.content);
                }
                None => {
                    let texture = ctx.load_texture(
                        format!("studio-{}", clip.id),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.previews.insert(
                        clip.id,
                        Preview {
                            key,
                            texture,
                            size,
                            content: rendered.content,
                        },
                    );
                }
            }
        }
        let preview = self.previews.get(&clip.id)?;
        Some((preview.texture.id(), preview.size, preview.content))
    }

    /// Small rendered sample used by the font and preset pickers.
    fn tile(&mut self, ctx: &egui::Context, key: String, style: &TextStyle) -> egui::TextureHandle {
        self.tiles
            .entry(key.clone())
            .or_insert_with(|| {
                let rendered = caption::render_text(style, 640.0, 360.0);
                let (w, h) = rendered.image.dimensions();
                ctx.load_texture(
                    key,
                    egui::ColorImage::from_rgba_unmultiplied(
                        [w as usize, h as usize],
                        rendered.image.as_raw(),
                    ),
                    egui::TextureOptions::LINEAR,
                )
            })
            .clone()
    }

    /// Commits a batch once edits pause for 0.45 s of input time and no pointer is down.
    fn settle(&mut self, pointer_down: bool, now: f64) {
        if self.project != self.shadow {
            self.shadow = self.project.clone();
            self.changed = now;
        }
        if !pointer_down
            && self.grab.is_none()
            && now - self.changed > 0.45
            && self.history.commit(&self.project)
        {
            self.save();
        }
    }

    fn undo(&mut self) {
        if self.history.undo(&mut self.project) {
            self.shadow = self.project.clone();
            self.pick = self.pick.filter(|p| match p {
                Pick::Clip(id) => self.project.clip(*id).is_some(),
                Pick::Segment(i) => *i < self.project.segments.len(),
            });
            self.save();
        }
    }

    fn redo(&mut self) {
        if self.history.redo(&mut self.project) {
            self.shadow = self.project.clone();
            self.save();
        }
    }

    fn seek(&mut self, t: i64, fast: bool) {
        let t = t.clamp(0, self.project.length());
        self.playhead = t;
        self.seeking = Some((t, Instant::now()));
        self.reported = None;
        bridge::send(json!({"op":"seek","time":self.project.source_time(t),"fast":fast}));
    }

    fn add_text(&mut self) {
        let style = TextStyle {
            text: String::new(),
            font: caption::book().default_family().to_owned(),
            ..TextStyle::default()
        };
        let (start, end) = self.slot();
        let id = self.project.add_clip(Clip {
            start,
            end,
            body: Body::Text(style),
            ..Clip::default()
        });
        self.pick = Some(Pick::Clip(id));
        self.panel = Panel::Text;
        self.focus_text = true;
    }

    /// Three seconds from the playhead, kept inside the video when it is long enough.
    fn slot(&self) -> (i64, i64) {
        let length = self.project.length();
        let start = self.playhead.min((length - 1000).max(0));
        (start, (start + 3000).min(length.max(start + MIN_CLIP)))
    }

    fn split(&mut self) {
        match self.pick {
            Some(Pick::Clip(id)) => {
                if self.project.split_clip(id, self.playhead).is_none() {
                    self.haptic = Some(egui_mobile::Haptic::Warning);
                }
            }
            _ => {
                if self.project.split(self.playhead) {
                    self.pick = self.project.locate(self.playhead).map(|(i, _)| Pick::Segment(i));
                } else {
                    self.haptic = Some(egui_mobile::Haptic::Warning);
                }
            }
        }
    }

    fn delete(&mut self) {
        match self.pick.take() {
            Some(Pick::Clip(id)) => {
                self.project.remove_clip(id);
                self.previews.remove(&id);
            }
            Some(Pick::Segment(i)) => {
                if !self.project.remove_segment(i) {
                    self.haptic = Some(egui_mobile::Haptic::Warning);
                }
                self.playhead = self.playhead.min(self.project.length());
            }
            None => {}
        }
        if self.panel != Panel::Layers && self.panel != Panel::Debug {
            self.panel = Panel::None;
        }
    }

    fn snap_tolerance(&self) -> i64 {
        (12.0 * 1000.0 / self.scale.max(1.0)) as i64
    }

    fn snap(&mut self, t: i64, exclude: u64) -> Option<i64> {
        let points = self.project.snap_points(exclude, self.playhead);
        let found = snap(t, &points, self.snap_tolerance());
        if found.is_some() && found != self.snapped {
            self.haptic = Some(egui_mobile::Haptic::Selection);
        }
        self.snapped = found;
        found
    }

    fn sample_captions(&mut self) {
        let length = self.project.length().max(3000);
        for (n, (name, preset)) in presets().into_iter().take(4).enumerate() {
            let start = length * n as i64 / 4;
            self.project.add_clip(Clip {
                start,
                end: start + length / 4,
                y: 0.2 + 0.2 * n as f32,
                body: Body::Text(TextStyle {
                    text: format!("{name} caption"),
                    font: caption::book().default_family().to_owned(),
                    ..preset
                }),
                ..Clip::default()
            });
        }
    }
}

/// Pointer position, or the touch's lift point on the release frame.
fn pointer(ui: &egui::Ui, response: &egui::Response) -> Option<Pos2> {
    let lifted = response.drag_stopped().then(|| {
        ui.input(|i| {
            i.events.iter().rev().find_map(|e| match e {
                egui::Event::Touch {
                    phase: egui::TouchPhase::End,
                    pos,
                    ..
                } => Some(*pos),
                _ => None,
            })
        })
    });
    lifted.flatten().or_else(|| response.interact_pointer_pos())
}

fn haptic_tick(s: &mut Studio, snapped: bool, axis: usize) {
    if snapped && !s.guides[axis] {
        s.haptic = Some(egui_mobile::Haptic::Selection);
    }
    s.guides[axis] = snapped;
}

fn rgb(c: [u8; 3], alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (alpha.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Translucent clip box with a top sheen.
fn glass(painter: &egui::Painter, rect: Rect, color: [u8; 3], strength: f32, selected: bool) {
    painter.rect_filled(rect, 6, rgb(color, 0.30 * strength));
    let sheen = Rect::from_min_max(
        rect.min + vec2(2.0, 1.0),
        pos2(rect.right() - 2.0, rect.top() + rect.height() * 0.55),
    );
    if sheen.width() > 0.0 {
        let mut mesh = egui::Mesh::default();
        let top = Color32::from_white_alpha((52.0 * strength) as u8);
        mesh.colored_vertex(sheen.left_top(), top);
        mesh.colored_vertex(sheen.right_top(), top);
        mesh.colored_vertex(sheen.right_bottom(), Color32::TRANSPARENT);
        mesh.colored_vertex(sheen.left_bottom(), Color32::TRANSPARENT);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(egui::Shape::mesh(mesh));
    }
    painter.rect_stroke(
        rect,
        6,
        if selected {
            Stroke::new(2.0, Color32::WHITE)
        } else {
            Stroke::new(1.0, rgb(color, 0.85 * strength))
        },
        StrokeKind::Inside,
    );
}

fn handles(painter: &egui::Painter, rect: Rect) {
    for x in [rect.left() - 12.0, rect.right()] {
        let tab = Rect::from_min_size(pos2(x, rect.top()), vec2(12.0, rect.height()));
        painter.rect_filled(tab, 4, Color32::WHITE);
        painter.vline(
            tab.center().x,
            tab.center().y - 6.0..=tab.center().y + 6.0,
            Stroke::new(2.0, BG),
        );
    }
}

fn swatches(ui: &mut egui::Ui, color: &mut [u8; 4]) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        for swatch in SWATCHES {
            let (rect, response) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
            let fill = Color32::from_rgb(swatch[0], swatch[1], swatch[2]);
            ui.painter().circle_filled(rect.center(), 11.0, fill);
            let chosen = color[..3] == swatch[..3];
            ui.painter().circle_stroke(
                rect.center(),
                12.5,
                Stroke::new(if chosen { 2.0 } else { 1.0 }, if chosen { ACCENT } else { LINE }),
            );
            if response.clicked() {
                color[..3].copy_from_slice(&swatch[..3]);
                if color[3] == 0 {
                    color[3] = 255;
                }
                changed = true;
            }
        }
        let mut srgba = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
        if egui::color_picker::color_edit_button_srgba(
            ui,
            &mut srgba,
            egui::color_picker::Alpha::OnlyBlend,
        )
        .changed()
        {
            *color = srgba.to_srgba_unmultiplied();
            changed = true;
        }
    });
    changed
}

fn ms_slider(ui: &mut egui::Ui, value: &mut i64, max: i64, label: &str) -> bool {
    let mut seconds = *value as f32 / 1000.0;
    let changed = ui
        .add(
            egui::Slider::new(&mut seconds, 0.0..=(max as f32 / 1000.0).max(0.1))
                .text(label)
                .suffix(" s")
                .fixed_decimals(1),
        )
        .changed();
    if changed {
        *value = (seconds * 1000.0).round() as i64;
    }
    changed
}

fn motion_row(ui: &mut egui::Ui, label: &str, motion: &mut Motion, ms: &mut i64) {
    ui.label(egui::RichText::new(label).strong());
    ui.horizontal_wrapped(|ui| {
        for (value, name) in Motion::ALL {
            if ui
                .add(egui::Button::new(name).selected(*motion == value).min_size(vec2(52.0, 32.0)))
                .clicked()
            {
                *motion = value;
                if value != Motion::None && *ms == 0 {
                    *ms = 400;
                }
            }
        }
    });
    if *motion != Motion::None {
        let mut seconds = *ms as f32 / 1000.0;
        if ui
            .add(egui::Slider::new(&mut seconds, 0.1..=2.0).text("Duration").suffix(" s"))
            .changed()
        {
            *ms = (seconds * 1000.0).round() as i64;
        }
    }
}

fn panel_title(ui: &mut egui::Ui, title: &str) -> bool {
    let mut done = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).size(16.0).strong());
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            done = icons::button(ui, "check", "Done", true).clicked();
        });
    });
    done
}

/// Equal-width icon tools in a horizontal strip; returns the tapped index.
fn tool_strip(ui: &mut egui::Ui, tools: &[(&str, &str, bool)]) -> Option<usize> {
    let mut tapped = None;
    egui::ScrollArea::horizontal()
        .id_salt("studio_tools")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let width = (ui.available_width() / tools.len().max(1) as f32).max(62.0);
                for (n, &(icon, label, enabled)) in tools.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(
                        vec2(width, TOOLS),
                        if enabled { Sense::click() } else { Sense::hover() },
                    );
                    let color = if !enabled {
                        MUTED.gamma_multiply(0.45)
                    } else if response.is_pointer_button_down_on() {
                        ACCENT
                    } else {
                        TEXT
                    };
                    icons::image(icon, color, 22.0).paint_at(
                        ui,
                        Rect::from_center_size(rect.center() - vec2(0.0, 9.0), Vec2::splat(22.0)),
                    );
                    ui.painter().text(
                        rect.center() + vec2(0.0, 15.0),
                        Align2::CENTER_CENTER,
                        label,
                        FontId::proportional(11.0),
                        color,
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label)
                    });
                    if response.clicked() {
                        tapped = Some(n);
                    }
                }
            });
        });
    tapped
}

impl Gallery {
    pub(super) fn open_studio(&mut self) {
        let Some(mut item) = self.selected.clone().filter(MediaItem::is_video) else {
            return;
        };
        item.duration = item.duration.max(self.duration());
        if item.duration <= 0 {
            self.notice("Wait for the video to load before editing");
            return;
        }
        let streams = self.metadata["streams"].as_array();
        let has_audio = streams.is_none_or(|s| s.iter().any(|s| s["codec_type"] == "audio"))
            && !self.waveform.status.to_lowercase().contains("no audio");
        let fps = streams
            .and_then(|s| s.iter().find(|s| s["codec_type"] == "video"))
            .and_then(|v| v["avg_frame_rate"].as_str())
            .filter(|r| !r.starts_with('0'))
            .unwrap_or("30")
            .to_owned();
        let dir = self
            .preferences
            .as_ref()
            .and_then(|p| p.parent())
            .map(|d| d.join("studio"));
        self.trimming = false;
        self.crop_video = false;
        self.looped = false;
        bridge::send(json!({"op":"loop","enabled":false}));
        bridge::send(json!({"op":"play","playing":false}));
        self.studio = Some(Studio::new(item, self.timeline.fps, fps, has_audio, dir));
    }

    pub(super) fn close_studio(&mut self, studio: Studio) {
        studio.save();
        bridge::send(json!({"op":"segments","ranges":[]}));
        bridge::send(json!({"op":"play","playing":false}));
        self.last_rect = None;
    }

    pub(super) fn studio_ui(&mut self, ui: &mut egui::Ui, host: &Host) {
        let Some(mut s) = self.studio.take() else {
            return;
        };
        if self.studio_frame(&mut s, ui, host) {
            self.close_studio(s);
        } else {
            self.studio = Some(s);
        }
    }

    fn studio_sync(&mut self, s: &mut Studio) {
        let ranges = s.project.ranges();
        if s.sent.as_ref() != Some(&ranges) {
            bridge::send(json!({"op":"segments","ranges":ranges}));
            s.sent = Some(ranges);
        }
        let playing = self.playing();
        let playback = &self.native["playback"];
        if playback["uri"] == s.item.uri.as_str()
            && let Some(source) = playback["position"].as_i64()
            && !matches!(s.grab, Some((Grab::Scrub { .. }, _)))
        {
            let t = s.project.timeline_time(source);
            if s.seeking.is_some_and(|(target, at)| {
                (t - target).abs() <= 80 || at.elapsed() > Duration::from_millis(1500)
            }) {
                s.seeking = None;
            }
            if s.seeking.is_none() {
                if playing {
                    if s.reported.is_none_or(|(value, _)| value != t) {
                        s.reported = Some((t, Instant::now()));
                    }
                    let (value, at) = s.reported.expect("set above");
                    let rate = playback["rate"].as_f64().unwrap_or(1.0);
                    let ahead = (at.elapsed().as_millis().min(400) as f64 * rate) as i64;
                    s.playhead = (value + ahead).min(s.project.length());
                } else {
                    s.reported = None;
                    s.playhead = t.min(s.project.length());
                }
            }
        }
        if let Some(rx) = &s.export
            && let Ok(result) = rx.try_recv()
        {
            s.export = None;
            match result {
                Ok(command) => bridge::send(command),
                Err(message) => self.notice(message),
            }
        }
    }

    fn studio_frame(&mut self, s: &mut Studio, ui: &mut egui::Ui, host: &Host) -> bool {
        ui.ctx().options_mut(|options| {
            options.input_options.max_click_duration =
                egui::InputOptions::default().max_click_duration;
        });
        self.studio_sync(s);
        let mut close = false;
        ui.horizontal(|ui| {
            if icons::button(ui, "back", "Close editor", false).clicked() {
                close = true;
            }
            ui.label(egui::RichText::new("Edit video").size(16.0).strong());
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let exporting = self.native["exporting"] == true || s.export.is_some();
                if ui
                    .add_enabled(
                        !exporting,
                        egui::Button::image_and_text(icons::image("save", ACCENT, 18.0), "Export")
                            .min_size(vec2(96.0, 36.0)),
                    )
                    .clicked()
                {
                    s.panel = Panel::Export;
                    s.pick = None;
                }
                if ui
                    .add_enabled_ui(s.history.can_redo(), |ui| icons::button(ui, "redo", "Redo", false))
                    .inner
                    .clicked()
                {
                    s.redo();
                }
                if ui
                    .add_enabled_ui(s.history.can_undo() || s.project != s.shadow, |ui| {
                        icons::button(ui, "undo", "Undo", false)
                    })
                    .inner
                    .clicked()
                {
                    s.undo();
                }
            });
        });
        let available = ui.available_size();
        let spacing = ui.spacing().item_spacing.y;
        if available.x > available.y * 1.3 && available.x >= 600.0 {
            ui.columns(2, |columns| {
                let left = columns[0].available_size();
                self.studio_stage(s, &mut columns[0], vec2(left.x, left.y - TRANSPORT - spacing));
                self.studio_transport(s, &mut columns[0]);
                let right = columns[1].available_size();
                self.studio_lower(s, &mut columns[1], right, host);
            });
        } else {
            let lanes = RULER + 28.0 + s.project.layers.len() as f32 * (LANE + 2.0) + FILM;
            let lower = match s.panel {
                Panel::None => lanes.clamp(150.0, (available.y * 0.42).max(150.0)) + TOOLS,
                Panel::Text => (available.y * 0.36).clamp(150.0, 240.0),
                _ => (available.y * 0.48).clamp(190.0, 420.0),
            };
            let stage = (available.y - lower - TRANSPORT - spacing * 3.0).max(90.0);
            self.studio_stage(s, ui, vec2(available.x, stage));
            self.studio_transport(s, ui);
            let rest = ui.available_size();
            self.studio_lower(s, ui, rest, host);
        }
        let (pointer_down, now) = ui.input(|i| (i.pointer.any_down(), i.time));
        s.settle(pointer_down, now);
        if let Some(kind) = s.haptic.take() {
            host.haptic(kind);
        }
        close
    }

    fn studio_lower(&mut self, s: &mut Studio, ui: &mut egui::Ui, size: Vec2, host: &Host) {
        if s.panel == Panel::None {
            self.studio_timeline(s, ui, vec2(size.x, (size.y - TOOLS - 4.0).max(80.0)));
            self.studio_tools(s, ui);
        } else {
            egui::Frame::new()
                .fill(PANEL)
                .corner_radius(8)
                .inner_margin(8)
                .show(ui, |ui| {
                    ui.set_min_size(vec2(size.x - 16.0, size.y - 16.0));
                    ui.set_max_height(size.y - 16.0);
                    self.studio_panel(s, ui, host);
                });
        }
    }

    fn studio_transport(&mut self, s: &mut Studio, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), TRANSPORT),
            egui::Layout::left_to_right(Align::Center),
            |ui| {
                ui.label(egui::RichText::new(timecode(s.playhead, true)).size(14.0).color(TEXT));
                ui.label(
                    egui::RichText::new(format!("/ {}", timecode(s.project.length(), false)))
                        .size(12.0)
                        .color(MUTED),
                );
                let playing = self.playing();
                let middle = (ui.available_width() / 2.0 - 64.0).max(0.0);
                ui.add_space(middle);
                if icons::button(ui, if playing { "pause" } else { "play" }, if playing { "Pause" } else { "Play" }, true)
                    .clicked()
                {
                    if !playing && s.playhead >= s.project.length() - 60 {
                        s.seek(0, false);
                    }
                    bridge::send(json!({"op":"play","playing":!playing}));
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if icons::button(ui, "zoom_in", "Zoom timeline in", false).clicked() {
                        s.scale = (s.scale * 1.6).min(900.0);
                    }
                    if icons::button(ui, "zoom_out", "Zoom timeline out", false).clicked() {
                        s.scale = (s.scale / 1.6).max(6.0);
                    }
                });
            },
        );
    }

    fn studio_stage(&mut self, s: &mut Studio, ui: &mut egui::Ui, size: Vec2) {
        let (stage, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        ui.painter().rect_filled(stage, 0, Color32::BLACK);
        let canvas = s.panel == Panel::Canvas;
        let (ow, oh) = if canvas {
            (s.project.width, s.project.height)
        } else {
            s.project.output_size(0)
        };
        let fit = ((stage.width() - 8.0) / ow as f32).min((stage.height() - 8.0) / oh as f32);
        let frame = Rect::from_center_size(stage.center(), vec2(ow as f32, oh as f32) * fit.max(0.01));
        let crop = if canvas { crate::edit::Crop::default() } else { s.project.crop };
        let full = Rect::from_min_size(
            frame.min - vec2(crop.left * frame.width() / crop.width(), crop.top * frame.height() / crop.height()),
            vec2(frame.width() / crop.width(), frame.height() / crop.height()),
        );
        let mut clipped = ui.new_child(egui::UiBuilder::new().max_rect(frame));
        clipped.set_clip_rect(frame.intersect(stage));
        ui.painter().rect_filled(frame, 0, PANEL);
        #[cfg(target_os = "android")]
        {
            let playback = &self.native["playback"];
            if playback["uri"] == s.item.uri.as_str() {
                let width = playback["width"].as_u64().unwrap_or(0) as u32;
                let height = playback["height"].as_u64().unwrap_or(0) as u32;
                if width > 0 && height > 0 && self.gpu_size != (width, height) {
                    self.configure_video_surface(width, height);
                }
            }
            if let Some(surface) = &self.gpu {
                surface.paint(&clipped, full);
            }
        }
        #[cfg(not(target_os = "android"))]
        let _ = full;
        let ppp = ui.ctx().pixels_per_point();
        let visible = self.dialog.is_none() && self.pending_shared.is_none();
        let rect = (
            (stage.left() * ppp).round() as i32,
            (stage.top() * ppp).round() as i32,
            (stage.width() * ppp).round() as i32,
            (stage.height() * ppp).round() as i32,
            visible,
        );
        if self.last_rect != Some(rect) {
            bridge::send(json!({"op":"rect","x":rect.0,"y":rect.1,"w":rect.2,"h":rect.3,"visible":rect.4}));
            self.last_rect = Some(rect);
        }
        if canvas {
            let mut drag = s.crop_drag.take();
            editor::crop_overlay(ui, frame, &response, &mut s.project.crop, &mut drag, true);
            s.crop_drag = drag;
            return;
        }
        let (cover, white) = s.project.fade_cover(s.playhead);
        if cover > 0.0 {
            let shade = if white { 255 } else { 0 };
            ui.painter().rect_filled(
                frame,
                0,
                Color32::from_rgba_unmultiplied(shade, shade, shade, (cover * 255.0) as u8),
            );
        }
        s.hits.clear();
        s.frame = frame;
        let frame_px = frame.size() * ppp;
        let active = s.project.active(s.playhead);
        let paused = !self.playing() && s.grab.is_none_or(|(g, _)| !matches!(g, Grab::Scrub { .. }));
        for (l, c) in active {
            let clip = s.project.layers[l].clips[c].clone();
            let Some((texture, size_px, content)) = s.preview(ui.ctx(), &clip, frame_px) else {
                continue;
            };
            let (alpha, dx, dy) = if paused && s.pick == Some(Pick::Clip(clip.id)) {
                (1.0, 0.0, 0.0)
            } else {
                clip.envelope(s.playhead)
            };
            let center = frame.min + vec2((clip.x + dx) * frame.width(), (clip.y + dy) * frame.height());
            let size = size_px / ppp;
            let alpha = (alpha * clip.opacity).clamp(0.0, 1.0);
            egui::Image::from_texture((texture, size))
                .rotate(clip.rotation.to_radians(), Vec2::splat(0.5))
                .tint(Color32::from_white_alpha((alpha * 255.0) as u8))
                .paint_at(&clipped, Rect::from_center_size(center, size));
            let offset = vec2(content[0] + content[2] / 2.0, content[1] + content[3] / 2.0) - size_px / 2.0;
            let half = (vec2(content[2], content[3]) / ppp / 2.0).max(vec2(14.0, 14.0));
            s.hits.push(Hit {
                id: clip.id,
                center: center + offset / ppp,
                half,
                angle: clip.rotation.to_radians(),
            });
        }
        let painter = ui.painter().with_clip_rect(stage);
        if s.hit_boxes {
            for hit in &s.hits {
                let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| hit.corner(x, y));
                painter.add(egui::Shape::closed_line(corners.to_vec(), Stroke::new(1.0, TRIM)));
                painter.circle_filled(hit.center, 3.0, TRIM);
            }
            painter.rect_stroke(frame, 0, Stroke::new(1.0, ACCENT), StrokeKind::Outside);
        }
        let selected = match s.pick {
            Some(Pick::Clip(id)) => s.hits.iter().find(|h| h.id == id).copied(),
            _ => None,
        };
        let selected = selected.map(|hit| Hit {
            half: hit.half + Vec2::splat(6.0),
            ..hit
        });
        if let Some(hit) = selected {
            let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| hit.corner(x, y));
            painter.add(egui::Shape::closed_line(corners.to_vec(), Stroke::new(1.5, Color32::WHITE)));
            for (at, icon) in hit.handles() {
                painter.circle_filled(at, 12.0, Color32::WHITE);
                icons::image(icon, BG, 15.0).paint_at(ui, Rect::from_center_size(at, Vec2::splat(15.0)));
            }
        }
        for (axis, on) in s.guides.into_iter().enumerate() {
            if on && s.grab.is_some() {
                let stroke = Stroke::new(1.0, TRIM);
                if axis == 0 {
                    painter.vline(frame.center().x, frame.y_range(), stroke);
                } else {
                    painter.hline(frame.x_range(), frame.center().y, stroke);
                }
            }
        }
        self.studio_stage_gestures(s, ui, frame, &response, selected);
    }

    fn studio_stage_gestures(
        &mut self,
        s: &mut Studio,
        ui: &egui::Ui,
        frame: Rect,
        response: &egui::Response,
        selected: Option<Hit>,
    ) {
        let locked = |s: &Studio, id: u64| {
            s.project
                .locate_clip(id)
                .is_none_or(|(l, _)| s.project.layers[l].locked)
        };
        if let Some(touch) = ui.input(|i| i.multi_touch())
            && response.rect.contains(touch.center_pos)
            && let Some(hit) = selected
            && !locked(s, hit.id)
        {
            s.pinching = true;
            s.grab = None;
            if let Some(clip) = s.project.clip_mut(hit.id) {
                clip.rotation += touch.rotation_delta.to_degrees();
                match &mut clip.body {
                    Body::Text(style) => style.size = (style.size * touch.zoom_delta).clamp(0.015, 0.4),
                    Body::Picture(p) => p.width = (p.width * touch.zoom_delta).clamp(0.04, 2.0),
                }
            }
            return;
        }
        if s.pinching {
            if !ui.input(|i| i.pointer.any_down()) {
                s.pinching = false;
            }
            return;
        }
        if response.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(frame.center());
            if let Some(hit) = selected
                && hit.handles()[3].0.distance(origin) < 24.0
                && !locked(s, hit.id)
                && let Some(clip) = s.project.clip(hit.id)
            {
                let d = origin - hit.center;
                s.grab = Some((
                    Grab::Turn {
                        id: hit.id,
                        angle: d.y.atan2(d.x),
                        distance: d.length().max(1.0),
                        rotation: clip.rotation,
                        size: match &clip.body {
                            Body::Text(style) => style.size,
                            Body::Picture(p) => p.width,
                        },
                    },
                    origin,
                ));
            } else if let Some(hit) = s.hits.iter().rev().find(|h| h.contains(origin, 6.0)).copied() {
                s.pick = Some(Pick::Clip(hit.id));
                if !locked(s, hit.id)
                    && let Some(clip) = s.project.clip(hit.id)
                {
                    s.grab = Some((Grab::Move { id: hit.id, x: clip.x, y: clip.y }, origin));
                }
            }
        }
        if let Some((grab, origin)) = s.grab
            && let Some(pointer) = pointer(ui, response)
        {
            match grab {
                Grab::Move { id, x, y } => {
                    let delta = pointer - origin;
                    let mut nx = x + delta.x / frame.width();
                    let mut ny = y + delta.y / frame.height();
                    let snap_x = ((nx - 0.5) * frame.width()).abs() < 7.0;
                    let snap_y = ((ny - 0.5) * frame.height()).abs() < 7.0;
                    if snap_x {
                        nx = 0.5;
                    }
                    if snap_y {
                        ny = 0.5;
                    }
                    haptic_tick(s, snap_x, 0);
                    haptic_tick(s, snap_y, 1);
                    if let Some(clip) = s.project.clip_mut(id) {
                        (clip.x, clip.y) = (nx.clamp(-0.5, 1.5), ny.clamp(-0.5, 1.5));
                    }
                }
                Grab::Turn { id, angle, distance, rotation, size } => {
                    let center = s.hits.iter().find(|h| h.id == id).map_or(frame.center(), |h| h.center);
                    let d = pointer - center;
                    let mut degrees = rotation + (d.y.atan2(d.x) - angle).to_degrees();
                    let nearest = (degrees / 90.0).round() * 90.0;
                    let square = (degrees - nearest).abs() < 4.0;
                    if square {
                        degrees = nearest;
                    }
                    haptic_tick(s, square, 0);
                    let factor = d.length() / distance;
                    if let Some(clip) = s.project.clip_mut(id) {
                        clip.rotation = degrees.rem_euclid(360.0);
                        if clip.rotation > 180.0 {
                            clip.rotation -= 360.0;
                        }
                        match &mut clip.body {
                            Body::Text(style) => style.size = (size * factor).clamp(0.015, 0.4),
                            Body::Picture(p) => p.width = (size * factor).clamp(0.04, 2.0),
                        }
                    }
                }
                _ => {}
            }
        }
        if response.drag_stopped() {
            s.grab = None;
            s.guides = [false; 2];
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            if let Some(hit) = selected
                && let Some(&(_, action)) = hit.handles().iter().find(|(at, _)| at.distance(p) < 22.0)
            {
                match action {
                    "close" => s.delete(),
                    "edit" => {
                        s.panel = if s.clip().is_some_and(|c| c.text().is_some()) {
                            s.focus_text = true;
                            Panel::Text
                        } else {
                            Panel::Style
                        };
                    }
                    "copy" => {
                        if let Some(id) = s.project.duplicate_clip(hit.id) {
                            s.pick = Some(Pick::Clip(id));
                        }
                    }
                    _ => {}
                }
            } else if let Some(hit) = s.hits.iter().rev().find(|h| h.contains(p, 6.0)).copied() {
                if s.pick == Some(Pick::Clip(hit.id)) && s.clip().is_some_and(|c| c.text().is_some()) {
                    s.panel = Panel::Text;
                    s.focus_text = true;
                }
                s.pick = Some(Pick::Clip(hit.id));
            } else if s.pick.is_some() {
                s.pick = None;
            } else {
                if !self.playing() && s.playhead >= s.project.length() - 60 {
                    s.seek(0, false);
                }
                bridge::send(json!({"op":"play","playing":!self.playing()}));
            }
        }
    }

    fn studio_timeline(&mut self, s: &mut Studio, ui: &mut egui::Ui, size: Vec2) {
        let (area, _) = ui.allocate_exact_size(size, Sense::hover());
        let track = Rect::from_min_max(pos2(area.left() + HEADER, area.top()), area.max);
        let length = s.project.length();
        if s.scale <= 0.0 {
            s.scale = (track.width() * 0.8 / (length as f32 / 1000.0).max(0.5)).clamp(10.0, 160.0);
        }
        let response = ui.interact(track, ui.id().with("studio_track"), Sense::click_and_drag());
        if let Some(touch) = ui.input(|i| i.multi_touch())
            && area.contains(touch.center_pos)
        {
            s.scale = (s.scale * touch.zoom_delta).clamp(6.0, 900.0);
            s.grab = None;
            s.pinching = true;
        } else if s.pinching && !ui.input(|i| i.pointer.any_down()) {
            s.pinching = false;
        }
        let playhead = s.playhead;
        let scale = s.scale;
        let center = track.center().x;
        let x_of = |t: i64| center + (t - playhead) as f32 * scale / 1000.0;
        let to_ms = |dx: f32| (dx * 1000.0 / scale) as i64;
        let visible = area.height() - RULER - 4.0;
        let content = s.project.layers.len() as f32 * (LANE + 2.0) + FILM;
        let max = (content - visible).max(0.0);
        s.rows = s.rows.clamp(0.0, max);
        let mut y = area.top() + RULER + 4.0 - s.rows;
        let mut lanes: Vec<(Lane, Rect)> = vec![];
        for l in (0..s.project.layers.len()).rev() {
            lanes.push((Lane::Layer(l), Rect::from_min_size(pos2(area.left(), y), vec2(area.width(), LANE))));
            y += LANE + 2.0;
        }
        lanes.push((Lane::Film, Rect::from_min_size(pos2(area.left(), y), vec2(area.width(), FILM))));
        s.track = track;
        s.lanes = lanes.clone();
        let body = Rect::from_min_max(pos2(area.left(), area.top() + RULER), area.max);
        let painter = ui.painter().with_clip_rect(body);
        let track_painter = ui.painter().with_clip_rect(track.intersect(body));
        let lane_at = |p: Pos2| lanes.iter().find(|(_, r)| r.y_range().contains(p.y)).map(|(l, r)| (*l, *r));
        // Lanes, clips and segments.
        for &(lane, rect) in &lanes {
            if !rect.intersects(body) {
                continue;
            }
            painter.rect_filled(rect, 0, Color32::from_white_alpha(6));
            match lane {
                Lane::Layer(l) => {
                    let layer = &s.project.layers[l];
                    let strength = if layer.visible { 1.0 } else { 0.35 };
                    for clip in &layer.clips {
                        let (x0, x1) = (x_of(clip.start), x_of(clip.end));
                        if x1 < track.left() || x0 > track.right() {
                            continue;
                        }
                        let r = Rect::from_min_max(pos2(x0, rect.top() + 3.0), pos2(x1, rect.bottom() - 3.0));
                        let picked = s.pick == Some(Pick::Clip(clip.id));
                        let lifted = s.armed == Some(clip.id) || matches!(s.grab, Some((Grab::Body { id, .. }, _)) if id == clip.id);
                        if lifted {
                            track_painter.add(icons::selection_glow(r));
                        }
                        glass(&track_painter, r, layer.rgb(), strength, picked);
                        let label = match &clip.body {
                            Body::Text(style) if style.text.trim().is_empty() => "Text".to_owned(),
                            _ => clip.label(),
                        };
                        let left = x0.max(track.left()) + 7.0;
                        if r.right() - left > 14.0 {
                            let galley = ui.fonts_mut(|f| {
                                let mut job = egui::text::LayoutJob::simple_singleline(
                                    label,
                                    FontId::proportional(12.0),
                                    rgb([255, 255, 255], strength),
                                );
                                job.wrap.max_width = r.right() - left - 4.0;
                                job.wrap.max_rows = 1;
                                job.wrap.break_anywhere = true;
                                f.layout_job(job)
                            });
                            track_painter.galley(pos2(left, r.center().y - galley.size().y / 2.0), galley, TEXT);
                        }
                        if picked {
                            handles(&track_painter, r);
                            if r.width() > 70.0 {
                                track_painter.text(
                                    pos2(r.right().min(track.right()) - 5.0, r.top() + 2.0),
                                    Align2::RIGHT_TOP,
                                    format!("{:.1}s", clip.duration() as f32 / 1000.0),
                                    FontId::proportional(9.0),
                                    Color32::from_white_alpha(200),
                                );
                            }
                        }
                    }
                }
                Lane::Film => {
                    let starts = s.project.starts();
                    let (ow, oh) = s.project.output_size(0);
                    let tile = (FILM * ow as f32 / oh.max(1) as f32).clamp(28.0, 110.0);
                    let tile_ms = to_ms(tile).max(40);
                    let mut picked_rect = None;
                    for (i, segment) in s.project.segments.clone().iter().enumerate() {
                        let start = starts[i];
                        let (x0, x1) = (x_of(start), x_of(start + segment.length()));
                        if x1 < track.left() || x0 > track.right() {
                            continue;
                        }
                        let r = Rect::from_min_max(pos2(x0 + 1.0, rect.top()), pos2(x1 - 1.0, rect.bottom()));
                        let film = track_painter.with_clip_rect(r.intersect(track_painter.clip_rect()));
                        film.rect_filled(r, 6, PANEL);
                        let first = (((track.left() - x0) / tile).floor().max(0.0)) as i64;
                        let last = (((track.right() - x0) / tile).ceil()) as i64;
                        for k in first..=last {
                            let at = k * tile_ms;
                            if at >= segment.length() {
                                break;
                            }
                            let tile_rect = Rect::from_min_size(pos2(x0 + k as f32 * tile, r.top()), vec2(tile, r.height()));
                            let source = ((segment.source_in + at) / 100 * 100).min((s.project.duration - 100).max(0));
                            if let Some(texture) = self.thumb(&s.item, source) {
                                let t = texture.size_vec2();
                                let factor = (tile / t.x).max(r.height() / t.y);
                                let uv = Rect::from_center_size(pos2(0.5, 0.5), tile_rect.size() / (t * factor));
                                film.image(texture.id(), tile_rect, uv, Color32::WHITE);
                            }
                        }
                        let wave = Rect::from_min_max(pos2(r.left(), r.bottom() - 15.0), r.max);
                        film.rect_filled(wave, 0, Color32::from_black_alpha(150));
                        if !self.waveform.peaks.is_empty() {
                            let columns = ((x1.min(track.right()) - x0.max(track.left())) / 2.0).max(0.0) as i32;
                            for n in 0..columns {
                                let x = x0.max(track.left()) + n as f32 * 2.0 + 1.0;
                                let from = segment.source_in as f64 + ((x - x0) * 1000.0 / scale) as f64;
                                let peak = self.waveform.peak(from, from + 2000.0 / scale as f64).sqrt() * 6.5;
                                film.vline(
                                    x,
                                    wave.center().y - peak..=wave.center().y + peak,
                                    Stroke::new(1.0, if s.project.audio.keep { TRIM } else { MUTED }),
                                );
                            }
                        }
                        for (ms, at_start) in [(segment.fade_in, true), (segment.fade_out, false)] {
                            if ms <= 0 {
                                continue;
                            }
                            let w = (ms as f32 * scale / 1000.0).min(r.width());
                            let (edge, inner) = if at_start { (r.left(), r.left() + w) } else { (r.right(), r.right() - w) };
                            let mut mesh = egui::Mesh::default();
                            let shade = if segment.white { Color32::from_white_alpha(150) } else { Color32::from_black_alpha(190) };
                            mesh.colored_vertex(pos2(edge, r.top()), shade);
                            mesh.colored_vertex(pos2(edge, r.bottom()), shade);
                            mesh.colored_vertex(pos2(inner, r.top()), Color32::TRANSPARENT);
                            mesh.add_triangle(0, 1, 2);
                            film.add(egui::Shape::mesh(mesh));
                        }
                        let picked = s.pick == Some(Pick::Segment(i));
                        film.rect_stroke(
                            r,
                            6,
                            if picked { Stroke::new(2.0, Color32::WHITE) } else { Stroke::new(1.0, LINE) },
                            StrokeKind::Inside,
                        );
                        if let Some((Grab::Cut { index, head, delta }, _)) = s.grab
                            && index == i
                        {
                            let w = delta.unsigned_abs() as f32 * scale / 1000.0;
                            let trimmed = if head == (delta > 0) {
                                if head {
                                    Rect::from_min_max(r.min, pos2(r.left() + w, r.bottom()))
                                } else {
                                    Rect::from_min_max(pos2(r.right() - w, r.top()), r.max)
                                }
                            } else {
                                Rect::NOTHING
                            };
                            track_painter.rect_filled(trimmed, 0, Color32::from_black_alpha(170));
                        }
                        if picked {
                            picked_rect = Some(r);
                        }
                    }
                    if let Some(r) = picked_rect {
                        handles(&track_painter, r);
                    }
                    for (n, layer) in s.project.layers.iter().filter(|l| l.visible).take(5).enumerate() {
                        let band_y = rect.top() + 3.0 + n as f32 * 5.0;
                        for clip in &layer.clips {
                            let band = Rect::from_min_max(pos2(x_of(clip.start), band_y), pos2(x_of(clip.end), band_y + 4.0));
                            track_painter.rect_filled(band, 2, rgb(layer.rgb(), 0.85));
                        }
                    }
                }
            }
        }
        let end = x_of(length);
        if end < track.right() {
            track_painter.rect_filled(
                Rect::from_min_max(pos2(end, body.top()), pos2(track.right(), body.bottom())),
                0,
                Color32::from_black_alpha(140),
            );
        }
        // Headers.
        for &(lane, rect) in &lanes {
            let header = Rect::from_min_size(rect.min, vec2(HEADER - 4.0, rect.height()));
            if !header.intersects(body) {
                continue;
            }
            painter.rect_filled(header, 4, PANEL);
            match lane {
                Lane::Layer(l) => {
                    let layer = &s.project.layers[l];
                    painter.rect_filled(
                        Rect::from_min_size(header.min, vec2(3.0, header.height())),
                        1,
                        rgb(layer.rgb(), 1.0),
                    );
                    let eye = Rect::from_center_size(header.left_center() + vec2(15.0, 0.0), Vec2::splat(22.0));
                    let lock = Rect::from_center_size(header.left_center() + vec2(35.0, 0.0), Vec2::splat(18.0));
                    icons::image(if layer.visible { "eye" } else { "eye_off" }, if layer.visible { TEXT } else { MUTED }, 17.0)
                        .paint_at(ui, Rect::from_center_size(eye.center(), Vec2::splat(17.0)));
                    icons::image(if layer.locked { "lock" } else { "unlock" }, if layer.locked { ACCENT } else { MUTED.gamma_multiply(0.6) }, 13.0)
                        .paint_at(ui, Rect::from_center_size(lock.center(), Vec2::splat(13.0)));
                    let name = layer.name.clone();
                    let eye_response = ui
                        .interact(eye.intersect(body), ui.id().with(("eye", layer.id)), Sense::click())
                        .on_hover_text(format!("Show or hide {name}"));
                    eye_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Visibility {name}")));
                    if eye_response.clicked() {
                        s.project.layers[l].visible = !s.project.layers[l].visible;
                    }
                    let lock_response = ui.interact(lock.intersect(body), ui.id().with(("lock", s.project.layers[l].id)), Sense::click());
                    if lock_response.clicked() {
                        s.project.layers[l].locked = !s.project.layers[l].locked;
                    }
                }
                Lane::Film => {
                    icons::image("video", TEXT, 17.0).paint_at(ui, Rect::from_center_size(header.center() - vec2(0.0, 11.0), Vec2::splat(17.0)));
                    let toggle = Rect::from_center_size(header.center() + vec2(0.0, 13.0), Vec2::splat(22.0));
                    let keep = s.project.audio.keep;
                    icons::image(if keep { "volume" } else { "mute" }, if keep { TRIM } else { MUTED }, 16.0)
                        .paint_at(ui, Rect::from_center_size(toggle.center(), Vec2::splat(16.0)));
                    let response = ui.interact(toggle.intersect(body), ui.id().with("film_audio"), Sense::click());
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Video audio"));
                    if response.clicked() && s.project.has_audio {
                        s.project.audio.keep = !keep;
                    }
                }
            }
        }
        // Ruler.
        let ruler = Rect::from_min_size(pos2(track.left(), area.top()), vec2(track.width(), RULER));
        let ruler_painter = ui.painter().with_clip_rect(ruler);
        let step = [100, 200, 500, 1000, 2000, 5000, 10_000, 15_000, 30_000, 60_000, 120_000, 300_000]
            .into_iter()
            .find(|step| *step as f32 * scale / 1000.0 >= 70.0)
            .unwrap_or(600_000);
        let minor = step / 5;
        let from = (playhead - to_ms(track.width())) / minor * minor;
        let mut t = from.max(0);
        while x_of(t) <= ruler.right() && t <= length.max(0) + step {
            let x = x_of(t);
            let major = t % step == 0;
            ruler_painter.vline(x, ruler.bottom() - if major { 7.0 } else { 3.0 }..=ruler.bottom(), Stroke::new(1.0, MUTED));
            if major {
                ruler_painter.text(pos2(x + 3.0, ruler.top() + 1.0), Align2::LEFT_TOP, timecode(t, step < 1000), FontId::proportional(9.0), MUTED);
            }
            t += minor.max(1);
        }
        if let Some(t) = s.snapped.filter(|_| s.grab.is_some()) {
            track_painter.vline(x_of(t), body.y_range(), Stroke::new(1.0, Color32::from_rgb(255, 221, 0)));
        }
        let head = ui.painter().with_clip_rect(area);
        head.vline(center, area.top()..=area.bottom(), Stroke::new(2.0, Color32::WHITE));
        head.circle_filled(pos2(center, area.top() + 4.0), 4.0, Color32::WHITE);
        self.studio_timeline_gestures(s, ui, &response, x_of(0), lane_at);
    }

    fn studio_timeline_gestures(
        &mut self,
        s: &mut Studio,
        ui: &egui::Ui,
        response: &egui::Response,
        zero: f32,
        lane_at: impl Fn(Pos2) -> Option<(Lane, Rect)>,
    ) {
        if s.pinching {
            return;
        }
        let scale = s.scale;
        let x_of = |t: i64| zero + t as f32 * scale / 1000.0;
        let t_of = |x: f32| ((x - zero) * 1000.0 / scale) as i64;
        let clip_at = |s: &Studio, p: Pos2| -> Option<u64> {
            match lane_at(p)? {
                (Lane::Layer(l), _) => s.project.layers[l]
                    .clips
                    .iter()
                    .find(|c| (x_of(c.start)..=x_of(c.end)).contains(&p.x))
                    .map(|c| c.id),
                _ => None,
            }
        };
        let held = ui.input(|i| i.pointer.press_start_time().map(|start| i.time - start));
        if s.grab.is_none() && response.is_pointer_button_down_on() {
            let origin = ui.input(|i| i.pointer.press_origin());
            let still = origin.zip(response.interact_pointer_pos()).is_some_and(|(o, p)| o.distance(p) < 8.0);
            if still && held.is_some_and(|h| h > 0.35) && s.armed.is_none() {
                s.armed = origin.and_then(|o| clip_at(s, o)).filter(|id| {
                    s.project.locate_clip(*id).is_some_and(|(l, _)| !s.project.layers[l].locked)
                });
                if s.armed.is_some() {
                    s.haptic = Some(egui_mobile::Haptic::Medium);
                }
            }
        }
        if response.drag_started() {
            let origin = ui.input(|i| i.pointer.press_origin()).unwrap_or(response.rect.center());
            let pointer = response.interact_pointer_pos().unwrap_or(origin);
            let lane = lane_at(origin);
            let near = |x: f32| (origin.x - x).abs() <= 18.0;
            let mut grab = None;
            if let Some(Pick::Clip(id)) = s.pick
                && let Some((l, c)) = s.project.locate_clip(id)
                && lane.is_some_and(|(lane, _)| lane == Lane::Layer(l))
                && !s.project.layers[l].locked
            {
                let clip = &s.project.layers[l].clips[c];
                grab = if near(x_of(clip.start)) {
                    Some(Grab::Edge { id, head: true, at: clip.start })
                } else if near(x_of(clip.end)) {
                    Some(Grab::Edge { id, head: false, at: clip.end })
                } else if (x_of(clip.start)..=x_of(clip.end)).contains(&origin.x) {
                    Some(Grab::Body { id, start: clip.start })
                } else {
                    None
                };
            }
            if grab.is_none()
                && let Some(Pick::Segment(index)) = s.pick
                && lane.is_some_and(|(lane, _)| lane == Lane::Film)
                && index < s.project.segments.len()
            {
                let start = s.project.starts()[index];
                let end = start + s.project.segments[index].length();
                if near(x_of(start)) {
                    grab = Some(Grab::Cut { index, head: true, delta: 0 });
                } else if near(x_of(end)) {
                    grab = Some(Grab::Cut { index, head: false, delta: 0 });
                }
            }
            if grab.is_none()
                && let Some(id) = s.armed
                && let Some(clip) = s.project.clip(id)
            {
                s.pick = Some(Pick::Clip(id));
                grab = Some(Grab::Body { id, start: clip.start });
            }
            let grab = grab.unwrap_or_else(|| {
                let d = pointer - origin;
                if d.y.abs() > d.x.abs() * 1.3 {
                    Grab::Rows { offset: s.rows }
                } else {
                    bridge::send(json!({"op":"play","playing":false}));
                    Grab::Scrub { playhead: s.playhead }
                }
            });
            s.grab = Some((grab, origin));
        }
        if let Some((grab, origin)) = s.grab
            && let Some(pointer) = pointer(ui, response)
        {
            let dx = pointer.x - origin.x;
            let ms = (dx * 1000.0 / scale) as i64;
            match grab {
                Grab::Scrub { playhead } => {
                    s.playhead = (playhead - ms).clamp(0, s.project.length());
                    if s.last_seek.elapsed() > Duration::from_millis(90) {
                        s.last_seek = Instant::now();
                        let t = s.playhead;
                        s.seek(t, true);
                    }
                }
                Grab::Rows { offset } => s.rows = offset - (pointer.y - origin.y),
                Grab::Body { id, start } => {
                    let Some(duration) = s.project.clip(id).map(Clip::duration) else {
                        return;
                    };
                    let mut next = (start + ms).max(0);
                    if let Some(t) = s.snap(next, id) {
                        next = t;
                    } else if let Some(t) = s.snap(next + duration, id) {
                        next = t - duration;
                    }
                    let kind = s.project.clip(id).map(Clip::kind);
                    let layer = match lane_at(pointer) {
                        Some((Lane::Layer(l), _))
                            if Some(s.project.layers[l].kind) == kind && !s.project.layers[l].locked =>
                        {
                            l
                        }
                        _ => s.project.locate_clip(id).map_or(0, |(l, _)| l),
                    };
                    s.project.move_clip(id, next, layer);
                }
                Grab::Edge { id, head, at } => {
                    let t = s.snap(at + ms, id).unwrap_or(at + ms);
                    s.project.set_clip_edge(id, head, t);
                }
                Grab::Cut { index, head, .. } => {
                    let segments = &s.project.segments;
                    let segment = &segments[index];
                    let delta = if head {
                        let floor = index.checked_sub(1).map_or(0, |i| segments[i].source_out);
                        (segment.source_in + ms).clamp(floor, segment.source_out - MIN_CLIP) - segment.source_in
                    } else {
                        let ceiling = segments.get(index + 1).map_or(s.project.duration, |s| s.source_in);
                        (segment.source_out + ms).clamp(segment.source_in + MIN_CLIP, ceiling) - segment.source_out
                    };
                    s.grab = Some((Grab::Cut { index, head, delta }, origin));
                }
                Grab::Move { .. } | Grab::Turn { .. } => {}
            }
        }
        if response.drag_stopped() {
            match s.grab.take() {
                Some((Grab::Cut { index, head, delta }, _)) if delta != 0 => {
                    s.project.trim_segment(index, head, delta);
                    let t = s.playhead.min(s.project.length());
                    s.seek(t, false);
                }
                Some((Grab::Scrub { .. }, _)) => {
                    let t = s.playhead;
                    s.seek(t, false);
                }
                _ => {}
            }
            s.snapped = None;
            s.armed = None;
        }
        if !ui.input(|i| i.pointer.any_down())
            && s.grab.is_none()
            && let Some(id) = s.armed.take()
        {
            s.pick = Some(Pick::Clip(id));
        }
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
        {
            s.pick = match (clip_at(s, p), lane_at(p)) {
                (Some(id), _) => Some(Pick::Clip(id)),
                (None, Some((Lane::Film, _))) => {
                    let t = t_of(p.x);
                    (0..=s.project.length())
                        .contains(&t)
                        .then(|| s.project.locate(t).map(|(i, _)| Pick::Segment(i)))
                        .flatten()
                }
                _ => None,
            };
            if s.pick.is_none() && s.panel != Panel::None {
                s.panel = Panel::None;
            }
        }
    }

    fn studio_tools(&mut self, s: &mut Studio, ui: &mut egui::Ui) {
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, LINE));
        let clip = s.clip().map(|c| c.kind());
        let tools: Vec<(&str, &str, bool)> = match (s.pick, clip) {
            (Some(Pick::Clip(_)), Some(kind)) => {
                let mut tools = vec![];
                if kind == LayerKind::Text {
                    tools.push(("edit", "Edit", true));
                }
                tools.extend([
                    ("brush", "Style", true),
                    ("sparkles", "Animate", true),
                    ("cut", "Split", true),
                    ("copy", "Copy", true),
                    ("delete", "Delete", true),
                ]);
                tools
            }
            (Some(Pick::Segment(_)), _) => vec![
                ("cut", "Split", true),
                ("fade", "Fade", true),
                ("volume", "Audio", true),
                ("delete", "Delete", s.project.segments.len() > 1),
            ],
            _ => vec![
                ("cut", "Split", true),
                ("text", "Text", true),
                ("photo", "Overlay", true),
                ("layers", "Layers", true),
                ("fade", "Fade", true),
                ("volume", "Audio", true),
                ("crop", "Canvas", true),
                ("chip", "Debug", true),
            ],
        };
        if let Some(n) = tool_strip(ui, &tools) {
            match tools[n].1 {
                "Edit" => {
                    s.panel = Panel::Text;
                    s.focus_text = true;
                }
                "Style" => s.panel = Panel::Style,
                "Animate" => s.panel = Panel::Motion,
                "Split" => s.split(),
                "Copy" => {
                    if let Some(Pick::Clip(id)) = s.pick
                        && let Some(copy) = s.project.duplicate_clip(id)
                    {
                        s.pick = Some(Pick::Clip(copy));
                    }
                }
                "Delete" => s.delete(),
                "Text" => s.add_text(),
                "Overlay" => s.panel = Panel::Photos,
                "Layers" => s.panel = Panel::Layers,
                "Fade" => s.panel = Panel::Fade,
                "Audio" => s.panel = Panel::Audio,
                "Canvas" => s.panel = Panel::Canvas,
                "Debug" => s.panel = Panel::Debug,
                _ => {}
            }
        }
    }

    fn studio_panel(&mut self, s: &mut Studio, ui: &mut egui::Ui, host: &Host) {
        ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_rgb(58, 44, 70);
        ui.spacing_mut().slider_width = (ui.available_width() - 150.0).clamp(120.0, 420.0);
        let title = match s.panel {
            Panel::Text => "Text",
            Panel::Style => "Style",
            Panel::Motion => "Animation",
            Panel::Fade => "Fade",
            Panel::Audio => "Audio",
            Panel::Canvas => "Canvas",
            Panel::Layers => "Layers",
            Panel::Export => "Export",
            Panel::Photos => "Add overlay",
            Panel::Debug => "Debug",
            Panel::None => "",
        };
        if panel_title(ui, title) {
            if s.panel == Panel::Text
                && let Some(Pick::Clip(id)) = s.pick
                && s.project.clip(id).and_then(Clip::text).is_some_and(|t| t.text.trim().is_empty())
            {
                s.project.remove_clip(id);
                s.pick = None;
            }
            s.panel = Panel::None;
            ui.memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt(("studio_panel", title))
            .scroll_source(egui::scroll_area::ScrollSource::ALL)
            .auto_shrink([false, false])
            .show(ui, |ui| match s.panel {
                Panel::Text => self.studio_text(s, ui),
                Panel::Style => self.studio_style(s, ui),
                Panel::Motion => {
                    if let Some(clip) = s.clip_mut() {
                        motion_row(ui, "In", &mut clip.enter, &mut clip.enter_ms);
                        ui.add_space(6.0);
                        motion_row(ui, "Out", &mut clip.exit, &mut clip.exit_ms);
                        let start = clip.start;
                        ui.add_space(6.0);
                        if ui.button("Preview from start").clicked() {
                            s.seek(start, false);
                            bridge::send(json!({"op":"play","playing":true}));
                        }
                    } else {
                        ui.label("Select a caption or overlay first.");
                    }
                }
                Panel::Fade => self.studio_fade(s, ui),
                Panel::Audio => {
                    if !s.project.has_audio {
                        ui.label("This video has no audio track.");
                        return;
                    }
                    let audio = &mut s.project.audio;
                    ui.checkbox(&mut audio.keep, "Keep the video's audio");
                    ui.add_enabled_ui(audio.keep, |ui| {
                        ui.add(egui::Slider::new(&mut audio.volume, 0.0..=2.0).text("Volume").custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                        ms_slider(ui, &mut audio.fade_in, 5000, "Fade in");
                        ms_slider(ui, &mut audio.fade_out, 5000, "Fade out");
                    });
                    ui.small("Cut fades also fade the audio inside each segment.");
                }
                Panel::Canvas => {
                    let (w, h) = (s.project.width, s.project.height);
                    editor::crop_presets(ui, &mut s.project.crop, w, h);
                    let (ow, oh) = s.project.output_size(0);
                    ui.small(format!("Output {ow} x {oh}. Drag the frame or its corners."));
                    ui.small("Captions keep their place relative to the cropped frame.");
                }
                Panel::Layers => self.studio_layers(s, ui),
                Panel::Export => self.studio_export(s, ui),
                Panel::Photos => self.studio_photos(s, ui),
                Panel::Debug => self.studio_debug(s, ui, host),
                Panel::None => {}
            });
    }

    fn studio_text(&mut self, s: &mut Studio, ui: &mut egui::Ui) {
        let focus = std::mem::take(&mut s.focus_text);
        let Some(style) = s.clip_mut().and_then(Clip::text_mut) else {
            ui.label("Select a caption, or add one with Text.");
            return;
        };
        let response = ui.add(
            egui::TextEdit::multiline(&mut style.text)
                .hint_text("Enter text")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        if focus {
            response.request_focus();
        }
        let current = style.clone();
        ui.add_space(4.0);
        let mut chosen = None;
        egui::ScrollArea::horizontal().id_salt("presets").show(ui, |ui| {
            ui.horizontal(|ui| {
                for (name, preset) in presets() {
                    let sample = TextStyle {
                        text: "Aa".into(),
                        font: current.font.clone(),
                        size: 0.2,
                        ..preset.clone()
                    };
                    let tile = s.tile(ui.ctx(), format!("preset:{name}:{}", current.font), &sample);
                    let (rect, response) = ui.allocate_exact_size(vec2(66.0, 50.0), Sense::click());
                    ui.painter().rect_filled(rect, 6, Color32::from_gray(48));
                    let size = tile.size_vec2();
                    let fit = (58.0 / size.x).min(34.0 / size.y).min(1.0);
                    ui.painter().image(
                        tile.id(),
                        Rect::from_center_size(rect.center() - vec2(0.0, 5.0), size * fit),
                        Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    ui.painter().text(rect.center_bottom() - vec2(0.0, 2.0), Align2::CENTER_BOTTOM, name, FontId::proportional(9.0), MUTED);
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
                    if response.clicked() {
                        chosen = Some(preset);
                    }
                }
            });
        });
        if let Some(preset) = chosen
            && let Some(style) = s.clip_mut().and_then(Clip::text_mut)
        {
            style.apply_preset(&preset);
        }
    }

    fn studio_style(&mut self, s: &mut Studio, ui: &mut egui::Ui) {
        let Some(clip) = s.clip().cloned() else {
            ui.label("Select a caption or overlay first.");
            return;
        };
        match clip.body {
            Body::Text(style) => {
                ui.label(egui::RichText::new("Font").strong());
                let mut font = None;
                egui::ScrollArea::horizontal().id_salt("fonts").show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let families: Vec<String> = caption::book().families().iter().map(|f| f.name.clone()).collect();
                        for family in families {
                            let sample = TextStyle {
                                text: family.clone(),
                                font: family.clone(),
                                size: 0.1,
                                bold: false,
                                outline: 0.0,
                                ..TextStyle::default()
                            };
                            let tile = s.tile(ui.ctx(), format!("font:{family}"), &sample);
                            let size = tile.size_vec2();
                            let fit = (30.0 / size.y).min(1.0);
                            let (rect, response) = ui.allocate_exact_size(vec2(size.x * fit + 12.0, 38.0), Sense::click());
                            let chosen = style.font == family
                                || (style.font.is_empty() && family == caption::book().default_family());
                            ui.painter().rect_filled(rect, 6, if chosen { Color32::from_rgb(65, 11, 33) } else { Color32::from_gray(40) });
                            if chosen {
                                ui.painter().rect_stroke(rect, 6, Stroke::new(1.0, ACCENT), StrokeKind::Inside);
                            }
                            ui.painter().image(
                                tile.id(),
                                Rect::from_center_size(rect.center(), size * fit),
                                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                                Color32::WHITE,
                            );
                            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &family));
                            if response.clicked() {
                                font = Some(family);
                            }
                        }
                    });
                });
                let Some(style) = s.clip_mut().and_then(Clip::text_mut) else {
                    return;
                };
                if let Some(font) = font {
                    style.font = font;
                }
                ui.add(egui::Slider::new(&mut style.size, 0.015..=0.3).text("Size").custom_formatter(|v, _| format!("{:.1}%", v * 100.0)));
                ui.horizontal_wrapped(|ui| {
                    for (label, value) in [("B", &mut style.bold), ("I", &mut style.italic), ("U", &mut style.underline), ("AA", &mut style.uppercase)] {
                        if ui.add(egui::Button::new(egui::RichText::new(label).strong()).selected(*value).min_size(vec2(38.0, 34.0))).clicked() {
                            *value = !*value;
                        }
                    }
                    ui.separator();
                    for (align, icon, label) in [
                        (TextAlign::Left, "align_left", "Align left"),
                        (TextAlign::Center, "align_center", "Align center"),
                        (TextAlign::Right, "align_right", "Align right"),
                    ] {
                        if icons::button(ui, icon, label, style.align == align).clicked() {
                            style.align = align;
                        }
                    }
                });
                ui.label(egui::RichText::new("Color").strong());
                swatches(ui, &mut style.color);
                ui.label(egui::RichText::new("Outline").strong());
                swatches(ui, &mut style.outline_color);
                ui.add(egui::Slider::new(&mut style.outline, 0.0..=0.25).text("Width"));
                ui.label(egui::RichText::new("Box").strong());
                swatches(ui, &mut style.background);
                let mut opacity = style.background[3] as f32 / 255.0;
                if ui.add(egui::Slider::new(&mut opacity, 0.0..=1.0).text("Opacity")).changed() {
                    style.background[3] = (opacity * 255.0).round() as u8;
                }
                ui.add(egui::Slider::new(&mut style.radius, 0.0..=1.0).text("Corners"));
                ui.add(egui::Slider::new(&mut style.padding, 0.0..=1.0).text("Padding"));
                ui.label(egui::RichText::new("Shadow").strong());
                swatches(ui, &mut style.shadow_color);
                ui.add(egui::Slider::new(&mut style.shadow, 0.0..=0.4).text("Blur"));
                ui.horizontal(|ui| {
                    if ui.selectable_label(style.shadow_offset != [0.0, 0.0], "Drop").clicked() {
                        style.shadow_offset = [0.04, 0.06];
                    }
                    if ui.selectable_label(style.shadow_offset == [0.0, 0.0], "Glow").clicked() {
                        style.shadow_offset = [0.0, 0.0];
                    }
                });
                ui.label(egui::RichText::new("Spacing").strong());
                ui.add(egui::Slider::new(&mut style.spacing, -0.1..=0.6).text("Letters"));
                ui.add(egui::Slider::new(&mut style.line_height, 0.7..=2.0).text("Lines"));
                ui.add(egui::Slider::new(&mut style.wrap, 0.15..=1.0).text("Wrap width"));
            }
            Body::Picture(_) => {
                let Some(clip) = s.clip_mut() else { return };
                if let Body::Picture(picture) = &mut clip.body {
                    ui.add(egui::Slider::new(&mut picture.width, 0.04..=2.0).text("Size").custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                    ui.add(egui::Slider::new(&mut picture.radius, 0.0..=0.5).text("Corners"));
                    if ui.button("Fill width").clicked() {
                        picture.width = 1.0;
                        clip.x = 0.5;
                    }
                }
            }
        }
        let Some(clip) = s.clip_mut() else { return };
        ui.label(egui::RichText::new("Placement").strong());
        ui.add(egui::Slider::new(&mut clip.opacity, 0.0..=1.0).text("Opacity").custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
        ui.add(egui::Slider::new(&mut clip.rotation, -180.0..=180.0).text("Rotation").suffix(" deg"));
        ui.horizontal(|ui| {
            if ui.button("Center").clicked() {
                (clip.x, clip.y) = (0.5, 0.5);
            }
            if ui.button("Lower third").clicked() {
                (clip.x, clip.y) = (0.5, 0.8);
            }
            if ui.button("Top").clicked() {
                (clip.x, clip.y) = (0.5, 0.15);
            }
            if ui.button("Straighten").clicked() {
                clip.rotation = 0.0;
            }
        });
    }

    fn studio_fade(&mut self, s: &mut Studio, ui: &mut egui::Ui) {
        let index = match s.pick {
            Some(Pick::Segment(i)) if i < s.project.segments.len() => i,
            _ => s.project.locate(s.playhead).map_or(0, |(i, _)| i),
        };
        let count = s.project.segments.len();
        ui.label(format!("Segment {} of {count}", index + 1));
        let segment = &mut s.project.segments[index];
        let limit = (segment.length() / 2).min(5000);
        ms_slider(ui, &mut segment.fade_in, limit, "Fade in");
        ms_slider(ui, &mut segment.fade_out, limit, "Fade out");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut segment.white, false, "Through black");
            ui.selectable_value(&mut segment.white, true, "Through white");
        });
        ui.separator();
        if ui.button("Fade the whole video in and out").clicked() {
            let first = &mut s.project.segments[0];
            first.fade_in = 800.min(first.length() / 2);
            let last = s.project.segments.last_mut().expect("never empty");
            last.fade_out = 800.min(last.length() / 2);
        }
        let starts = s.project.starts();
        let cut = (1..count).min_by_key(|&i| (starts[i] - s.playhead).abs());
        if let Some(i) = cut
            && (starts[i] - s.playhead).abs() < 1500
        {
            if ui.button(format!("Dip to black at cut {}", i)).clicked() {
                let left = &mut s.project.segments[i - 1];
                left.fade_out = 400.min(left.length() / 2);
                left.white = false;
                let right = &mut s.project.segments[i];
                right.fade_in = 400.min(right.length() / 2);
                right.white = false;
            }
        } else if count > 1 {
            ui.small("Move the playhead near a cut to add a dip there.");
        } else {
            ui.small("Split the video to add transitions between parts.");
        }
    }

    fn studio_layers(&mut self, s: &mut Studio, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.add(egui::Button::image_and_text(icons::image("text", TEXT, 16.0), "Text layer")).clicked() {
                s.project.add_layer(LayerKind::Text);
            }
            if ui.add(egui::Button::image_and_text(icons::image("photo", TEXT, 16.0), "Overlay layer")).clicked() {
                s.project.add_layer(LayerKind::Picture);
            }
        });
        ui.small("Top of the list draws in front. Hidden layers are left out of exports.");
        let mut action = None;
        for l in (0..s.project.layers.len()).rev() {
            let layer = &mut s.project.layers[l];
            ui.horizontal(|ui| {
                let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), Sense::click());
                ui.painter().circle_filled(rect.center(), 9.0, rgb(layer.rgb(), 1.0));
                if response.on_hover_text("Change color").clicked() {
                    layer.color = (layer.color + 1) % PALETTE.len();
                }
                ui.add(egui::TextEdit::singleline(&mut layer.name).desired_width(96.0));
                ui.label(egui::RichText::new(format!("{}", layer.clips.len())).small().color(MUTED));
                if icons::button(ui, if layer.visible { "eye" } else { "eye_off" }, "Visibility", !layer.visible).clicked() {
                    layer.visible = !layer.visible;
                }
                if icons::button(ui, if layer.locked { "lock" } else { "unlock" }, "Lock", layer.locked).clicked() {
                    layer.locked = !layer.locked;
                }
                if ui.small_button("Up").clicked() {
                    action = Some((l, 1));
                }
                if ui.small_button("Down").clicked() {
                    action = Some((l, 2));
                }
                if icons::button(ui, "delete", "Delete layer", false).clicked() {
                    action = Some((l, 3));
                }
            });
        }
        if let Some((l, kind)) = action {
            match kind {
                1 => {
                    s.project.move_layer(l, true);
                }
                2 => {
                    s.project.move_layer(l, false);
                }
                _ => {
                    let removed = s.project.layers.remove(l);
                    if removed.clips.iter().any(|c| s.pick == Some(Pick::Clip(c.id))) {
                        s.pick = None;
                    }
                }
            }
        }
        ui.separator();
        let has_audio = s.project.has_audio;
        let segments = s.project.segments.len();
        ui.horizontal(|ui| {
            ui.add(icons::image("video", TEXT, 18.0));
            ui.label(format!("Video  ·  {segments} segment(s)"));
            ui.add_enabled(has_audio, egui::Checkbox::new(&mut s.project.audio.keep, "Audio"));
        });
    }

    fn studio_photos(&mut self, s: &mut Studio, ui: &mut egui::Ui) {
        let photos: Vec<MediaItem> = self.items.iter().filter(|i| i.kind == "photo").take(240).cloned().collect();
        if photos.is_empty() {
            ui.label("No photos in the library. Grant photo access in Settings.");
            return;
        }
        let side = ((ui.available_width() - 12.0) / 4.0).floor();
        let mut chosen = None;
        egui::Grid::new("studio_photos").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
            for (n, item) in photos.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
                if ui.is_rect_visible(rect) {
                    self.cover(ui, item, rect, 0, Color32::WHITE);
                }
                response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &item.name));
                if response.clicked() {
                    chosen = Some(item.clone());
                }
                if n % 4 == 3 {
                    ui.end_row();
                }
            }
        });
        if let Some(item) = chosen {
            let aspect = if item.width > 0 { item.height as f32 / item.width as f32 } else { 1.0 };
            let (start, end) = s.slot();
            let id = s.project.add_clip(Clip {
                start,
                end,
                y: 0.45,
                body: Body::Picture(Picture {
                    uri: item.uri.clone(),
                    name: item.name.clone(),
                    width: 0.42,
                    aspect,
                    radius: 0.06,
                    ..Picture::default()
                }),
                ..Clip::default()
            });
            let session = format!("studio-photo-{id}");
            s.waiting.insert(session.clone(), id);
            bridge::send(json!({"op":"photo_prepare","session":session,"item":item}));
            s.pick = Some(Pick::Clip(id));
            s.panel = Panel::None;
        }
    }

    fn studio_export(&mut self, s: &mut Studio, ui: &mut egui::Ui) {
        let original = s.project.output_size(0);
        ui.label(egui::RichText::new("Resolution").strong());
        ui.horizontal_wrapped(|ui| {
            for (height, label) in [(0, format!("Original {} x {}", original.0, original.1)), (1080, "1080p".into()), (720, "720p".into())] {
                let enabled = height == 0 || height < original.1;
                if ui.add_enabled(enabled, egui::Button::new(label).selected(s.height == height)).clicked() {
                    s.height = height;
                }
            }
        });
        ui.label(egui::RichText::new("Quality").strong());
        ui.horizontal(|ui| {
            for (crf, label) in [(18, "High"), (21, "Balanced"), (26, "Smaller")] {
                if ui.add(egui::Button::new(label).selected(s.crf == crf)).clicked() {
                    s.crf = crf;
                }
            }
        });
        let overlays: usize = s.project.layers.iter().filter(|l| l.visible).map(|l| l.clips.len()).sum();
        let (w, h) = s.project.output_size(s.height);
        ui.small(format!(
            "{} · {w} x {h} · {} overlay(s) · audio {}",
            timecode(s.project.length(), true),
            overlays,
            if s.project.has_audio && s.project.audio.keep { "on" } else { "off" }
        ));
        let pending = s
            .project
            .layers
            .iter()
            .filter(|l| l.visible)
            .flat_map(|l| &l.clips)
            .any(|c| matches!(&c.body, Body::Picture(p) if p.path.is_empty()));
        if pending {
            ui.colored_label(ACCENT, "A photo overlay is still loading.");
        }
        let busy = self.native["exporting"] == true || s.export.is_some();
        ui.add_space(8.0);
        if ui
            .add_enabled(
                !busy && !pending,
                egui::Button::image_and_text(icons::image("save", ACCENT, 20.0), "Export video").min_size(vec2(180.0, 44.0)),
            )
            .clicked()
        {
            self.studio_start_export(s);
        }
        if s.export.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Rendering overlays…");
            });
        }
        ui.small("Saves a new video in Movies/Luma. The original is untouched.");
    }

    fn studio_start_export(&mut self, s: &mut Studio) {
        let Some(dir) = s.dir.clone() else {
            self.notice("App storage is unavailable");
            return;
        };
        let render = dir.join("render");
        let _ = std::fs::remove_dir_all(&render);
        if std::fs::create_dir_all(&render).is_err() {
            self.notice("Could not prepare the export");
            return;
        }
        s.save();
        let project = s.project.clone();
        let pictures = s.pictures.clone();
        let settings = compose::Settings {
            max_height: s.height,
            crf: s.crf,
            fps: s.fps.clone(),
        };
        let (uri, name) = (s.item.uri.clone(), s.item.name.clone());
        let (sender, receiver) = mpsc::channel();
        s.export = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(studio_job(&project, &pictures, &settings, &render, uri, name));
        });
    }

    fn studio_debug(&mut self, s: &mut Studio, ui: &mut egui::Ui, host: &Host) {
        ui.checkbox(&mut s.hit_boxes, "Show overlay boxes and frame bounds");
        let source = s.project.source_time(s.playhead);
        ui.small(format!(
            "Playhead {} -> source {}  ·  player {}",
            timecode(s.playhead, true),
            timecode(source, true),
            timecode(self.native["playback"]["position"].as_i64().unwrap_or(0), true)
        ));
        ui.small(format!("Ranges {:?}", s.project.ranges()));
        ui.small(format!(
            "Previews {} · renders {} · last {:.1} ms · tiles {}",
            s.previews.len(),
            s.renders,
            s.render_ms,
            s.tiles.len()
        ));
        let book = caption::book();
        ui.small(format!(
            "Fonts: {} families, default {}",
            book.families().len(),
            book.default_family()
        ));
        ui.small(format!("Scale {:.0} pt/s · fps {} · {}", s.scale, s.fps, s.project.name));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Copy project JSON").clicked() {
                host.copy_text(serde_json::to_string_pretty(&s.project).unwrap_or_default());
            }
            if ui.button("Copy FFmpeg command").clicked() {
                let overlays: Vec<compose::Overlay> = s
                    .project
                    .layers
                    .iter()
                    .filter(|l| l.visible)
                    .flat_map(|l| &l.clips)
                    .map(|c| compose::Overlay { path: format!("{}.png", c.id), clip: c.clone() })
                    .collect();
                let job = compose::job(
                    &s.project,
                    &overlays,
                    &compose::Settings { max_height: s.height, crf: s.crf, fps: s.fps.clone() },
                );
                host.copy_text(job.command());
            }
            if ui.button("Add sample captions").clicked() {
                s.sample_captions();
            }
            if ui.button("Clear overlays").clicked() {
                s.project.layers.clear();
                s.pick = None;
            }
            if ui.button("Reset project").clicked() {
                let fresh = Project::new(&s.item.uri, &s.item.name, s.project.duration, s.project.width, s.project.height);
                s.project = Project {
                    fps: s.project.fps,
                    has_audio: s.project.has_audio,
                    ..fresh
                };
                s.pick = None;
            }
        });
        ui.separator();
        let json = serde_json::to_string_pretty(&s.project).unwrap_or_default();
        ui.add(egui::Label::new(egui::RichText::new(json.chars().take(12_000).collect::<String>()).monospace().size(10.0)).wrap());
    }
}

/// Renders every visible overlay at output size and returns the export command.
fn studio_job(
    project: &Project,
    pictures: &HashMap<String, Arc<RgbaImage>>,
    settings: &compose::Settings,
    render: &std::path::Path,
    uri: String,
    name: String,
) -> Result<Value, String> {
    let (w, h) = project.output_size(settings.max_height);
    let mut overlays = vec![];
    for layer in project.layers.iter().filter(|l| l.visible) {
        for clip in layer.clips.iter().filter(|c| c.start < project.length()) {
            let rendered = match &clip.body {
                Body::Text(style) => caption::render_text(style, w as f32, h as f32),
                Body::Picture(picture) => {
                    let source = match pictures.get(&picture.path) {
                        Some(image) => image.clone(),
                        None => Arc::new(
                            image::open(&picture.path)
                                .map_err(|_| format!("Photo overlay {} is unavailable", picture.name))?
                                .to_rgba8(),
                        ),
                    };
                    caption::render_picture(picture, &source, w as f32)
                }
            };
            if rendered.image.width() <= 2 {
                continue;
            }
            let image = if clip.rotation.abs() > 0.01 || clip.opacity < 0.999 {
                caption::transform(&rendered.image, clip.rotation, clip.opacity)
            } else {
                rendered.image
            };
            let path = render.join(format!("{}.png", clip.id));
            image.save(&path).map_err(|e| format!("Could not write an overlay: {e}"))?;
            overlays.push(compose::Overlay {
                path: path.to_string_lossy().into_owned(),
                clip: clip.clone(),
            });
        }
    }
    let job = compose::job(project, &overlays, settings);
    Ok(json!({"op":"studio_export","uri":uri,"name":name,"args":job.args,"duration":job.duration}))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Vec2 = vec2(411.0, 891.0);

    struct Rig {
        ctx: egui::Context,
        host: Host,
        time: f64,
        gallery: Gallery,
    }

    impl Rig {
        fn new() -> Self {
            let mut gallery = Gallery::new(&CreateContext {
                width_px: 1440,
                height_px: 3120,
                pixels_per_point: 3.5,
            });
            gallery.selected = Some(MediaItem {
                uri: "content://media/external/video/media/1".into(),
                name: "Clip.mp4".into(),
                kind: "video".into(),
                width: 1920,
                height: 1080,
                duration: 10_000,
                ..MediaItem::default()
            });
            gallery.open_studio();
            let ctx = egui::Context::default();
            gallery.theme(&ctx);
            let mut rig = Self {
                ctx,
                host: Host::new(),
                time: 0.0,
                gallery,
            };
            rig.frame(vec![]);
            rig
        }
        fn studio(&mut self) -> &mut Studio {
            self.gallery.studio.as_mut().expect("editor open")
        }
        fn frame(&mut self, events: Vec<egui::Event>) {
            self.time += 1.0 / 60.0;
            let gallery = &mut self.gallery;
            let host = &self.host;
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| gallery.studio_ui(ui, host));
                },
            );
            output.textures_delta.clear();
        }
        fn idle(&mut self, seconds: f64) {
            for _ in 0..(seconds * 60.0) as usize {
                self.frame(vec![]);
            }
        }
        /// Presses at `from`, moves to `to` in steps, then releases.
        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.frame(touch(egui::TouchPhase::Start, from));
            for step in 1..=6 {
                self.frame(touch(egui::TouchPhase::Move, from.lerp(to, step as f32 / 6.0)));
            }
            self.frame(touch(egui::TouchPhase::End, to));
            self.frame(vec![]);
        }
        fn tap(&mut self, at: Pos2) {
            self.frame(touch(egui::TouchPhase::Start, at));
            self.frame(touch(egui::TouchPhase::End, at));
            self.frame(vec![]);
        }
        fn caption(&mut self, start: i64, end: i64, x: f32, y: f32) -> u64 {
            let id = self.studio().project.add_clip(Clip {
                start,
                end,
                x,
                y,
                enter: Motion::None,
                exit: Motion::None,
                body: Body::Text(TextStyle {
                    text: "Hello Luma".into(),
                    ..TextStyle::default()
                }),
                ..Clip::default()
            });
            self.frame(vec![]);
            self.frame(vec![]);
            id
        }
    }

    fn touch(phase: egui::TouchPhase, pos: Pos2) -> Vec<egui::Event> {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let mut events = vec![egui::Event::Touch {
            device_id: egui::TouchDeviceId(0),
            id: egui::TouchId(3),
            phase,
            pos,
            force: None,
        }];
        match phase {
            egui::TouchPhase::Start => events.extend([egui::Event::PointerMoved(pos), button(true)]),
            egui::TouchPhase::Move => events.push(egui::Event::PointerMoved(pos)),
            _ => events.extend([button(false), egui::Event::PointerGone]),
        }
        events
    }

    #[test]
    fn dragging_a_caption_moves_it_snaps_to_center_and_undoes() {
        let mut rig = Rig::new();
        let id = rig.caption(0, 3000, 0.3, 0.75);
        rig.idle(0.6);
        let hit = *rig.studio().hits.iter().find(|h| h.id == id).expect("caption on stage");
        let frame = rig.studio().frame;
        rig.drag(hit.center, hit.center + vec2(40.0, -30.0));
        let clip = rig.studio().project.clip(id).unwrap().clone();
        assert!((clip.x - (0.3 + 40.0 / frame.width())).abs() < 0.01, "x {}", clip.x);
        assert!((clip.y - (0.75 - 30.0 / frame.height())).abs() < 0.01, "y {}", clip.y);
        assert_eq!(rig.studio().pick, Some(Pick::Clip(id)));
        rig.idle(0.6);
        let hit = *rig.studio().hits.iter().find(|h| h.id == id).unwrap();
        let target = pos2(frame.center().x + 4.0, hit.center.y);
        rig.drag(hit.center, target);
        assert_eq!(rig.studio().project.clip(id).unwrap().x, 0.5, "Snapped to the center line");
        rig.idle(0.6);
        rig.studio().undo();
        assert!((rig.studio().project.clip(id).unwrap().x - clip.x).abs() < 1e-6);
    }

    #[test]
    fn timeline_handle_lengthens_the_selected_caption() {
        let mut rig = Rig::new();
        let id = rig.caption(1000, 3000, 0.5, 0.8);
        rig.studio().pick = Some(Pick::Clip(id));
        rig.frame(vec![]);
        let studio = rig.studio();
        let lane = studio.lanes.iter().find(|(l, _)| *l == Lane::Layer(0)).unwrap().1;
        let x_end = studio.track.center().x + (3000 - studio.playhead) as f32 * studio.scale / 1000.0;
        let scale = studio.scale;
        let from = pos2(x_end + 5.0, lane.center().y);
        rig.drag(from, from + vec2(30.0, 0.0));
        let end = rig.studio().project.clip(id).unwrap().end;
        let expected = 3000 + (30.0 * 1000.0 / scale) as i64;
        assert!((end - expected).abs() <= (12.0 * 1000.0 / scale) as i64 + 1, "end {end}, expected {expected}");
        assert!(end > 3000);
    }

    #[test]
    fn drags_end_at_the_touch_lift_point() {
        let mut rig = Rig::new();
        let studio = rig.studio();
        let (scale, y) = (studio.scale, studio.lanes.last().unwrap().1.center().y);
        let from = pos2(studio.track.center().x + 60.0, y);
        let to = from - vec2(90.0, 0.0);
        let last_move = from - vec2(30.0, 0.0);
        rig.frame(touch(egui::TouchPhase::Start, from));
        rig.frame(touch(egui::TouchPhase::Move, last_move));
        // egui-winit's lift: the button at the last move, the touch at the lift point.
        rig.frame(vec![
            egui::Event::Touch {
                device_id: egui::TouchDeviceId(0),
                id: egui::TouchId(3),
                phase: egui::TouchPhase::End,
                pos: to,
                force: None,
            },
            egui::Event::PointerButton {
                pos: last_move,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            },
            egui::Event::PointerGone,
        ]);
        rig.frame(vec![]);
        let expected = (90.0 * 1000.0 / scale) as i64;
        assert!((rig.studio().playhead - expected).abs() <= 2, "{} vs {expected}", rig.studio().playhead);
    }

    #[test]
    fn eye_toggle_hides_a_layer_from_the_stage() {
        let mut rig = Rig::new();
        rig.caption(0, 3000, 0.5, 0.5);
        assert_eq!(rig.studio().hits.len(), 1);
        let lane = rig.studio().lanes.iter().find(|(l, _)| *l == Lane::Layer(0)).unwrap().1;
        rig.tap(lane.left_center() + vec2(15.0, 0.0));
        assert!(!rig.studio().project.layers[0].visible);
        rig.frame(vec![]);
        assert!(rig.studio().hits.is_empty(), "Hidden layer still drawn");
    }

    #[test]
    fn player_position_maps_through_removed_segments() {
        let mut rig = Rig::new();
        let studio = rig.studio();
        studio.project.split(3000);
        studio.project.split(5000);
        studio.project.remove_segment(1);
        let uri = studio.item.uri.clone();
        rig.gallery.native = json!({"playback":{"uri":uri,"position":7500,"playing":false}});
        rig.frame(vec![]);
        assert_eq!(rig.studio().playhead, 5500);
        assert_eq!(rig.studio().sent.as_deref(), Some(&[[0, 3000], [5000, 10_000]][..]));
    }

    #[test]
    fn closing_saves_and_reopening_restores_the_project() {
        let dir = std::env::temp_dir().join(format!("luma-studio-{}", std::process::id()));
        let mut rig = Rig::new();
        rig.gallery.preferences = Some(dir.join("luma-favorites.json"));
        rig.gallery.studio = None;
        rig.gallery.open_studio();
        rig.frame(vec![]);
        let id = rig.caption(0, 2000, 0.5, 0.5);
        rig.gallery.back();
        assert!(rig.gallery.studio.is_none());
        rig.gallery.open_studio();
        assert!(rig.studio().project.clip(id).is_some(), "Project restored");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
