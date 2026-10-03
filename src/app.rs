mod manage;

use crate::{ACCENT, BG, LINE, MUTED, PANEL, TEXT, TRIM, bridge, icons, model::*};
use egui::{
    self, Align, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2,
    vec2,
};
use egui_mobile::{CreateContext, EguiApp, Host};
use manage::Dialog;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, PartialEq)]
enum View {
    Albums,
    All,
    Favorites,
    Trash,
}

const ACTION_BAR: f32 = 56.0;

pub struct Gallery {
    items: Vec<MediaItem>,
    trash: Vec<MediaItem>,
    selection: Selection,
    dialog: Option<Dialog>,
    menu_open: bool,
    drag_origin: Option<Pos2>,
    buzz: bool,
    last_change: Value,
    view: View,
    album: Option<String>,
    filter: String,
    query: String,
    search: bool,
    oldest: bool,
    favorites: HashSet<String>,
    preferences: Option<std::path::PathBuf>,
    selected: Option<MediaItem>,
    center_rail: bool,
    rail_width: f32,
    textures: HashMap<String, (egui::TextureHandle, u64)>,
    requested: HashMap<String, Instant>,
    frame: u64,
    native: Value,
    metadata: Value,
    diagnostics: Value,
    timeline: Timeline,
    pending_seek: Option<(i64, Instant)>,
    trimming: bool,
    looped: bool,
    export_dialog: bool,
    exact: bool,
    keyframe: Option<i64>,
    info: bool,
    settings: bool,
    software: bool,
    toast: Option<(String, Instant, Vec<MediaItem>)>,
    last_rect: Option<(i32, i32, i32, i32, bool)>,
    last_preview: Instant,
    proxy: bool,
    #[cfg(target_os = "android")]
    gpu: Option<egui_mobile::video::GpuVideoSurface>,
    #[cfg(target_os = "android")]
    gpu_size: (u32, u32),
}

impl Gallery {
    pub fn new(_: &CreateContext) -> Self {
        Self {
            items: vec![],
            trash: vec![],
            selection: Selection::default(),
            dialog: None,
            menu_open: false,
            drag_origin: None,
            buzz: false,
            last_change: json!({}),
            view: View::Albums,
            album: None,
            filter: String::new(),
            query: String::new(),
            search: false,
            oldest: false,
            favorites: HashSet::new(),
            preferences: None,
            selected: None,
            center_rail: true,
            rail_width: 0.0,
            textures: HashMap::new(),
            requested: HashMap::new(),
            frame: 0,
            native: json!({}),
            metadata: json!({}),
            diagnostics: json!({}),
            timeline: Timeline::default(),
            pending_seek: None,
            trimming: false,
            looped: false,
            export_dialog: false,
            exact: false,
            keyframe: None,
            info: false,
            settings: false,
            software: false,
            toast: None,
            last_rect: None,
            last_preview: Instant::now() - Duration::from_secs(1),
            proxy: false,
            #[cfg(target_os = "android")]
            gpu: None,
            #[cfg(target_os = "android")]
            gpu_size: (0, 0),
        }
    }

    fn notice(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now(), vec![]));
    }
    fn busy(&self) -> bool {
        self.native["managing"].as_bool().unwrap_or(false)
    }
    fn trash_supported(&self) -> bool {
        self.native["sdk"].as_i64().unwrap_or(0) >= 30
    }
    fn save(&self) {
        if let Some(path) = &self.preferences {
            if let Ok(bytes) = serde_json::to_vec(&self.favorites) {
                let _ = std::fs::write(path, bytes);
            }
        }
    }

    fn pump(&mut self, ctx: &egui::Context) {
        self.native = bridge::poll();
        let events = self.native["events"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for event in events {
            match event["type"].as_str().unwrap_or_default() {
                "library" => {
                    if let Ok(items) = serde_json::from_value(event["items"].clone()) {
                        self.items = items;
                    }
                    if let Ok(trash) = serde_json::from_value(event["trash"].clone()) {
                        self.trash = trash;
                    }
                    if self
                        .album
                        .as_ref()
                        .is_some_and(|album| !self.items.iter().any(|i| &i.album == album))
                    {
                        self.album = None;
                        self.selection.clear();
                    }
                    self.refresh_selected();
                }
                "thumb" => {
                    let key = event["key"].as_str().unwrap_or_default().to_owned();
                    if let Some(image) = event["path"]
                        .as_str()
                        .and_then(|p| std::fs::read(p).ok())
                        .and_then(|b| image::load_from_memory(&b).ok())
                    {
                        let rgba = image.to_rgba8();
                        let pixels = egui::ColorImage::from_rgba_unmultiplied(
                            [rgba.width() as usize, rgba.height() as usize],
                            rgba.as_raw(),
                        );
                        self.textures.insert(
                            key.clone(),
                            (
                                ctx.load_texture(&key, pixels, egui::TextureOptions::LINEAR),
                                self.frame,
                            ),
                        );
                    }
                    self.requested.remove(&key);
                }
                "thumb_failed" => {}
                "metadata" => {
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|i| Some(i.uri.as_str()) == event["uri"].as_str())
                    {
                        self.metadata = event["probe"].clone();
                        if let Some(streams) = self.metadata["streams"].as_array() {
                            if let Some(video) = streams.iter().find(|s| s["codec_type"] == "video")
                            {
                                if let Some((n, d)) = video["avg_frame_rate"]
                                    .as_str()
                                    .and_then(|s| s.split_once('/'))
                                {
                                    if let (Ok(n), Ok(d)) = (n.parse::<f64>(), d.parse::<f64>()) {
                                        if d > 0.0 && n > 0.0 {
                                            self.timeline.fps = n / d;
                                        }
                                    }
                                }
                                if let Some(item) = self.selected.as_mut() {
                                    item.width =
                                        video["width"].as_u64().unwrap_or(item.width as u64) as u32;
                                    item.height =
                                        video["height"].as_u64().unwrap_or(item.height as u64)
                                            as u32;
                                }
                            }
                        }
                        if let Some(ms) = self.metadata["format"]["duration"]
                            .as_str()
                            .and_then(|s| s.parse::<f64>().ok())
                            .map(|s| (s * 1000.0) as i64)
                        {
                            if let Some(item) = self.selected.as_mut() {
                                item.duration = ms;
                            }
                            if self.timeline.end == 0 {
                                self.timeline.end = ms;
                            }
                        }
                    }
                }
                "keyframe" => {
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|i| Some(i.uri.as_str()) == event["uri"].as_str())
                        && event["requested"].as_i64() == Some(self.timeline.start)
                    {
                        self.keyframe = event["time"].as_i64();
                    }
                }
                "photo" => {
                    if let Some(item) = self
                        .selected
                        .as_mut()
                        .filter(|i| Some(i.uri.as_str()) == event["uri"].as_str())
                    {
                        item.width = event["width"].as_u64().unwrap_or(0) as u32;
                        item.height = event["height"].as_u64().unwrap_or(0) as u32;
                    }
                }
                "navigate" => {
                    if !self.info
                        && !self.settings
                        && !self.export_dialog
                        && self.dialog.is_none()
                        && self.selected.as_ref().is_some_and(|i| {
                            !i.is_video() && Some(i.uri.as_str()) == event["uri"].as_str()
                        })
                    {
                        match event["delta"].as_i64() {
                            Some(-1) => self.neighbor(-1),
                            Some(1) => self.neighbor(1),
                            _ => {}
                        }
                    }
                }
                "error" => self.notice(event["message"].as_str().unwrap_or("Operation failed")),
                "exported" => self.notice("Clip saved to Movies/Luma"),
                "proxy" => {
                    self.proxy = true;
                    self.notice("Playback proxy ready. Exports use the original.");
                }
                "diagnostics" => self.diagnostics = event,
                "managed" => self.managed(&event),
                _ => {}
            }
        }
        if self.timeline.dragging.is_none()
            && self
                .selected
                .as_ref()
                .is_some_and(|item| self.native["playback"]["uri"] == item.uri)
        {
            let position = self.native["playback"]["position"]
                .as_i64()
                .unwrap_or(self.timeline.position);
            if self.pending_seek.is_some_and(|(target, started)| {
                (position - target).abs() <= (self.timeline.frame_ms() / 2).max(1)
                    || started.elapsed() > Duration::from_secs(2)
            }) {
                self.pending_seek = None;
            }
            if self.pending_seek.is_none() {
                self.timeline.position = position;
            }
        }
        if self.timeline.end == 0 {
            self.timeline.end = self.duration();
        }
        if self.textures.len() > 180 {
            let mut oldest: Vec<_> = self
                .textures
                .iter()
                .map(|(k, (_, f))| (k.clone(), *f))
                .collect();
            oldest.sort_by_key(|(_, f)| *f);
            for (key, _) in oldest.into_iter().take(self.textures.len() - 150) {
                self.textures.remove(&key);
            }
        }
        self.requested
            .retain(|_, started| started.elapsed() < Duration::from_secs(45));
    }

    fn duration(&self) -> i64 {
        self.selected
            .as_ref()
            .map(|i| i.duration)
            .unwrap_or(0)
            .max(self.native["playback"]["duration"].as_i64().unwrap_or(0))
    }
    fn playing(&self) -> bool {
        self.native["playback"]["playing"]
            .as_bool()
            .unwrap_or(false)
    }
    fn thumb(&mut self, item: &MediaItem, at: i64) -> Option<egui::TextureHandle> {
        let key = item.thumb_key(at);
        if let Some((texture, frame)) = self.textures.get_mut(&key) {
            *frame = self.frame;
            return Some(texture.clone());
        }
        if self
            .requested
            .get(&key)
            .is_none_or(|t| t.elapsed() > Duration::from_secs(45))
            && self.requested.len() < 80
        {
            self.requested.insert(key.clone(), Instant::now());
            bridge::send(json!({"op":"thumb","key":key,"item":item,"time":at,"width":384}));
        }
        None
    }
    fn cover(&mut self, ui: &egui::Ui, item: &MediaItem, rect: Rect, at: i64, tint: Color32) {
        ui.painter().rect_filled(rect, 6, PANEL);
        if let Some(texture) = self.thumb(item, at) {
            let source = texture.size_vec2();
            let factor = (rect.width() / source.x).max(rect.height() / source.y);
            let visible = rect.size() / (source * factor);
            let uv = Rect::from_center_size(pos2(0.5, 0.5), visible);
            egui::Image::from_texture(&texture)
                .uv(uv)
                .tint(tint)
                .corner_radius(6)
                .paint_at(ui, rect);
        } else {
            icons::image(&item.kind, MUTED, 24.0)
                .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(24.0)));
        }
    }

    fn open(&mut self, item: MediaItem) {
        #[cfg(target_os = "android")]
        if item.is_video() {
            if self.gpu.is_none() {
                match egui_mobile::video::GpuVideoSurface::new() {
                    Ok(surface) => self.gpu = Some(surface),
                    Err(error) => self.notice(error),
                }
            }
            self.configure_video_surface(
                if item.width > 0 { item.width } else { 1920 },
                if item.height > 0 { item.height } else { 1080 },
            );
        }
        self.timeline = Timeline {
            end: item.duration,
            ..Timeline::default()
        };
        self.pending_seek = None;
        self.trimming = false;
        self.info = false;
        self.looped = false;
        self.metadata = json!({});
        self.keyframe = None;
        self.proxy = false;
        self.last_rect = None;
        self.center_rail = true;
        bridge::send(json!({"op":"open","item":item}));
        self.selected = Some(item);
    }
    #[cfg(target_os = "android")]
    fn configure_video_surface(&mut self, width: u32, height: u32) {
        if let Some(surface) = &self.gpu {
            surface.set_buffer_size(width, height);
            if let Ok(reference) = surface.java_surface() {
                bridge::attach_surface(&reference, width, height);
            }
            self.gpu_size = (width, height);
        }
    }
    fn back(&mut self) {
        if self.export_dialog {
            self.export_dialog = false;
        } else if self.info {
            self.info = false;
        } else if self.settings {
            self.settings = false;
        } else if self.dialog.take().is_some() {
        } else if self.selected.is_some() {
            self.close_viewer();
        } else if self.selection.active {
            self.selection.clear();
        } else if self.album.take().is_none() {
            self.view = View::Albums;
            self.query.clear();
        }
    }
    fn close_viewer(&mut self) {
        if self.selected.take().is_some() {
            bridge::send(json!({"op":"close"}));
            self.last_rect = None;
        }
    }
    fn eligible(&self, item: &MediaItem) -> bool {
        (self.filter.is_empty() || item.kind == self.filter)
            && self.album.as_ref().is_none_or(|a| a == &item.album)
            && (self.view != View::Favorites || self.favorites.contains(&item.uri))
            && (self.query.is_empty()
                || item
                    .name
                    .to_lowercase()
                    .contains(&self.query.to_lowercase())
                || item
                    .album
                    .to_lowercase()
                    .contains(&self.query.to_lowercase()))
    }
    fn visible_items(&self) -> Vec<MediaItem> {
        let source = if self.view == View::Trash {
            &self.trash
        } else {
            &self.items
        };
        let mut list: Vec<_> = source
            .iter()
            .filter(|i| self.eligible(i))
            .cloned()
            .collect();
        if self.oldest {
            list.reverse();
        }
        list
    }
    fn neighbor(&mut self, delta: isize) {
        let list = self.visible_items();
        let current = self
            .selected
            .as_ref()
            .and_then(|selected| list.iter().position(|i| i.uri == selected.uri));
        if let Some(index) = current
            .and_then(|i| i.checked_add_signed(delta))
            .filter(|i| *i < list.len())
        {
            self.open(list[index].clone());
        }
    }

    fn library(&mut self, ui: &mut egui::Ui) {
        // Long-press selection threshold; the viewer keeps egui's default for its drags.
        ui.ctx()
            .options_mut(|options| options.input_options.max_click_duration = 0.5);
        if self.selection.dragging() && !ui.input(|i| i.pointer.primary_down()) {
            self.selection.end_drag();
        }
        let state = (
            self.view,
            self.album.clone(),
            self.filter.clone(),
            self.query.clone(),
            self.oldest,
        );
        let mut list = self.visible_items();
        if self.selection.active {
            self.selection_header(ui, &list);
        } else {
            ui.horizontal(|ui| {
                if self.album.is_some() && icons::button(ui, "back", "Albums", false).clicked() {
                    self.album = None;
                }
                ui.add_sized(
                    [(ui.available_width() - 184.0).max(40.0), 40.0],
                    egui::Label::new(
                        egui::RichText::new(
                            self.album
                                .as_deref()
                                .and_then(|s| s.rsplit('/').next())
                                .unwrap_or("Luma Gallery"),
                        )
                        .size(20.0)
                        .strong(),
                    )
                    .halign(Align::Min)
                    .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if icons::button(ui, "settings", "Settings", false).clicked() {
                        self.settings = true;
                        bridge::send(json!({"op":"diagnostics"}));
                    }
                    if icons::button(ui, "folder", "Add folder", false).clicked() {
                        bridge::send(json!({"op":"folder"}));
                    }
                    if icons::button(ui, "refresh", "Refresh library", false).clicked() {
                        self.requested.clear();
                        bridge::send(json!({"op":"scan"}));
                    }
                    if icons::button(ui, "select", "Select", false).clicked() {
                        self.selection.active = true;
                    }
                });
            });
        }
        ui.add_space(2.0);
        if self.album.is_none() {
            ui.horizontal(|ui| {
                for (view, name, label) in [
                    (View::Albums, "albums", "Albums"),
                    (View::All, "grid", "All media"),
                    (View::Favorites, "star", "Favorites"),
                    (View::Trash, "delete", "Trash"),
                ] {
                    if view == View::Trash && !self.trash_supported() {
                        continue;
                    }
                    let button = egui::Button::image_and_text(
                        icons::image(name, if self.view == view { ACCENT } else { MUTED }, 18.0),
                        label,
                    )
                    .selected(self.view == view)
                    .min_size(vec2(0.0, 36.0));
                    let glow = ui.painter().add(egui::Shape::Noop);
                    let response = ui.add(button);
                    if self.view == view {
                        ui.painter().set(glow, icons::selection_glow(response.rect));
                        ui.painter().rect_stroke(
                            response.rect,
                            5,
                            Stroke::new(1.0, ACCENT),
                            StrokeKind::Inside,
                        );
                    }
                    if response.clicked() && self.view != view {
                        self.view = view;
                        self.selection.clear();
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} items", list.len()))
                .size(12.0)
                .color(MUTED),
            );
            if self.native["scanning"].as_bool().unwrap_or(false) {
                ui.spinner();
            }
            if self.view == View::Trash
                && !self.trash.is_empty()
                && !self.selection.active
                && ui
                    .add_enabled(
                        !self.busy(),
                        egui::Button::image_and_text(icons::image("delete", ACCENT, 16.0), "Empty")
                            .min_size(vec2(0.0, 32.0)),
                    )
                    .clicked()
            {
                self.empty_trash();
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, "search", "Search", self.search).clicked() {
                    self.search = !self.search;
                    if !self.search {
                        self.query.clear();
                    }
                }
                if icons::button(ui, "sort", "Reverse date order", self.oldest).clicked() {
                    self.oldest = !self.oldest;
                }
                for (kind, label) in [
                    ("raw", "RAW photos"),
                    ("photo", "Photos"),
                    ("video", "Videos"),
                ] {
                    if icons::button(ui, kind, label, self.filter == kind).clicked() {
                        if self.filter == kind {
                            self.filter.clear();
                        } else {
                            self.filter = kind.into();
                        }
                    }
                }
            });
        });
        if self.search {
            ui.add_sized(
                [ui.available_width(), 36.0],
                egui::TextEdit::singleline(&mut self.query).hint_text("Search files or albums"),
            );
        }
        if self.native["access"] == "selected" {
            ui.horizontal_wrapped(|ui| {
                ui.small("Selected photos only");
                if ui.small_button("Manage access").clicked() {
                    bridge::send(json!({"op":"permission"}));
                }
            });
        }
        ui.add_space(4.0);
        if state
            != (
                self.view,
                self.album.clone(),
                self.filter.clone(),
                self.query.clone(),
                self.oldest,
            )
        {
            list = self.visible_items();
        }
        if list.is_empty() {
            ui.add_space(36.0);
            ui.vertical_centered(|ui| {
                if self.view == View::Trash {
                    ui.add(icons::image("delete", MUTED, 44.0));
                    ui.add_space(12.0);
                    ui.label(egui::RichText::new("Trash is empty").size(20.0));
                    ui.small("Deleted items stay here for 30 days");
                    return;
                }
                ui.add(icons::image("albums", MUTED, 44.0));
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(if self.items.is_empty() {
                        "Your library"
                    } else {
                        "No matching media"
                    })
                    .size(20.0),
                );
                ui.add_space(16.0);
                if ui
                    .add(
                        egui::Button::image_and_text(
                            icons::image("photo", ACCENT, 20.0),
                            "Choose photos and videos",
                        )
                        .min_size(vec2(240.0, 44.0)),
                    )
                    .clicked()
                {
                    bridge::send(json!({"op":"permission"}));
                }
                if ui
                    .add(
                        egui::Button::image_and_text(
                            icons::image("folder", TEXT, 20.0),
                            "Add a folder",
                        )
                        .min_size(vec2(240.0, 44.0)),
                    )
                    .clicked()
                {
                    bridge::send(json!({"op":"folder"}));
                }
            });
            return;
        }
        let footer = if self.selection.active {
            ACTION_BAR + ui.spacing().item_spacing.y
        } else {
            0.0
        };
        let grid_height = (ui.available_height() - footer).max(48.0);
        let pointer = ui.input(|i| i.pointer.latest_pos());
        if self.view == View::Albums && self.album.is_none() {
            let mut groups: BTreeMap<String, Vec<MediaItem>> = BTreeMap::new();
            for item in list {
                groups.entry(item.album.clone()).or_default().push(item);
            }
            let albums: Vec<_> = groups.into_iter().collect();
            let keys: Vec<String> = albums.iter().map(|(album, _)| album.clone()).collect();
            let cols = ((ui.available_width() / 180.0).floor() as usize).clamp(2, 5);
            let side = (ui.available_width() - 12.0 * (cols - 1) as f32) / cols as f32;
            // Register the drag target before Android delivers its first touch.
            egui::ScrollArea::vertical()
                .id_salt("albums")
                .scroll_source(egui::scroll_area::ScrollSource::ALL)
                .max_height(grid_height)
                .show_rows(
                    ui,
                    // show_rows adds item_spacing itself; this must match the drawn row.
                    side,
                    albums.len().div_ceil(cols),
                    |ui, rows| {
                        self.drag_scroll(ui, pointer);
                        for row in rows {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                for (index, (album, items)) in
                                    albums.iter().enumerate().skip(row * cols).take(cols)
                                {
                                    let (rect, response) =
                                        ui.allocate_exact_size(vec2(side, side), Sense::click());
                                    let picked =
                                        self.selection.active && self.selection.contains(album);
                                    ui.painter().rect(
                                        rect,
                                        8,
                                        PANEL,
                                        Stroke::new(
                                            if picked { 2.0 } else { 1.0 },
                                            if picked || response.hovered() {
                                                ACCENT
                                            } else {
                                                LINE
                                            },
                                        ),
                                        StrokeKind::Inside,
                                    );
                                    let cover_side = (side - 72.0).max(48.0);
                                    let front = Rect::from_min_size(
                                        rect.min + vec2(16.0, 22.0),
                                        Vec2::splat(cover_side),
                                    );
                                    for depth in (0..items.len().min(3)).rev() {
                                        let card = front.translate(vec2(
                                            depth as f32 * 10.0,
                                            -(depth as f32) * 5.0,
                                        ));
                                        ui.painter().rect_filled(card.expand(2.0), 6, BG);
                                        self.cover(
                                            ui,
                                            &items[depth],
                                            card,
                                            0,
                                            if depth == 0 {
                                                TEXT
                                            } else {
                                                Color32::from_gray(150)
                                            },
                                        );
                                    }
                                    text_at(
                                        ui,
                                        rect.min + vec2(12.0, side - 41.0),
                                        album.rsplit('/').next().unwrap_or(album),
                                        15.0,
                                        TEXT,
                                        side - 24.0,
                                    );
                                    text_at(
                                        ui,
                                        rect.min + vec2(12.0, side - 22.0),
                                        &format!("{} items", items.len()),
                                        12.0,
                                        MUTED,
                                        side - 24.0,
                                    );
                                    if self.selection.active {
                                        pick_badge(ui, rect, picked);
                                    }
                                    if self.pick_input(&response, rect, &keys, index, pointer) {
                                        self.album = Some(album.clone());
                                    }
                                }
                            });
                        }
                    },
                );
            if self.selection.active {
                self.album_actions(ui, &albums);
            }
        } else {
            let keys: Vec<String> = list.iter().map(|item| item.uri.clone()).collect();
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let cols = ((ui.available_width() / 118.0).floor() as usize).clamp(3, 8);
            let side = (ui.available_width() - 5.0 * (cols - 1) as f32) / cols as f32;
            egui::ScrollArea::vertical()
                .id_salt(("media", &self.album))
                .scroll_source(egui::scroll_area::ScrollSource::ALL)
                .max_height(grid_height)
                .show_rows(ui, side, list.len().div_ceil(cols), |ui, rows| {
                    self.drag_scroll(ui, pointer);
                    for row in rows {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            for (index, item) in list.iter().enumerate().skip(row * cols).take(cols)
                            {
                                let (rect, response) =
                                    ui.allocate_exact_size(Vec2::splat(side), Sense::click());
                                let picked =
                                    self.selection.active && self.selection.contains(&item.uri);
                                let inset = ui.ctx().animate_bool_with_time(
                                    egui::Id::new(("picked", &item.uri)),
                                    picked,
                                    0.12,
                                ) * side
                                    * 0.07;
                                let tile = rect.shrink(inset);
                                self.cover(ui, item, tile, 0, Color32::WHITE);
                                let strip = Rect::from_min_max(
                                    pos2(tile.left(), tile.bottom() - 24.0),
                                    tile.max,
                                );
                                ui.painter()
                                    .rect_filled(strip, 0, Color32::from_black_alpha(190));
                                let label = if item.is_trashed() {
                                    days_left(item.expires, now)
                                } else if item.is_video() {
                                    timecode(item.duration, false)
                                } else if item.kind == "raw" {
                                    "RAW".into()
                                } else {
                                    String::new()
                                };
                                text_at(
                                    ui,
                                    strip.min + vec2(5.0, 4.0),
                                    &label,
                                    11.0,
                                    TEXT,
                                    tile.width() - 26.0,
                                );
                                icons::image(&item.kind, TEXT, 14.0).paint_at(
                                    ui,
                                    Rect::from_min_size(
                                        strip.right_top() + vec2(-21.0, 5.0),
                                        Vec2::splat(14.0),
                                    ),
                                );
                                if self.favorites.contains(&item.uri) {
                                    icons::image("star", TRIM, 16.0).paint_at(
                                        ui,
                                        Rect::from_min_size(
                                            tile.min + vec2(6.0, 6.0),
                                            Vec2::splat(16.0),
                                        ),
                                    );
                                }
                                if self.selection.active {
                                    pick_badge(ui, tile, picked);
                                }
                                if picked {
                                    ui.painter().rect_stroke(
                                        tile,
                                        6,
                                        Stroke::new(2.0, ACCENT),
                                        StrokeKind::Inside,
                                    );
                                }
                                if self.pick_input(&response, rect, &keys, index, pointer) {
                                    self.open(item.clone());
                                }
                            }
                        });
                    }
                });
            if self.selection.active {
                self.media_actions(ui, &list);
            }
        }
    }

    /// A long press starts a drag selection and taps toggle while selecting; true for a plain tap.
    fn pick_input(
        &mut self,
        response: &egui::Response,
        rect: Rect,
        keys: &[String],
        index: usize,
        pointer: Option<Pos2>,
    ) -> bool {
        if response.long_touched() {
            self.selection.begin_drag(keys, index);
            self.drag_origin = pointer;
            self.buzz = true;
        } else if self.selection.dragging() {
            if pointer.is_some_and(|p| rect.contains(p)) {
                self.selection.drag_to(keys, index);
            }
        } else if response.clicked() {
            if !self.selection.active {
                return true;
            }
            self.selection.toggle(&keys[index]);
        }
        false
    }

    /// Scrolls the grid while a drag selection holds the pointer near its top or bottom edge.
    fn drag_scroll(&self, ui: &egui::Ui, pointer: Option<Pos2>) {
        let Some(pointer) = pointer.filter(|p| {
            self.selection.dragging()
                && self.drag_origin.is_some_and(|origin| origin.distance(*p) > 24.0)
        }) else {
            return;
        };
        let view = ui.clip_rect();
        let edge = 64.0;
        let depth = if pointer.y > view.bottom() - edge {
            view.bottom() - edge - pointer.y
        } else if pointer.y < view.top() + edge {
            view.top() + edge - pointer.y
        } else {
            return;
        };
        ui.scroll_with_delta_animation(
            vec2(0.0, depth.clamp(-edge, edge) * 0.35),
            egui::style::ScrollAnimation::none(),
        );
        ui.ctx().request_repaint();
    }

    fn viewer(&mut self, ui: &mut egui::Ui) {
        let Some(item) = self.selected.clone() else {
            return;
        };
        ui.ctx().options_mut(|options| {
            options.input_options.max_click_duration =
                egui::InputOptions::default().max_click_duration;
        });
        ui.horizontal(|ui| {
            if icons::button(ui, "back", "Back to library", false).clicked() {
                self.back();
            }
            // Width minus four 40-point buttons and their gaps.
            let title_width = (ui.available_width() - 180.0).max(40.0);
            // Long names shrink to at most 11 points before truncating.
            let natural = ui.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(item.name.clone(), FontId::proportional(15.0), TEXT)
                    .size()
                    .x
            });
            let size = (15.0 * title_width / natural.max(1.0)).clamp(11.0, 15.0);
            ui.add_sized(
                [title_width, 32.0],
                egui::Label::new(egui::RichText::new(&item.name).size(size).strong()).truncate(),
            );
            let trashed = item.is_trashed();
            let more = icons::button(ui, "more", "More actions", false);
            self.menu_open = egui::Popup::menu(&more)
                .show(|ui| {
                    ui.set_min_width(200.0);
                    let action = |ui: &mut egui::Ui, icon: &str, label: &str| {
                        ui.add(
                            egui::Button::image_and_text(icons::image(icon, TEXT, 18.0), label)
                                .min_size(vec2(200.0, 42.0)),
                        )
                        .clicked()
                    };
                    if trashed {
                        if action(ui, "restore", "Restore") {
                            self.send_manage("restore", std::slice::from_ref(&item), json!({}));
                        }
                        if action(ui, "delete", "Delete forever") {
                            self.delete(vec![item.clone()]);
                        }
                    } else {
                        if action(ui, "delete", "Delete") {
                            self.delete(vec![item.clone()]);
                        }
                        if action(ui, "edit", "Rename") {
                            self.dialog = Some(Dialog::rename(vec![item.clone()]));
                        }
                        if action(ui, "move", "Move to album") {
                            self.dialog = Some(Dialog::target(false, vec![item.clone()]));
                        }
                        if action(ui, "copy", "Copy to album") {
                            self.dialog = Some(Dialog::target(true, vec![item.clone()]));
                        }
                    }
                })
                .is_some();
            if icons::button(ui, "star", "Favorite", self.favorites.contains(&item.uri)).clicked() {
                if !self.favorites.remove(&item.uri) {
                    self.favorites.insert(item.uri.clone());
                }
                self.save();
            }
            if icons::button(ui, "share", "Share original", false).clicked() {
                bridge::send(json!({"op":"share","uri":item.uri}));
            }
            if icons::button(ui, "info", "File information", self.info).clicked() {
                self.info = !self.info;
            }
        });
        if self.selected.is_none() {
            return;
        }
        if ui.available_width() > ui.available_height() * 1.5 {
            if item.is_video() {
                ui.columns(2, |columns| {
                    let size = columns[0].available_size();
                    self.media_stage(&mut columns[0], &item, size);
                    self.video_controls(&mut columns[1], &item);
                });
            } else {
                let available = ui.available_size();
                let sidebar = 260.0;
                let stage_width = (available.x - sidebar - ui.spacing().item_spacing.x).max(64.0);
                ui.horizontal(|ui| {
                    self.media_stage(ui, &item, vec2(stage_width, available.y));
                    ui.allocate_ui_with_layout(
                        vec2(sidebar, available.y),
                        egui::Layout::top_down(Align::Min),
                        |ui| {
                            ui.add_space((available.y - 124.0).max(0.0));
                            self.photo_controls(ui, &item);
                            self.photo_rail(ui, &item);
                        },
                    );
                });
            }
            return;
        }
        let control_height = if item.is_video() {
            // Three 40-point control rows and the 62-point timeline, plus row gaps.
            182.0
                + ui.spacing().item_spacing.y * 4.0
                + if self.trimming {
                    40.0 + ui.spacing().item_spacing.y
                } else {
                    0.0
                }
        } else {
            126.0
        };
        let available = ui.available_size();
        self.media_stage(
            ui,
            &item,
            vec2(available.x, (available.y - control_height).max(64.0)),
        );
        if item.is_video() {
            self.video_controls(ui, &item);
        } else {
            self.photo_controls(ui, &item);
            self.photo_rail(ui, &item);
        }
    }

    fn photo_controls(&mut self, ui: &mut egui::Ui, item: &MediaItem) {
        ui.horizontal(|ui| {
            let status = self.native["playback"]["photo_status"]
                .as_str()
                .unwrap_or("Loading photo");
            let status_width = (ui.available_width() - 140.0).max(40.0);
            ui.allocate_ui_with_layout(
                vec2(status_width, 40.0),
                egui::Layout::top_down(Align::Min),
                |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(status).small()).truncate());
                    ui.label(
                        egui::RichText::new(format!("{} x {}", item.width, item.height))
                            .size(12.0)
                            .color(MUTED),
                    );
                },
            );
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, "fit", "Fit photo", false).clicked() {
                    bridge::send(json!({"op":"zoom","scale":0}));
                }
                let zoom = self.native["playback"]["zoom"].as_f64().unwrap_or(1.0);
                if ui
                    .add(
                        egui::Button::new("1:1")
                            .selected((zoom - 1.0).abs() < 0.01)
                            .min_size(vec2(40.0, 40.0)),
                    )
                    .on_hover_text("Actual pixels")
                    .clicked()
                {
                    bridge::send(json!({"op":"zoom","scale":1}));
                }
                ui.add_sized(
                    [36.0, 20.0],
                    egui::Label::new(
                        egui::RichText::new(format!("{:.0}%", zoom * 100.0))
                            .small()
                            .color(MUTED),
                    ),
                );
            });
        });
    }

    fn photo_rail(&mut self, ui: &mut egui::Ui, selected: &MediaItem) {
        let items = self.visible_items();
        let width = ui.available_width();
        let center = self.center_rail || (width - self.rail_width).abs() > 1.0;
        self.center_rail = false;
        self.rail_width = width;
        let slot = vec2(88.0, 76.0);
        let padding = ((width - slot.x) / 2.0).max(0.0);
        let mut open = None;
        egui::ScrollArea::horizontal()
            .id_salt("photo_rail")
            .max_height(slot.y)
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show_viewport(ui, |ui, viewport| {
                let (bounds, _) = ui.allocate_exact_size(
                    vec2(items.len() as f32 * slot.x + padding * 2.0, slot.y),
                    Sense::hover(),
                );
                let card_slot = |index: usize| {
                    Rect::from_min_size(
                        bounds.min + vec2(padding + index as f32 * slot.x, 0.0),
                        slot,
                    )
                };
                // Center only on selection/rotation so browsing the strip never snaps back.
                if center {
                    if let Some(index) = items.iter().position(|i| i.uri == selected.uri) {
                        ui.scroll_to_rect(card_slot(index), Some(Align::Center));
                    }
                }
                let first =
                    (((viewport.left() - padding) / slot.x).floor() as isize - 1).max(0) as usize;
                let last = (((viewport.right() - padding) / slot.x).ceil().max(0.0) as usize + 1)
                    .min(items.len());
                for (index, item) in items.iter().enumerate().take(last).skip(first) {
                    let active = item.uri == selected.uri;
                    let amount =
                        ui.ctx()
                            .animate_bool_with_time(ui.id().with(&item.uri), active, 0.14);
                    let target = card_slot(index);
                    let rect = Rect::from_center_size(
                        target.center(),
                        vec2(64.0 + amount * 16.0, 48.0 + amount * 12.0),
                    );
                    if active {
                        ui.painter().add(icons::selection_glow(rect));
                    }
                    self.cover(
                        ui,
                        item,
                        rect,
                        0,
                        Color32::from_gray(if active { 255 } else { 180 }),
                    );
                    ui.painter().rect_stroke(
                        rect,
                        6,
                        Stroke::new(
                            if active { 1.5 } else { 1.0 },
                            if active { ACCENT } else { LINE },
                        ),
                        StrokeKind::Inside,
                    );
                    let response = ui
                        .interact(target, ui.id().with(("thumb", &item.uri)), Sense::click())
                        .on_hover_text(&item.name);
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            active,
                            &item.name,
                        )
                    });
                    if response.clicked() && !active {
                        open = Some(item.clone());
                    }
                }
            });
        if let Some(item) = open {
            self.open(item);
        }
    }

    fn media_stage(&mut self, ui: &mut egui::Ui, item: &MediaItem, size: Vec2) {
        let (stage, response) = ui.allocate_exact_size(size, Sense::click());
        ui.painter().rect_filled(stage, 0, Color32::BLACK);
        #[cfg(target_os = "android")]
        if item.is_video() && self.native["playback"]["uri"] == item.uri {
            let width = self.native["playback"]["width"].as_u64().unwrap_or(0) as u32;
            let height = self.native["playback"]["height"].as_u64().unwrap_or(0) as u32;
            if width > 0 && height > 0 && self.gpu_size != (width, height) {
                self.configure_video_surface(width, height);
            }
        }
        #[cfg(target_os = "android")]
        if item.is_video()
            && let Some(surface) = &self.gpu
        {
            let size = vec2(
                if item.width > 0 {
                    item.width as f32
                } else {
                    1920.0
                },
                if item.height > 0 {
                    item.height as f32
                } else {
                    1080.0
                },
            );
            let fit = (stage.width() / size.x).min(stage.height() / size.y);
            surface.paint(ui, Rect::from_center_size(stage.center(), size * fit));
        }
        if item.is_video() && response.clicked() {
            bridge::send(json!({"op":"play","playing":!self.playing()}));
        }
        let ppp = ui.ctx().pixels_per_point();
        let rect = (
            (stage.left() * ppp).round() as i32,
            (stage.top() * ppp).round() as i32,
            (stage.width() * ppp).round() as i32,
            (stage.height() * ppp).round() as i32,
            !self.info
                && !self.settings
                && !self.export_dialog
                && self.dialog.is_none()
                && !self.menu_open,
        );
        if self.last_rect != Some(rect) {
            bridge::send(
                json!({"op":"rect","x":rect.0,"y":rect.1,"w":rect.2,"h":rect.3,"visible":rect.4}),
            );
            self.last_rect = Some(rect);
        }
    }

    fn video_controls(&mut self, ui: &mut egui::Ui, item: &MediaItem) {
        let duration = self.duration();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(timecode(self.timeline.position, true))
                    .size(15.0)
                    .color(TEXT),
            );
            ui.label(
                egui::RichText::new(format!("/ {}", timecode(duration, false)))
                    .size(12.0)
                    .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, "zoom_in", "Zoom timeline", false).clicked() {
                    self.timeline.zoom_at(duration, 2.0);
                }
                if icons::button(ui, "zoom_out", "Zoom timeline out", false).clicked() {
                    self.timeline.zoom_at(duration, 0.5);
                }
                ui.label(
                    egui::RichText::new(format!("{:.0}x", self.timeline.zoom))
                        .color(MUTED)
                        .size(12.0),
                );
            });
        });
        self.draw_timeline(ui, item, duration);
        ui.horizontal(|ui| {
            if icons::button(ui, "previous", "Previous video", false).clicked() {
                self.neighbor(-1);
            }
            if icons::button(ui, "step_back", "Previous frame", false).clicked() {
                self.seek(self.timeline.step_time(-1, duration), true);
            }
            if icons::button(
                ui,
                if self.playing() { "pause" } else { "play" },
                if self.playing() { "Pause" } else { "Play" },
                true,
            )
            .clicked()
            {
                if self.timeline.position >= duration - 50 {
                    self.seek(if self.looped { self.timeline.start } else { 0 }, false);
                }
                bridge::send(json!({"op":"play","playing":!self.playing()}));
            }
            if icons::button(ui, "step_forward", "Next frame", false).clicked() {
                self.seek(self.timeline.step_time(1, duration), true);
            }
            if icons::button(ui, "next", "Next video", false).clicked() {
                self.neighbor(1);
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let muted = self.native["playback"]["muted"].as_bool().unwrap_or(false);
                if icons::button(
                    ui,
                    if muted { "mute" } else { "volume" },
                    "Toggle audio",
                    muted,
                )
                .clicked()
                {
                    bridge::send(json!({"op":"mute","muted":!muted}));
                }
                if icons::button(ui, "loop", "Loop selection", self.looped).clicked() {
                    self.looped = !self.looped;
                    self.sync_loop();
                }
                let rate = self.native["playback"]["rate"].as_f64().unwrap_or(1.0);
                egui::ComboBox::from_id_salt("rate")
                    .width(46.0)
                    .selected_text(format!("{rate}x"))
                    .show_ui(ui, |ui| {
                        for speed in [0.25, 0.5, 1.0, 1.5, 2.0] {
                            if ui
                                .selectable_label((rate - speed).abs() < 0.01, format!("{speed}x"))
                                .clicked()
                            {
                                bridge::send(json!({"op":"rate","rate":speed}));
                            }
                        }
                    });
            });
        });
        if self.trimming {
            ui.horizontal(|ui| {
                if icons::button(ui, "in_mark", "Set in point", false).clicked() {
                    self.timeline
                        .set_mark(DragKind::In, self.timeline.position, duration);
                    self.keyframe = None;
                    self.sync_loop();
                }
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(timecode(self.timeline.start, true))
                            .size(12.0)
                            .color(TRIM),
                    );
                    ui.small("In");
                });
                if icons::button(ui, "out_mark", "Set out point", false).clicked() {
                    self.timeline
                        .set_mark(DragKind::Out, self.timeline.position, duration);
                    self.sync_loop();
                }
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(timecode(self.timeline.end, true))
                            .size(12.0)
                            .color(TRIM),
                    );
                    ui.small("Out");
                });
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(timecode(
                            self.timeline.end - self.timeline.start,
                            false,
                        ))
                        .color(MUTED)
                        .size(12.0),
                    );
                });
            });
        }
        ui.horizontal(|ui| {
            if icons::button(ui, "cut", "Trim video", self.trimming).clicked() {
                self.trimming = !self.trimming;
            }
            ui.label(
                egui::RichText::new(if self.proxy {
                    "Playback proxy".into()
                } else {
                    format!("{}p  {:.2} fps", item.height, self.timeline.fps)
                })
                .size(11.0)
                .color(MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        duration > 0 && !self.native["exporting"].as_bool().unwrap_or(false),
                        egui::Button::image_and_text(
                            icons::image("save", ACCENT, 20.0),
                            "Export clip",
                        )
                        .min_size(vec2(120.0, 36.0)),
                    )
                    .clicked()
                {
                    self.export_dialog = true;
                    self.keyframe = None;
                    bridge::send(json!({"op":"keyframe","time":self.timeline.start}));
                    bridge::send(json!({"op":"play","playing":false}));
                }
            });
        });
    }

    fn seek(&mut self, time: i64, pause: bool) {
        let time = time.clamp(0, self.duration().max(0));
        self.timeline.position = time;
        self.pending_seek = Some((time, Instant::now()));
        if pause {
            bridge::send(json!({"op":"play","playing":false}));
        }
        bridge::send(json!({"op":"seek","time":time}));
    }
    fn sync_loop(&self) {
        bridge::send(
            json!({"op":"loop","enabled":self.looped,"start":if self.trimming{self.timeline.start}else{0},"end":if self.trimming{self.timeline.end}else{self.duration()}}),
        );
    }

    fn draw_timeline(&mut self, ui: &mut egui::Ui, item: &MediaItem, duration: i64) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::click_and_drag());
        let rail = rect.shrink2(vec2(12.0, 8.0));
        let (begin, end) = self.timeline.window(duration);
        let span = (end - begin).max(1);
        for n in 0..8 {
            let tile = Rect::from_min_size(
                rail.min + vec2(rail.width() * n as f32 / 8.0, 0.0),
                vec2(rail.width() / 8.0, rail.height()),
            );
            let at = (begin + span * n / 8).max(0).min((duration - 100).max(0));
            self.cover(ui, item, tile, at, Color32::WHITE);
        }
        let x_for = |time: i64| rail.left() + rail.width() * (time - begin) as f32 / span as f32;
        if self.trimming {
            let left = x_for(self.timeline.start).clamp(rail.left(), rail.right());
            let right = x_for(self.timeline.end).clamp(rail.left(), rail.right());
            ui.painter().rect_filled(
                Rect::from_min_max(rail.min, pos2(left, rail.bottom())),
                0,
                Color32::from_black_alpha(180),
            );
            ui.painter().rect_filled(
                Rect::from_min_max(pos2(right, rail.top()), rail.max),
                0,
                Color32::from_black_alpha(180),
            );
            ui.painter().rect_stroke(
                Rect::from_min_max(pos2(left, rail.top()), pos2(right, rail.bottom())),
                3,
                Stroke::new(2.0, TRIM),
                StrokeKind::Inside,
            );
            for at in [self.timeline.start, self.timeline.end] {
                if at >= begin && at <= end {
                    let x = x_for(at);
                    let handle = Rect::from_center_size(
                        pos2(x, rail.center().y),
                        vec2(12.0, rail.height() + 8.0),
                    );
                    ui.painter().rect_filled(handle, 3, TRIM);
                    ui.painter().vline(
                        x,
                        rail.center().y - 7.0..=rail.center().y + 7.0,
                        Stroke::new(2.0, BG),
                    );
                }
            }
        }
        let playhead = x_for(self.timeline.position).clamp(rail.left(), rail.right());
        ui.painter()
            .vline(playhead, rect.top()..=rect.bottom(), Stroke::new(2.5, TEXT));
        ui.painter()
            .circle_filled(pos2(playhead, rect.top() + 3.0), 4.0, TEXT);
        let at_pos = |p: egui::Pos2| {
            begin
                + ((p.x - rail.left()) / rail.width())
                    .clamp(0.0, 1.0)
                    .mul_add(span as f32, 0.0) as i64
        };
        if response.drag_started() {
            let origin = ui
                .input(|i| i.pointer.press_origin())
                .unwrap_or(rect.center());
            let kind = if self.trimming && (origin.x - x_for(self.timeline.start)).abs() < 22.0 {
                DragKind::In
            } else if self.trimming && (origin.x - x_for(self.timeline.end)).abs() < 22.0 {
                DragKind::Out
            } else {
                DragKind::Seek
            };
            self.timeline.dragging = Some(kind);
            self.timeline.drag_origin = Some((origin, at_pos(origin)));
            self.timeline.resume_after_drag = self.playing();
            bridge::send(json!({"op":"play","playing":false}));
        }
        if let Some(kind) = self.timeline.dragging {
            if let Some(pointer) = response.interact_pointer_pos() {
                let (origin, start) = self
                    .timeline
                    .drag_origin
                    .unwrap_or((pointer, at_pos(pointer)));
                let precision = if origin.y - pointer.y > 120.0 {
                    20.0
                } else if origin.y - pointer.y > 60.0 {
                    5.0
                } else {
                    1.0
                };
                let at = if precision > 1.0 {
                    start + ((pointer.x - origin.x) / rail.width() * span as f32 / precision) as i64
                } else {
                    at_pos(pointer)
                }
                .clamp(0, duration);
                self.timeline.set_mark(kind, at, duration);
                let target = match kind {
                    DragKind::In => self.timeline.start,
                    DragKind::Out => self.timeline.end,
                    DragKind::Seek => self.timeline.position,
                };
                if self.last_preview.elapsed() > Duration::from_millis(100) {
                    self.last_preview = Instant::now();
                    let ppp = ui.ctx().pixels_per_point();
                    let screen = ui.ctx().content_rect();
                    let width = 170.0;
                    let x = (pointer.x - width / 2.0).clamp(
                        screen.left() + 4.0,
                        (screen.right() - width - 4.0).max(screen.left() + 4.0),
                    );
                    let y = (rect.top() - 125.0).max(screen.top());
                    bridge::send(
                        json!({"op":"preview","time":target,"label":format!("{}  {}x",timecode(target,true),precision),"x":(x*ppp) as i32,"y":(y*ppp) as i32,"w":(width*ppp) as i32,"h":(118.0*ppp) as i32}),
                    );
                }
            }
        }
        if response.drag_stopped() {
            if let Some(kind) = self.timeline.dragging.take() {
                let time = match kind {
                    DragKind::In => self.timeline.start,
                    DragKind::Out => self.timeline.end,
                    DragKind::Seek => self.timeline.position,
                };
                self.seek(time, false);
                self.keyframe = None;
                self.sync_loop();
                if self.timeline.resume_after_drag && kind == DragKind::Seek {
                    bridge::send(json!({"op":"play","playing":true}));
                }
            }
            self.timeline.drag_origin = None;
            bridge::send(json!({"op":"preview_end"}));
        } else if response.clicked() {
            if let Some(pointer) = response.interact_pointer_pos() {
                self.seek(at_pos(pointer), false);
            }
        }
        if self.timeline.zoom > 1.0 {
            let max = (duration - span).max(0);
            ui.add_sized(
                [ui.available_width(), 12.0],
                egui::Slider::new(&mut self.timeline.window_start, 0..=max).show_value(false),
            );
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        self.manage_dialog(ctx);
        let width = (ctx.content_rect().width() - 32.0).min(440.0);
        if self.export_dialog {
            egui::Window::new("Export clip").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER,Vec2::ZERO).fixed_size(vec2(width,240.0)).show(ctx,|ui|{
                ui.horizontal(|ui|{ui.selectable_value(&mut self.exact,false,"Fast / original quality");ui.selectable_value(&mut self.exact,true,"Exact cut");});
                ui.add_space(10.0);
                if self.exact {ui.label("Re-encodes video at the selected frames.");}
                else {ui.label("Copies video. Start snaps to the preceding keyframe.");}
                let start=if self.exact{Some(self.timeline.start)}else{self.keyframe};
                ui.add_space(12.0);
                ui.label(format!("In      {}",start.map(|s|timecode(s,true)).unwrap_or("Locating keyframe...".into())));
                ui.label(format!("Out     {}",timecode(self.timeline.end,true)));
                ui.label(egui::RichText::new("Movies / Luma").color(MUTED));
                ui.add_space(12.0);
                ui.horizontal(|ui|{
                    if icons::button(ui,"close","Cancel",false).clicked(){self.export_dialog=false;}
                    if ui.add_enabled(start.is_some(),egui::Button::image_and_text(icons::image("save",ACCENT,20.0),"Save clip").min_size(vec2(160.0,42.0))).clicked(){
                        bridge::send(json!({"op":"export","start":self.timeline.start,"end":self.timeline.end,"exact":self.exact}));self.export_dialog=false;
                    }
                });
            });
        }
        if self.info {
            egui::Window::new("File information")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(vec2(width, 300.0))
                .show(ctx, |ui| {
                    if let Some(item) = &self.selected {
                        ui.label(&item.name);
                        ui.label(&item.album);
                        ui.label(format!(
                            "{} x {}    {}",
                            item.width,
                            item.height,
                            size_label(item.size)
                        ));
                        if item.is_video() {
                            if let Some(streams) = self.metadata["streams"].as_array() {
                                for stream in streams.iter().filter(|s| {
                                    s["codec_type"] == "video" || s["codec_type"] == "audio"
                                }) {
                                    ui.label(format!(
                                        "{}: {} {} {}",
                                        stream["codec_type"].as_str().unwrap_or(""),
                                        stream["codec_name"].as_str().unwrap_or(""),
                                        stream["profile"].as_str().unwrap_or(""),
                                        stream["pix_fmt"].as_str().unwrap_or("")
                                    ));
                                }
                            }
                            ui.separator();
                            if ui
                                .add_enabled(
                                    !self.native["exporting"].as_bool().unwrap_or(false),
                                    egui::Button::image_and_text(
                                        icons::image("video", ACCENT, 20.0),
                                        "Build playback proxy",
                                    ),
                                )
                                .clicked()
                            {
                                bridge::send(json!({"op":"proxy"}));
                                self.info = false;
                            }
                            ui.small("1080p preview. Original file remains the export source.");
                        }
                    }
                    ui.add_space(12.0);
                    if icons::button(ui, "close", "Close information", false).clicked() {
                        self.info = false;
                    }
                });
        }
        if self.settings {
            egui::Window::new("Settings")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .fixed_size(vec2(width, 440.0))
                .show(ctx, |ui| {
                    if ui
                        .checkbox(&mut self.software, "Force software video decoding")
                        .changed()
                    {
                        bridge::send(json!({"op":"software","enabled":self.software}));
                    }
                    if ui.button("Manage photo and video access").clicked() {
                        bridge::send(json!({"op":"permission"}));
                    }
                    if self.native["sdk"].as_i64().unwrap_or(0) >= 31 {
                        let direct = self.native["manage_media"].as_bool().unwrap_or(false);
                        ui.horizontal_wrapped(|ui| {
                            ui.small(if direct {
                                "Deletes, moves and renames apply without asking"
                            } else {
                                "Android confirms each delete, move or rename"
                            });
                            if ui
                                .small_button(if direct {
                                    "Media management"
                                } else {
                                    "Skip confirmations"
                                })
                                .clicked()
                            {
                                bridge::send(json!({"op":"manage_access"}));
                            }
                        });
                    }
                    if let Some(action) = self.last_change["action"].as_str() {
                        ui.small(format!(
                            "Last change: {action}, {} done, {} failed{}{}",
                            self.last_change["done"].as_u64().unwrap_or(0),
                            self.last_change["failed"].as_u64().unwrap_or(0),
                            if self.last_change["cancelled"] == true {
                                ", cancelled"
                            } else {
                                ""
                            },
                            self.last_change["error"]
                                .as_str()
                                .filter(|e| !e.is_empty())
                                .map(|e| format!(" ({e})"))
                                .unwrap_or_default()
                        ));
                    }
                    ui.separator();
                    ui.strong(
                        self.diagnostics["model"]
                            .as_str()
                            .unwrap_or("Device decoders"),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(190.0)
                        .show(ui, |ui| {
                            if let Some(codecs) = self.diagnostics["codecs"].as_array() {
                                for codec in codecs {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.colored_label(
                                            if codec["hardware"] == true {
                                                ACCENT
                                            } else {
                                                MUTED
                                            },
                                            if codec["hardware"] == true {
                                                "HW"
                                            } else {
                                                "SW"
                                            },
                                        );
                                        ui.small(codec["name"].as_str().unwrap_or(""));
                                    });
                                }
                            }
                        });
                    ui.separator();
                    ui.small(concat!("Luma Gallery ", env!("CARGO_PKG_VERSION")));
                    ui.small("LibVLC 3.7.7 / FFmpeg 8.0.1 / LibRaw 0.22");
                    if icons::button(ui, "close", "Close settings", false).clicked() {
                        self.settings = false;
                    }
                });
        }
    }
}

fn text_at(ui: &egui::Ui, position: egui::Pos2, text: &str, size: f32, color: Color32, width: f32) {
    let mut job =
        egui::text::LayoutJob::simple(text.into(), FontId::proportional(size), color, width);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(position, galley, color);
}

fn pick_badge(ui: &egui::Ui, rect: Rect, picked: bool) {
    let center = rect.right_top() + vec2(-15.0, 15.0);
    if picked {
        ui.painter().circle_filled(center, 10.0, ACCENT);
        icons::image("check", BG, 14.0)
            .paint_at(ui, Rect::from_center_size(center, Vec2::splat(14.0)));
    } else {
        ui.painter().circle(
            center,
            10.0,
            Color32::from_black_alpha(110),
            Stroke::new(1.5, TEXT),
        );
    }
}

fn days_left(expires: u64, now: u64) -> String {
    match expires.saturating_sub(now) / 86_400 {
        0 => "Today".into(),
        days => plural(days as usize, "day"),
    }
}

/// Bottom row of equal-width actions; returns the icon name of a tapped, enabled action.
fn action_bar(ui: &mut egui::Ui, actions: &[(&'static str, &str, bool)]) -> Option<&'static str> {
    ui.add_space((ui.available_height() - ACTION_BAR).max(0.0));
    let width = ui.available_width() / actions.len().max(1) as f32;
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        Stroke::new(1.0, LINE),
    );
    let mut tapped = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for &(icon, label, enabled) in actions {
            let (rect, response) = ui.allocate_exact_size(
                vec2(width, ACTION_BAR),
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
                rect.center() + vec2(0.0, 14.0),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(11.0),
                color,
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label)
            });
            if response.clicked() {
                tapped = Some(icon);
            }
        }
    });
    tapped
}

impl EguiApp for Gallery {
    fn theme(&self, ctx: &egui::Context) {
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = BG;
        style.visuals.window_fill = PANEL;
        style.visuals.extreme_bg_color = BG;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.selection.bg_fill = Color32::from_rgb(65, 11, 33);
        style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        style.visuals.widgets.inactive.bg_fill = PANEL;
        style.visuals.widgets.inactive.weak_bg_fill = PANEL;
        style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(6, 35, 33);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(6, 35, 33);
        style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, TRIM);
        style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TRIM);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(65, 11, 33);
        style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(65, 11, 33);
        style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, ACCENT);
        style.visuals.widgets.open = style.visuals.widgets.active;
        style.visuals.hyperlink_color = TRIM;
        style.visuals.window_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
        style.visuals.window_shadow = egui::Shadow {
            offset: [0, 0],
            blur: 6,
            spread: 1,
            color: ACCENT.gamma_multiply(0.25),
        };
        style.visuals.window_corner_radius = 8.into();
        for widget in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            widget.corner_radius = 5.into();
        }
        style.spacing.item_spacing = vec2(6.0, 4.0);
        style.spacing.button_padding = vec2(8.0, 6.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(11.0));
        ctx.set_theme(egui::Theme::Dark);
        ctx.set_style_of(egui::Theme::Dark, style);
        egui_extras::install_image_loaders(ctx);
    }
    fn on_start(&mut self, _: &egui::Context, host: &Host) {
        if let Some(dir) = host.documents_dir() {
            let path = std::path::PathBuf::from(dir).join("luma-favorites.json");
            if let Ok(data) = std::fs::read(&path) {
                self.favorites = serde_json::from_slice(&data).unwrap_or_default();
            }
            self.preferences = Some(path);
        }
    }
    fn on_pause(&mut self, _: &Host) {
        bridge::send(json!({"op":"play","playing":false}));
        self.save();
    }
    fn update(&mut self, ui: &mut egui::Ui, host: &Host) {
        self.frame += 1;
        self.pump(ui.ctx());
        if ui.input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::BrowserBack)
        }) {
            self.back();
        }
        let safe = host.safe_area_insets();
        egui::Frame::new()
            .fill(BG)
            .inner_margin(egui::Margin {
                left: 12,
                right: 12,
                // EguiMobile already applies the Android safe area to the root UI.
                top: 4,
                bottom: 4,
            })
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                if self.native["exporting"].as_bool().unwrap_or(false) {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::ProgressBar::new(
                                self.native["progress"].as_f64().unwrap_or(0.0) as f32
                            )
                            .desired_width((ui.available_width() - 50.0).max(80.0))
                            .text(self.native["export_status"].as_str().unwrap_or("Exporting")),
                        );
                        if icons::button(ui, "close", "Cancel export", false).clicked() {
                            bridge::send(json!({"op":"cancel_export"}));
                        }
                    });
                }
                if self.selected.is_some() {
                    self.viewer(ui);
                } else {
                    self.library(ui);
                }
            });
        self.dialogs(ui.ctx());
        if std::mem::take(&mut self.buzz) {
            host.haptic(egui_mobile::Haptic::Medium);
        }
        if self.busy() {
            egui::Area::new(egui::Id::new("manage_progress"))
                .order(egui::Order::Foreground)
                .anchor(Align2::CENTER_TOP, vec2(0.0, safe.top + 52.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(8)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::ProgressBar::new(
                                        self.native["manage_progress"].as_f64().unwrap_or(0.0)
                                            as f32,
                                    )
                                    .desired_width(
                                        (ui.ctx().content_rect().width() - 110.0).min(360.0),
                                    )
                                    .text(
                                        self.native["manage_status"].as_str().unwrap_or("Working"),
                                    ),
                                );
                                if icons::button(ui, "close", "Stop changes", false).clicked() {
                                    bridge::send(json!({"op":"cancel_manage"}));
                                }
                            });
                        });
                });
        }
        let mut undo = None;
        if let Some((text, at, restore)) = &self.toast
            && at.elapsed() < Duration::from_secs(9)
        {
            let bottom = if self.selection.active {
                ACTION_BAR
            } else {
                0.0
            };
            egui::Area::new(egui::Id::new("notice"))
                .order(egui::Order::Foreground)
                .anchor(
                    Align2::CENTER_BOTTOM,
                    vec2(0.0, -safe.bottom - bottom - 8.0),
                )
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.set_max_width((ui.ctx().content_rect().width() - 48.0).min(500.0));
                            ui.label(text);
                            if !restore.is_empty()
                                && ui
                                    .add(
                                        egui::Button::image_and_text(
                                            icons::image("restore", ACCENT, 18.0),
                                            "Undo",
                                        )
                                        .min_size(vec2(96.0, 36.0)),
                                    )
                                    .clicked()
                            {
                                undo = Some(restore.clone());
                            }
                        });
                });
        }
        if let Some(items) = undo {
            self.toast = None;
            self.send_manage("restore", &items, json!({}));
        }
        ui.ctx()
            .request_repaint_after(Duration::from_millis(if self.playing() {
                16
            } else if self.selected.is_some() || self.busy() {
                50
            } else {
                180
            }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gallery() -> Gallery {
        let mut gallery = Gallery::new(&CreateContext {
            width_px: 1440,
            height_px: 3120,
            pixels_per_point: 3.5,
        });
        gallery.items = [
            ("new", "New.ARW", "raw", "Camera"),
            ("middle", "Middle.jpg", "photo", "Camera"),
            ("old", "Old.ARW", "raw", "Camera"),
            ("other", "Other.ARW", "raw", "Downloads"),
        ]
        .into_iter()
        .map(|(uri, name, kind, album)| MediaItem {
            uri: uri.into(),
            name: name.into(),
            kind: kind.into(),
            album: album.into(),
            ..MediaItem::default()
        })
        .collect();
        gallery.album = Some("Camera".into());
        gallery
    }

    fn uris(gallery: &Gallery) -> Vec<String> {
        gallery
            .visible_items()
            .into_iter()
            .map(|item| item.uri)
            .collect()
    }

    #[test]
    fn grid_rail_and_navigation_share_date_order() {
        let mut gallery = gallery();
        assert_eq!(uris(&gallery), ["new", "middle", "old"]);
        gallery.oldest = true;
        assert_eq!(uris(&gallery), ["old", "middle", "new"]);
        gallery.open(gallery.items[1].clone());
        gallery.center_rail = false;
        gallery.neighbor(1);
        assert_eq!(gallery.selected.as_ref().unwrap().uri, "new");
        assert!(gallery.center_rail);
        gallery.neighbor(-1);
        assert_eq!(gallery.selected.as_ref().unwrap().uri, "middle");
    }

    #[test]
    fn navigation_stays_inside_filtered_album_and_does_not_wrap() {
        let mut gallery = gallery();
        gallery.filter = "raw".into();
        assert_eq!(uris(&gallery), ["new", "old"]);
        gallery.open(gallery.items[0].clone());
        gallery.neighbor(-1);
        assert_eq!(gallery.selected.as_ref().unwrap().uri, "new");
        gallery.neighbor(1);
        assert_eq!(gallery.selected.as_ref().unwrap().uri, "old");
        gallery.neighbor(1);
        assert_eq!(gallery.selected.as_ref().unwrap().uri, "old");
    }

    #[test]
    fn rail_respects_search_and_favorites() {
        let mut gallery = gallery();
        gallery.query = "old".into();
        assert_eq!(uris(&gallery), ["old"]);
        gallery.query.clear();
        gallery.view = View::Favorites;
        gallery.favorites.extend(["middle".into(), "other".into()]);
        assert_eq!(uris(&gallery), ["middle"]);
        gallery.album = None;
        assert_eq!(uris(&gallery), ["middle", "other"]);
    }

    #[test]
    fn virtual_grid_rows_keep_their_position_across_scroll_boundaries() {
        for view in [View::Albums, View::All] {
            for size in [vec2(411.0, 891.0), vec2(891.0, 411.0)] {
                let ctx = egui::Context::default();
                let mut gallery = gallery();
                gallery.album = None;
                gallery.view = view;
                gallery.items = (0..120)
                    .map(|i| MediaItem {
                        uri: format!("test:{i}"),
                        album: format!("Album-{i:03}"),
                        name: format!("{i}.mp4"),
                        kind: "video".into(),
                        duration: (i + 1) * 1000,
                        ..MediaItem::default()
                    })
                    .collect();
                gallery.theme(&ctx);
                let mut positions = HashMap::<String, f32>::new();
                let mut comparisons = 0;
                // Small steps cross many virtual row boundaries in both directions.
                for offset in (0..1400).step_by(3).chain((0..1400).step_by(3).rev()) {
                    let mut actual_offset = 0.0;
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)),
                            viewports: [(
                                egui::ViewportId::ROOT,
                                egui::ViewportInfo {
                                    native_pixels_per_point: Some(3.5),
                                    ..Default::default()
                                },
                            )]
                            .into_iter()
                            .collect(),
                            ..Default::default()
                        },
                        |ui| {
                            egui::CentralPanel::default().show(ui, |ui| {
                                let id = if view == View::Albums {
                                    ui.make_persistent_id(egui::IdSalt::new("albums"))
                                } else {
                                    ui.make_persistent_id(egui::IdSalt::new((
                                        "media",
                                        &gallery.album,
                                    )))
                                };
                                let mut state =
                                    egui::scroll_area::State::load(&ctx, id).unwrap_or_default();
                                state.offset.y = offset as f32;
                                state.store(&ctx, id);
                                gallery.library(ui);
                                actual_offset =
                                    egui::scroll_area::State::load(&ctx, id).unwrap().offset.y;
                            });
                        },
                    );
                    output.textures_delta.clear();
                    for shape in output.shapes {
                        if let egui::Shape::Text(text) = shape.shape {
                            let label = text.galley.text();
                            let row_label = if view == View::Albums {
                                label.starts_with("Album-")
                            } else {
                                label.contains(':') && label.len() == 5
                            };
                            if row_label && shape.clip_rect.contains(text.pos) {
                                let document_y = text.pos.y + actual_offset;
                                if let Some(previous) = positions.insert(label.into(), document_y) {
                                    assert!(
                                        (previous - document_y).abs() < 1.0,
                                        "{label} jumped {} points at offset {offset}",
                                        document_y - previous
                                    );
                                    comparisons += 1;
                                }
                            }
                        }
                    }
                }
                assert!(positions.len() > 10 && comparisons > 200);
            }
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
            id: egui::TouchId(7),
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

    /// Runs one library frame and returns its largest square panels, row by row.
    fn library_frame(
        ctx: &egui::Context,
        gallery: &mut Gallery,
        time: f64,
        events: Vec<egui::Event>,
    ) -> Vec<Rect> {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(411.0, 891.0))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| gallery.library(ui));
            },
        );
        output.textures_delta.clear();
        let mut tiles: Vec<Rect> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect)
                    if rect.fill == PANEL && (rect.rect.width() - rect.rect.height()).abs() < 1.0 =>
                {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect();
        let side = tiles.iter().map(|r| r.width()).fold(0.0, f32::max);
        tiles.retain(|r| r.width() > 60.0 && (r.width() - side).abs() < 1.0);
        tiles.sort_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)));
        tiles
    }

    #[test]
    fn long_press_selects_drags_a_range_and_taps_toggle_without_scrolling() {
        use egui::TouchPhase::{End, Move, Start};
        for view in [View::Albums, View::All] {
            let ctx = egui::Context::default();
            let mut gallery = gallery();
            gallery.album = None;
            gallery.view = view;
            gallery.items = (0..40)
                .map(|i| MediaItem {
                    uri: format!("test:{i}"),
                    album: format!("Album-{i:03}"),
                    kind: "photo".into(),
                    ..MediaItem::default()
                })
                .collect();
            gallery.theme(&ctx);
            let keys: Vec<String> = gallery
                .items
                .iter()
                .map(|item| match view {
                    View::Albums => item.album.clone(),
                    _ => item.uri.clone(),
                })
                .collect();
            let picked = |gallery: &Gallery| {
                let mut picked: Vec<_> = gallery.selection.picked.iter().cloned().collect();
                picked.sort();
                picked
            };
            let tiles = library_frame(&ctx, &mut gallery, 0.0, vec![]);
            assert!(tiles.len() > 5, "{} tiles", tiles.len());
            library_frame(&ctx, &mut gallery, 0.1, touch(Start, tiles[0].center()));
            library_frame(&ctx, &mut gallery, 0.3, vec![]);
            assert!(!gallery.selection.active, "Selection began before the hold");
            library_frame(&ctx, &mut gallery, 0.5, vec![]);
            library_frame(&ctx, &mut gallery, 0.65, vec![]);
            assert!(gallery.selection.active && gallery.selection.dragging());
            assert!(gallery.buzz);
            assert_eq!(picked(&gallery), keys[..1]);
            library_frame(&ctx, &mut gallery, 0.7, touch(Move, tiles[4].center()));
            assert_eq!(picked(&gallery), keys[..5]);
            library_frame(&ctx, &mut gallery, 0.8, touch(End, tiles[4].center()));
            assert!(!gallery.selection.dragging());
            library_frame(&ctx, &mut gallery, 1.0, touch(Start, tiles[1].center()));
            library_frame(&ctx, &mut gallery, 1.05, touch(End, tiles[1].center()));
            assert_eq!(
                picked(&gallery),
                [&keys[0], &keys[2], &keys[3], &keys[4]].map(String::clone)
            );
            assert!(gallery.selected.is_none() && gallery.album.is_none());
            let salt = if view == View::Albums {
                egui::IdSalt::new("albums")
            } else {
                egui::IdSalt::new(("media", &gallery.album))
            };
            let mut offset = -1.0;
            ctx.run_ui(Default::default(), |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    offset = egui::scroll_area::State::load(ui.ctx(), ui.make_persistent_id(salt))
                        .map_or(-1.0, |state| state.offset.y);
                });
            })
            .textures_delta
            .clear();
            assert_eq!(offset, 0.0, "Selecting scrolled the grid");
            gallery.back();
            assert!(!gallery.selection.active && gallery.selection.picked.is_empty());
        }
    }

    #[test]
    fn finished_changes_move_the_viewer_and_keep_favorites() {
        let uri = |name: &str| format!("content://media/external/images/media/{name}");
        let mut gallery = gallery();
        for item in &mut gallery.items {
            item.uri = uri(&item.uri);
        }
        gallery.favorites.insert(uri("middle"));
        gallery.open(gallery.items[1].clone());
        gallery.selection.active = true;
        gallery.managed(&json!({"action":"trash","done":1,"uris":[uri("middle")]}));
        assert_eq!(gallery.selected.as_ref().unwrap().uri, uri("old"));
        assert!(!gallery.selection.active);
        let (text, _, undo) = gallery.toast.clone().unwrap();
        assert_eq!(text, "Moved 1 item to the trash");
        assert_eq!(undo.len(), 1);
        assert!(gallery.favorites.contains(&uri("middle")));
        gallery.managed(&json!({"action":"move","done":1,"target":"Pictures/Trips",
            "uris":[uri("middle")],"renamed":{uri("middle"): uri("99")}}));
        assert!(gallery.favorites.contains(&uri("99")) && !gallery.favorites.contains(&uri("middle")));
        assert_eq!(gallery.toast.as_ref().unwrap().0, "Moved 1 item to Trips");
        gallery.managed(&json!({"action":"delete","done":1,"uris":[uri("99")]}));
        assert!(gallery.favorites.is_empty());
        gallery.managed(&json!({"action":"rename","failed":2,"error":"a.jpg: denied"}));
        assert_eq!(gallery.toast.as_ref().unwrap().0, "Could not rename: a.jpg: denied");
        gallery.managed(&json!({"action":"copy","done":2,"failed":1,"cancelled":true,
            "target":"DCIM/Keep","error":"b.jpg: full"}));
        assert_eq!(
            gallery.toast.as_ref().unwrap().0,
            "Copied 2 items to Keep; the rest was cancelled. 1 item failed: b.jpg: full"
        );
        gallery.managed(&json!({"action":"restore","done":0,"cancelled":true}));
        assert_eq!(gallery.toast.as_ref().unwrap().0, "Cancelled");
        gallery.items[3].uri = "content://com.android.externalstorage.documents/tree/a/document/b".into();
        let linked = gallery.items[3].uri.clone();
        gallery.managed(&json!({"action":"trash","done":2,"uris":[uri("new"), linked]}));
        let (text, _, undo) = gallery.toast.clone().unwrap();
        assert_eq!(text, "Moved 1 item to the trash and deleted 1");
        assert_eq!(undo.len(), 1);
        gallery.managed(&json!({"action":"trash","done":1,"uris":[linked]}));
        assert_eq!(gallery.toast.as_ref().unwrap().0, "Deleted 1 item");
        gallery.open(gallery.items[2].clone());
        gallery.managed(&json!({"action":"trash","done":1,"uris":[uri("old")]}));
        assert_eq!(gallery.selected.as_ref().unwrap().uri, uri("middle"));
    }

    #[test]
    fn deletes_trash_library_items_and_confirm_permanent_ones() {
        let mut gallery = gallery();
        gallery.native = json!({"sdk": 35});
        let mut items = gallery.items.clone();
        for item in &mut items {
            item.uri = format!("content://media/external/images/media/{}", item.uri);
        }
        gallery.delete(items.clone());
        assert!(gallery.dialog.is_none(), "Trash moves are confirmed by Android");
        let mut linked = items.clone();
        linked[0].uri = "content://com.android.externalstorage.documents/tree/x/document/y".into();
        gallery.delete(linked);
        assert!(matches!(&gallery.dialog, Some(Dialog::Delete { action: "trash", detail, .. })
            if detail.starts_with("1 item from linked folders")));
        items[0].expires = 1;
        gallery.delete(items.clone());
        assert!(matches!(&gallery.dialog, Some(Dialog::Delete { action: "delete", title, .. })
            if title == "Delete forever?"));
        gallery.native = json!({"sdk": 29});
        items[0].expires = 0;
        gallery.delete(items);
        assert!(matches!(&gallery.dialog, Some(Dialog::Delete { action: "delete", .. })));
        gallery.back();
        assert!(gallery.dialog.is_none());
    }

    #[test]
    fn first_touch_can_scroll_without_a_prior_tap() {
        for view in [View::Albums, View::All] {
            let ctx = egui::Context::default();
            let mut gallery = gallery();
            gallery.album = None;
            gallery.view = view;
            gallery.items = (0..100)
                .map(|i| MediaItem {
                    uri: format!("test:{i}"),
                    album: format!("Album-{i:03}"),
                    kind: "photo".into(),
                    ..MediaItem::default()
                })
                .collect();
            gallery.theme(&ctx);
            let mut offset = 0.0;
            for frame in 0..12 {
                let mut events = vec![];
                if frame >= 2 {
                    let position = pos2(185.0, 770.0 - (frame - 2) as f32 * 35.0);
                    events.push(egui::Event::Touch {
                        device_id: egui::TouchDeviceId(0),
                        id: egui::TouchId(42),
                        phase: if frame == 2 {
                            egui::TouchPhase::Start
                        } else {
                            egui::TouchPhase::Move
                        },
                        pos: position,
                        force: None,
                    });
                    events.push(egui::Event::PointerMoved(position));
                    if frame == 2 {
                        events.push(egui::Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: Default::default(),
                        });
                    }
                }
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(411.0, 891.0),
                        )),
                        time: Some(frame as f64 / 60.0),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        egui::CentralPanel::default().show(ui, |ui| {
                            let salt = if view == View::Albums {
                                egui::IdSalt::new("albums")
                            } else {
                                egui::IdSalt::new(("media", &gallery.album))
                            };
                            let id = ui.make_persistent_id(salt);
                            gallery.library(ui);
                            offset = egui::scroll_area::State::load(&ctx, id).unwrap().offset.y;
                        });
                    },
                );
                output.textures_delta.clear();
            }
            assert!(
                offset > 200.0,
                "First touch did not move the gallery: {offset}"
            );
            assert!(gallery.selected.is_none(), "Scrolling opened an item");
        }
    }
}
