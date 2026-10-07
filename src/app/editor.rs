use super::*;
use crate::edit::{Crop, Recipe};
use crate::mask::{self, MaskCanvas, StrokeRec, ViewXform};
#[cfg(target_os = "android")]
use egui_mobile::HostExt;
use image::RgbaImage;
use std::io::Cursor;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Crop,
    Adjust,
    Qwen,
}

#[derive(Clone, Copy)]
pub(super) struct CropDrag {
    kind: usize,
    base: Crop,
    origin: Pos2,
}

pub(super) struct PhotoEditor {
    pub session: String,
    item: MediaItem,
    source: Option<RgbaImage>,
    path: String,
    export_size: (u32, u32),
    recipe: Recipe,
    rendered: Option<(Recipe, bool)>,
    working: Option<RgbaImage>,
    texture: Option<egui::TextureHandle>,
    result: Option<(String, egui::TextureHandle)>,
    compare: bool,
    tab: Tab,
    crop_drag: Option<CropDrag>,
    view: ViewXform,
    canvas: MaskCanvas,
    overlay: Option<egui::TextureHandle>,
    strokes: Vec<StrokeRec>,
    groups: Vec<usize>,
    last_uv: Option<(f32, f32)>,
    pressure: Option<f32>,
    contact_erase: bool,
    brush: f32,
    erase: bool,
    pen_only: bool,
    pen_initialized: bool,
    nav_latch: bool,
    telemetry: bool,
    prompt: String,
    turbo: bool,
    png: bool,
    status: String,
    server_open: bool,
    server: String,
    key: String,
    username: String,
    password: String,
    pending: bool,
    submitted: bool,
    discard: bool,
    saved: bool,
}

impl PhotoEditor {
    pub fn new(item: MediaItem) -> Self {
        let session = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_string();
        bridge::send(json!({"op":"photo_prepare","session":session,"item":item}));
        bridge::send(json!({"op":"qwen_config","session":session,"uri":item.uri}));
        Self {
            session,
            item,
            source: None,
            path: String::new(),
            export_size: (0, 0),
            recipe: Recipe::default(),
            rendered: None,
            working: None,
            texture: None,
            result: None,
            compare: false,
            tab: Tab::Crop,
            crop_drag: None,
            view: ViewXform::FIT,
            canvas: MaskCanvas::new(1, 1),
            overlay: None,
            strokes: vec![],
            groups: vec![],
            last_uv: None,
            pressure: None,
            contact_erase: false,
            brush: 0.045,
            erase: false,
            pen_only: false,
            pen_initialized: false,
            nav_latch: false,
            telemetry: false,
            prompt: String::new(),
            turbo: false,
            png: false,
            status: "Preparing photo…".into(),
            server_open: false,
            server: String::new(),
            key: String::new(),
            username: String::new(),
            password: String::new(),
            pending: false,
            submitted: false,
            discard: false,
            saved: false,
        }
    }

    pub fn event(&mut self, ctx: &egui::Context, e: &Value) {
        if e["session"].as_str() != Some(&self.session) {
            return;
        }
        match e["type"].as_str().unwrap_or_default() {
            "edit_ready" => {
                self.path = e["path"].as_str().unwrap_or_default().into();
                match std::fs::read(&self.path)
                    .ok()
                    .and_then(|b| image::load_from_memory(&b).ok())
                {
                    Some(image) => {
                        self.source = Some(image.to_rgba8());
                        self.status.clear();
                    }
                    None => {
                        self.status = "Could not load the editor preview. Reopen the photo.".into()
                    }
                }
                self.export_size = (
                    e["export_width"].as_u64().unwrap_or(0) as u32,
                    e["export_height"].as_u64().unwrap_or(0) as u32,
                );
                self.item.width = e["width"].as_u64().unwrap_or(self.item.width as u64) as u32;
                self.item.height = e["height"].as_u64().unwrap_or(self.item.height as u64) as u32;
            }
            "qwen_config" => {
                self.server = e["url"].as_str().unwrap_or_default().into();
                self.pending = e["pending"] == true;
                self.status = e["message"].as_str().unwrap_or_default().into();
            }
            "qwen_result" => {
                self.saved = false;
                self.submitted = false;
                self.pending = false;
                let path = e["path"].as_str().unwrap_or_default();
                match std::fs::read(path)
                    .ok()
                    .and_then(|b| image::load_from_memory(&b).ok())
                {
                    Some(image) => {
                        self.result =
                            Some((path.into(), texture(ctx, "qwen-result", &image.to_rgba8())));
                        self.compare = false;
                        self.status = "Review the result, then save a copy.".into();
                    }
                    None => self.status = "The server returned an unreadable image.".into(),
                }
            }
            "edit_error" | "qwen_error" => {
                self.submitted = false;
                self.status = e["message"].as_str().unwrap_or("Edit failed").into();
                if e["type"] == "qwen_error" {
                    self.pending = e["pending"] == true;
                }
            }
            "photo_saved" => {
                self.saved = true;
                self.submitted = false;
                self.status = "Saved to Pictures/Luma".into();
            }
            _ => {}
        }
    }

    pub fn back(&mut self) -> bool {
        if self.server_open {
            self.server_open = false;
            return false;
        }
        if !self.saved
            && (self.recipe != Recipe::default()
                || !self.canvas.is_empty()
                || self.result.is_some())
        {
            self.discard = true;
            false
        } else {
            true
        }
    }

    fn clear_mask(&mut self) {
        self.saved = false;
        self.strokes.clear();
        self.groups.clear();
        self.last_uv = None;
        self.pressure = None;
        self.overlay = None;
        self.canvas = MaskCanvas::new(self.canvas.w, self.canvas.h);
        self.view = ViewXform::FIT;
    }

    fn prepare_texture(&mut self, ctx: &egui::Context) {
        let crop_mode = self.tab == Tab::Crop;
        if self.rendered == Some((self.recipe, crop_mode)) {
            return;
        }
        let Some(source) = &self.source else {
            return;
        };
        let transformed = self.recipe.preview(source);
        let (x, y, w, h) = self
            .recipe
            .crop
            .pixels(transformed.width(), transformed.height());
        let cropped = image::imageops::crop_imm(&transformed, x, y, w, h).to_image();
        self.texture = Some(texture(
            ctx,
            "photo-editor",
            if crop_mode { &transformed } else { &cropped },
        ));
        // A tab switch changes the presentation, not the painted region. Only a
        // changed recipe invalidates the mask's source pixels and coordinates.
        if self
            .rendered
            .is_none_or(|(recipe, _)| recipe != self.recipe)
        {
            let scale = 512.0 / cropped.width().max(cropped.height()) as f32;
            self.canvas = MaskCanvas::new(
                (cropped.width() as f32 * scale).round().max(1.0) as u32,
                (cropped.height() as f32 * scale).round().max(1.0) as u32,
            );
            self.clear_mask();
        }
        self.working = Some(cropped);
        self.rendered = Some((self.recipe, crop_mode));
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, host: &Host, native: &Value) -> bool {
        let old_recipe = self.recipe;
        if !self.pen_initialized {
            #[cfg(target_os = "android")]
            {
                self.pen_only = host.has_stylus();
            }
            self.pen_initialized = true;
        }
        self.prepare_texture(ui.ctx());
        let busy = self.submitted || native["photo_saving"] == true || native["qwen_busy"] == true;
        let mut close = false;
        ui.horizontal(|ui| {
            if icons::button(ui,"back","Close editor",false).clicked(){close=self.back();}
            ui.strong("Edit photo");
            ui.with_layout(egui::Layout::right_to_left(Align::Center),|ui| {
                if ui.add_enabled(!busy&&self.source.is_some(),egui::Button::image_and_text(icons::image("save",ACCENT,20.0),"Save copy").min_size(vec2(110.0,40.0))).clicked() {
                    let mut request=json!({"op":"photo_save","session":self.session,"item":self.item,"recipe":self.recipe,"png":self.png});
                    if let Some((path,_))=&self.result {request["result"]=path.clone().into();request["png"]=true.into();}
                    bridge::send(request); self.submitted=true;self.status="Saving photo…".into();
                }
            });
        });
        ui.add(
            egui::Label::new(egui::RichText::new(&self.item.name).small().color(MUTED)).truncate(),
        );
        let available = ui.available_size();
        // A tall keyboard can make a portrait viewport look landscape-shaped.
        // Only split when both columns have room for their controls.
        if available.x >= 600.0 && available.x > available.y * 1.5 {
            ui.columns(2, |columns| {
                let size = columns[0].available_size();
                self.stage(&mut columns[0], host, size, busy);
                egui::ScrollArea::vertical()
                    .id_salt("edit-controls")
                    .min_scrolled_height(0.0)
                    .scroll_source(egui::scroll_area::ScrollSource::ALL)
                    .show(&mut columns[1], |ui| self.controls(ui, busy, native));
            });
        } else {
            self.stage(
                ui,
                host,
                vec2(available.x, (available.y - 290.0).max(100.0)),
                busy,
            );
            egui::ScrollArea::vertical()
                .id_salt("edit-controls")
                .min_scrolled_height(0.0)
                .scroll_source(egui::scroll_area::ScrollSource::ALL)
                .show(ui, |ui| self.controls(ui, busy, native));
        }
        self.server_dialog(ui.ctx(), busy);
        if self.discard {
            egui::Window::new("Leave editor?")
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    ui.label("Unsaved edits will be discarded.");
                    ui.horizontal(|ui| {
                        if ui.button("Keep editing").clicked() {
                            self.discard = false;
                        }
                        if ui.button("Discard edits").clicked() {
                            close = true;
                        }
                    });
                });
        }
        if old_recipe != self.recipe {
            self.saved = false;
        }
        close
    }

    fn controls(&mut self, ui: &mut egui::Ui, busy: bool, native: &Value) {
        if self.result.is_some() {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.compare, true, "Before");
                ui.selectable_value(&mut self.compare, false, "Qwen result");
                if ui
                    .add_enabled(!busy, egui::Button::new("Discard result"))
                    .clicked()
                {
                    self.result = None;
                    self.saved = false;
                    self.status.clear();
                }
            });
        } else {
            ui.add_enabled_ui(!busy, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.tab, Tab::Crop, "Crop");
                    ui.selectable_value(&mut self.tab, Tab::Adjust, "Adjust");
                    ui.selectable_value(&mut self.tab, Tab::Qwen, "Qwen edit");
                });
                match self.tab {
                    Tab::Crop => {
                        let dims = self.texture.as_ref().map(|t| t.size()).unwrap_or([1, 1]);
                        crop_presets(ui, &mut self.recipe.crop, dims[0] as u32, dims[1] as u32);
                        ui.horizontal(|ui| {
                            if icons::button(ui, "rotate", "Rotate clockwise", false).clicked() {
                                self.recipe.rotation = (self.recipe.rotation + 1) % 4;
                                self.recipe.crop = Crop::default();
                            }
                            if icons::button(ui, "flip", "Flip horizontally", self.recipe.flip)
                                .clicked()
                            {
                                self.recipe.flip = !self.recipe.flip;
                                self.recipe.crop = Crop::default();
                            }
                            ui.small("Drag a corner or move the crop");
                        });
                    }
                    Tab::Adjust => {
                        ui.add(
                            egui::Slider::new(&mut self.recipe.exposure, -3.0..=3.0)
                                .text("Exposure")
                                .suffix(" EV"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.recipe.contrast, 0.5..=1.8)
                                .text("Contrast"),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.recipe.saturation, 0.0..=2.0).text("Color"),
                        );
                    }
                    Tab::Qwen => self.qwen_controls(ui),
                }
            });
        }
        if self.tab != Tab::Qwen && self.result.is_none() {
            ui.horizontal(|ui| {
                ui.add_enabled(!busy, egui::Checkbox::new(&mut self.png, "PNG"));
                let (w, h) = self
                    .recipe
                    .output_size(self.export_size.0, self.export_size.1);
                ui.small(format!("{w} × {h} · SDR copy"));
                if ui.add_enabled(!busy, egui::Button::new("Reset")).clicked() {
                    self.recipe = Recipe::default();
                }
            });
            if (self.item.width as u64 * self.item.height as u64) > 32_000_000 {
                ui.small("Large photos are exported at up to 32 MP.");
            }
            if self.item.name.to_lowercase().ends_with(".gif") {
                ui.small("Edits save the first frame as a still photo.");
            }
        }
        let progress = if native["qwen_busy"] == true {
            native["qwen_status"].as_str().unwrap_or("")
        } else if native["photo_saving"] == true {
            native["photo_edit_status"].as_str().unwrap_or("")
        } else {
            &self.status
        };
        if !progress.is_empty() {
            ui.colored_label(TRIM, progress);
        }
        if native["qwen_busy"] == true && ui.button("Stop waiting").clicked() {
            bridge::send(json!({"op":"qwen_stop"}));
        }
    }

    fn qwen_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if icons::button(ui, "brush", "Paint mask", !self.erase).clicked() {
                self.erase = false;
            }
            if icons::button(ui, "eraser", "Erase mask", self.erase).clicked() {
                self.erase = true;
            }
            if ui
                .add_enabled(
                    !self.groups.is_empty(),
                    egui::Button::image(icons::image("undo", TEXT, 21.0))
                        .min_size(vec2(40.0, 40.0)),
                )
                .on_hover_text("Undo stroke")
                .clicked()
            {
                self.strokes.truncate(self.groups.pop().unwrap_or(0));
                self.saved = false;
                self.canvas = mask::rasterize(self.canvas.w, self.canvas.h, &self.strokes);
                self.update_overlay(ui.ctx());
            }
            if icons::button(ui, "delete", "Clear mask", false).clicked() {
                self.clear_mask();
            }
            ui.checkbox(&mut self.pen_only, "Pen only");
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::Slider::new(&mut self.brush, 0.008..=0.15)
                    .show_value(false)
                    .text("Brush"),
            );
            if icons::button(ui, "fit", "Reset canvas zoom", false).clicked() {
                self.view = ViewXform::FIT;
            }
            if icons::button(ui, "info", "S Pen input details", self.telemetry).clicked() {
                self.telemetry = !self.telemetry;
            }
        });
        let edit = egui::TextEdit::multiline(&mut self.prompt)
            // The keyboard can switch the editor between stacked and column layouts.
            // Auto IDs include the parent UI, which would end this editing session.
            .id(egui::Id::new(("qwen-prompt", &self.session)))
            .hint_text("Paint an area, then describe the change")
            .desired_rows(2)
            .desired_width(f32::INFINITY)
            .show(ui);
        // Pasting into an empty TextEdit can grow its galley a frame after its change
        // event. Also follow the caret when the IME or rotation changes the viewport.
        // Do not follow ordinary scrolling, which must still let users inspect earlier lines.
        let geometry = (edit.galley.size(), ui.clip_rect());
        let geometry_id = edit.response.id.with("prompt-geometry");
        let reflowed = ui.ctx().data_mut(|data| {
            let previous = data.get_temp::<(Vec2, Rect)>(geometry_id);
            data.insert_temp(geometry_id, geometry);
            previous != Some(geometry)
        });
        if reflowed
            && edit.response.has_focus()
            && let Some(cursor) = edit.cursor_range
        {
            let caret = edit
                .galley
                .pos_from_cursor(cursor.primary)
                .translate(edit.galley_pos.to_vec2());
            ui.scroll_to_rect(caret, None);
        }
        ui.horizontal(|ui|{
            ui.checkbox(&mut self.turbo,"Quick draft");
            if ui.button("Server").clicked(){self.server_open=true;}
            let enabled=!self.prompt.trim().is_empty()&&self.prompt.chars().count()<=2000&&self.canvas.buf.iter().any(|&p|p>=128)&&!self.server.is_empty()&&self.working.is_some();
            if ui.add_enabled(enabled,egui::Button::new("Run edit")).clicked(){
                ui.memory_mut(|m| { if let Some(id)=m.focused() { m.surrender_focus(id); } });
                let result=(||->Result<String,String>{
                    let working=self.working.as_ref().ok_or("Photo is still loading")?;
                    let mut input=Cursor::new(Vec::new());
                    working.write_to(&mut input,image::ImageFormat::Png).map_err(|e|e.to_string())?;
                    let bytes=mask::bake_alpha(input.get_ref(),&self.canvas)?;
                    let path=format!("{}-mask.png",self.path);
                    std::fs::write(&path,bytes).map_err(|e|e.to_string())?;Ok(path)
                })();
                match result{
                    Ok(path)=>{bridge::send(json!({"op":"qwen_edit","session":self.session,"uri":self.item.uri,"path":path,"instruction":self.prompt,"turbo":self.turbo,"steps":if self.turbo{4}else{25}}));self.submitted=true;self.status="Uploading painted photo…".into();}
                    Err(e)=>self.status=e,
                }
            }
            if self.pending && ui.button("Resume").clicked(){bridge::send(json!({"op":"qwen_resume","session":self.session,"uri":self.item.uri}));self.submitted=true;}
        });
        if let Some(working) = &self.working {
            ui.small(format!(
                "AI copy: {} × {}. Pinch to zoom; two fingers pan.",
                working.width(),
                working.height()
            ));
        }
        ui.small(if self.server.is_empty() {
            "Set your ComfyUI server to use Qwen.".into()
        } else {
            format!("Uploads the photo and mask to {}", self.server)
        });
    }

    fn stage(&mut self, ui: &mut egui::Ui, host: &Host, size: Vec2, busy: bool) {
        let (area, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let Some(tex) = self.texture.as_ref() else {
            ui.painter().text(
                area.center(),
                Align2::CENTER_CENTER,
                &self.status,
                FontId::proportional(14.0),
                MUTED,
            );
            return;
        };
        let tex = if !self.compare {
            self.result.as_ref().map(|(_, t)| t).unwrap_or(tex)
        } else {
            tex
        };
        let ratio = (area.width() / tex.size_vec2().x).min(area.height() / tex.size_vec2().y);
        let fit = Rect::from_center_size(area.center(), tex.size_vec2() * ratio);
        let mut view = fit;
        let editable = !busy && !self.server_open && !self.discard && self.result.is_none();
        if self.tab == Tab::Qwen && self.result.is_none() {
            let mt = ui.input(|i| i.multi_touch());
            if let Some(mt) = mt.filter(|mt| area.contains(mt.center_pos)) {
                self.view = self.view.pinch(
                    (fit.center().x, fit.center().y, fit.width(), fit.height()),
                    (area.width(), area.height()),
                    mt.zoom_delta,
                    (mt.center_pos.x, mt.center_pos.y),
                    (mt.translation_delta.x, mt.translation_delta.y),
                );
                self.nav_latch = true;
                self.last_uv = None;
            }
            if !ui.input(|i| i.pointer.any_down()) {
                self.nav_latch = false;
            }
            let (x, y, w, h) =
                self.view
                    .view_rect((fit.center().x, fit.center().y, fit.width(), fit.height()));
            view = Rect::from_min_size(pos2(x, y), vec2(w, h));
        }
        ui.painter().with_clip_rect(area).image(
            tex.id(),
            view,
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        if self.tab == Tab::Crop && self.result.is_none() {
            crop_overlay(
                ui,
                view,
                &response,
                &mut self.recipe.crop,
                &mut self.crop_drag,
                editable,
            );
        }
        if self.tab != Tab::Qwen || self.result.is_some() {
            return;
        }
        #[cfg(target_os = "android")]
        let (tool, hover, buttons) = {
            let p = host.stylus_probe();
            (p.tool, p.hover, p.buttons)
        };
        #[cfg(not(target_os = "android"))]
        let (tool, hover, buttons) = {
            let _ = host;
            (0u8, None::<(f32, f32)>, 0u32)
        };
        let kind = match tool {
            1 => mask::PointerKind::Finger,
            2 => mask::PointerKind::Stylus,
            3 => mask::PointerKind::Mouse,
            4 => mask::PointerKind::Eraser,
            5 => mask::PointerKind::Palm,
            _ => mask::PointerKind::Unknown,
        };
        let pressure_event = ui.input(|i| {
            i.events.iter().rev().find_map(|e| {
                if let egui::Event::Touch {
                    force,
                    phase: egui::TouchPhase::Start | egui::TouchPhase::Move,
                    ..
                } = e
                {
                    Some(*force)
                } else {
                    None
                }
            })
        });
        // Android need not send another MotionEvent while the pen is stationary.
        // Keep that contact's pressure instead of stamping at full pressure on idle frames.
        if let Some(force) = pressure_event {
            self.pressure = force;
        }
        let force = self.pressure;
        let brush = mask::pressure_brush(self.brush / self.view.zoom, force);
        let erase = self.erase || buttons & 0x60 != 0 || kind == mask::PointerKind::Eraser;
        let down = ui.input(|i| i.pointer.any_down());
        if down {
            self.contact_erase = erase;
        }
        // Button bits may already be released when egui emits the final click.
        let erase = if !down && self.last_uv.is_some() {
            self.contact_erase
        } else {
            erase
        };
        let position = response.interact_pointer_pos();
        if editable
            && !self.nav_latch
            && mask::accept_paint(kind, self.pen_only)
            && (response.dragged() || response.is_pointer_button_down_on() || response.clicked())
        {
            if let Some(p) = position.filter(|p| view.contains(*p) && area.contains(*p)) {
                let uv = (
                    (p.x - view.left()) / view.width(),
                    (p.y - view.top()) / view.height(),
                );
                if self.last_uv.is_none() {
                    self.groups.push(self.strokes.len());
                }
                let from = self.last_uv.unwrap_or(uv);
                self.canvas
                    .stroke(from, uv, brush.radius_uv, 0.45, brush.intensity, erase);
                self.saved = false;
                self.strokes.push(StrokeRec {
                    from,
                    to: uv,
                    radius_uv: brush.radius_uv,
                    soft: 0.45,
                    intensity: brush.intensity,
                    erase,
                });
                self.last_uv = Some(uv);
                self.update_overlay(ui.ctx());
            } else {
                self.last_uv = None;
            }
        }
        if response.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
            self.last_uv = None;
            self.pressure = None;
        }
        if let Some(overlay) = &self.overlay {
            ui.painter().with_clip_rect(area).image(
                overlay.id(),
                view,
                Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        let hover = hover.map(|(x, y)| pos2(x, y) / ui.ctx().pixels_per_point());
        if let Some(p) = position
            .or(hover)
            .filter(|p| area.contains(*p) && view.contains(*p))
        {
            ui.painter().with_clip_rect(area).circle_stroke(
                p,
                brush.radius_uv * view.width().min(view.height()),
                Stroke::new(1.5, if erase { TRIM } else { ACCENT }),
            );
        }
        if self.telemetry {
            ui.painter().text(
                area.left_top() + vec2(8.0, 8.0),
                Align2::LEFT_TOP,
                format!(
                    "{kind:?}   pressure {:.2}\nbuttons {:#x}   zoom {:.1}x",
                    force.unwrap_or(0.0),
                    buttons,
                    self.view.zoom
                ),
                FontId::proportional(12.0),
                TRIM,
            );
        }
    }

    fn update_overlay(&mut self, ctx: &egui::Context) {
        let pixels: Vec<Color32> = self
            .canvas
            .buf
            .iter()
            .map(|&m| Color32::from_rgba_unmultiplied(255, 61, 139, m / 2))
            .collect();
        let image = egui::ColorImage::new([self.canvas.w as usize, self.canvas.h as usize], pixels);
        if let Some(tex) = self.overlay.as_mut() {
            tex.set(image, egui::TextureOptions::LINEAR);
        } else {
            self.overlay = Some(ctx.load_texture("edit-mask", image, egui::TextureOptions::LINEAR));
        }
    }

    fn server_dialog(&mut self, ctx: &egui::Context, busy: bool) {
        if !self.server_open {
            return;
        }
        egui::Window::new("Qwen server").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER,Vec2::ZERO).fixed_size(vec2((ctx.content_rect().width()-32.0).min(400.0),300.0)).show(ctx,|ui|{
            ui.label("Use the same comfy-gate server as ComfyUI Android.");
            ui.add(egui::TextEdit::singleline(&mut self.server).hint_text("https://your-comfy-server").desired_width(f32::INFINITY));
            ui.add(egui::TextEdit::singleline(&mut self.key).password(true).hint_text("API key (blank keeps saved key)").desired_width(f32::INFINITY));
            ui.collapsing("Sign in with an account",|ui|{
                ui.add(egui::TextEdit::singleline(&mut self.username).hint_text("Username"));
                ui.add(egui::TextEdit::singleline(&mut self.password).password(true).hint_text("Password"));
                if ui.add_enabled(!busy&&!self.username.is_empty()&&!self.password.is_empty(),egui::Button::new("Sign in")).clicked(){
                    bridge::send(json!({"op":"qwen_config","session":self.session,"uri":self.item.uri,"url":self.server,"username":self.username,"password":self.password}));self.password.clear();
                }
            });
            ui.horizontal(|ui|{
                if ui.add_enabled(!busy&&!self.server.is_empty(),egui::Button::new("Save & check")).clicked(){
                    let mut request=json!({"op":"qwen_config","session":self.session,"uri":self.item.uri,"url":self.server,"check":true});
                    if !self.key.is_empty(){request["key"]=std::mem::take(&mut self.key).into();}bridge::send(request);
                }
                if ui.add_enabled(!busy,egui::Button::new("Clear sign-in")).clicked(){bridge::send(json!({"op":"qwen_config","session":self.session,"url":self.server,"clear_auth":true}));}
                if ui.button("Done").clicked(){self.server_open=false;}
            });
            ui.label(&self.status);
        });
    }
}

fn texture(ctx: &egui::Context, name: &str, pixels: &RgbaImage) -> egui::TextureHandle {
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(
            [pixels.width() as usize, pixels.height() as usize],
            pixels.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}

pub(super) fn crop_presets(ui: &mut egui::Ui, crop: &mut Crop, width: u32, height: u32) {
    ui.horizontal_wrapped(|ui| {
        for (label, ratio) in [
            ("Full", 0.0),
            ("1:1", 1.0),
            ("4:3", 4.0 / 3.0),
            ("16:9", 16.0 / 9.0),
            ("9:16", 9.0 / 16.0),
        ] {
            if ui.button(label).clicked() {
                *crop = Crop::aspect(width, height, ratio);
            }
        }
    });
}

/// Shared photo/video crop handles. Dragging the interior moves the selection without resizing.
pub(super) fn crop_overlay(
    ui: &egui::Ui,
    image: Rect,
    response: &egui::Response,
    crop: &mut Crop,
    drag: &mut Option<CropDrag>,
    enabled: bool,
) {
    let at = |x: f32, y: f32| image.min + image.size() * vec2(x, y);
    let selection = Rect::from_min_max(at(crop.left, crop.top), at(crop.right, crop.bottom));
    let painter = ui.painter().with_clip_rect(image);
    for r in [
        Rect::from_min_max(image.min, pos2(image.right(), selection.top())),
        Rect::from_min_max(pos2(image.left(), selection.bottom()), image.max),
        Rect::from_min_max(pos2(image.left(), selection.top()), selection.left_bottom()),
        Rect::from_min_max(
            selection.right_top(),
            pos2(image.right(), selection.bottom()),
        ),
    ] {
        painter.rect_filled(r, 0, Color32::from_black_alpha(165));
    }
    painter.rect_stroke(selection, 0, Stroke::new(1.5, TRIM), StrokeKind::Inside);
    let corners = [
        selection.left_top(),
        selection.right_top(),
        selection.right_bottom(),
        selection.left_bottom(),
    ];
    for p in corners {
        painter.rect_filled(Rect::from_center_size(p, vec2(10.0, 10.0)), 2, TRIM);
    }
    for part in [1.0 / 3.0, 2.0 / 3.0] {
        painter.vline(
            selection.left() + selection.width() * part,
            selection.y_range(),
            Stroke::new(0.5, Color32::from_white_alpha(100)),
        );
        painter.hline(
            selection.x_range(),
            selection.top() + selection.height() * part,
            Stroke::new(0.5, Color32::from_white_alpha(100)),
        );
    }
    if !enabled {
        return;
    }
    if response.drag_started() {
        let p = ui
            .input(|i| i.pointer.press_origin())
            .unwrap_or(image.center());
        let corner = corners
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.distance(p).total_cmp(&b.distance(p)))
            .filter(|(_, c)| c.distance(p) < 30.0)
            .map(|(n, _)| n);
        if let Some(n) = corner {
            *drag = Some(CropDrag {
                kind: n,
                base: *crop,
                origin: p,
            });
        } else if selection.contains(p) {
            *drag = Some(CropDrag {
                kind: 4,
                base: *crop,
                origin: p,
            });
        }
    }
    if let Some(CropDrag { kind, base, origin }) = *drag {
        if let Some(p) = response.interact_pointer_pos() {
            // egui clears press_origin on release, before reporting drag_stopped.
            // Retain the gesture's origin so that final frame keeps its displacement.
            let delta = (p - origin) / image.size();
            let mut next = base;
            if kind == 4 {
                let dx = delta.x.clamp(-base.left, 1.0 - base.right);
                let dy = delta.y.clamp(-base.top, 1.0 - base.bottom);
                next.left += dx;
                next.right += dx;
                next.top += dy;
                next.bottom += dy;
            } else {
                if kind == 0 || kind == 3 {
                    next.left = (base.left + delta.x).clamp(0.0, base.right - 0.02);
                } else {
                    next.right = (base.right + delta.x).clamp(base.left + 0.02, 1.0);
                }
                if kind < 2 {
                    next.top = (base.top + delta.y).clamp(0.0, base.bottom - 0.02);
                } else {
                    next.bottom = (base.bottom + delta.y).clamp(base.top + 0.02, 1.0);
                }
            }
            *crop = next;
        }
    }
    if response.drag_stopped() {
        *drag = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwen_keeps_focus_and_accepts_text_across_keyboard_reflow() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let host = Host::new();
        let mut editor = PhotoEditor::new(MediaItem::default());
        editor.tab = Tab::Qwen;
        editor.prompt = "focus marker".into();
        let mut time = 0.0;
        let mut frame = |editor: &mut PhotoEditor, size: Vec2, events| {
            time += 1.0 / 60.0;
            let mut output = ctx.run_ui(egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                time: Some(time), events, ..Default::default()
            }, |ui| { editor.ui(ui, &host, &json!({})); });
            output.textures_delta.clear();
            output
        };
        let full = vec2(400.0, 800.0);
        frame(&mut editor, full, vec![]);
        let output = frame(&mut editor, full, vec![]);
        let pos = output.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "focus marker" =>
                Some(t.pos + vec2(10.0, 5.0)),
            _ => None,
        }).expect("visible Qwen prompt");
        for pressed in [true, false] {
            frame(&mut editor, full, vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton { pos, pressed,
                    button: egui::PointerButton::Primary, modifiers: Default::default() },
            ]);
        }
        let focus = ctx.memory(|m| m.focused()).expect("prompt focused by tap");
        // Opening a tall IME, a wider split-screen viewport crossing the
        // stacked/columns breakpoint, rotation, and closing the IME.
        for size in [vec2(400.0, 480.0), vec2(400.0, 240.0), vec2(640.0, 700.0),
            vec2(640.0, 330.0), vec2(800.0, 350.0), full] {
            for _ in 0..3 { frame(&mut editor, size, vec![]); }
            assert_eq!(ctx.memory(|m| m.focused()), Some(focus), "focus at {size:?}");
            let before = editor.prompt.len();
            frame(&mut editor, size, vec![egui::Event::Text("x".into())]);
            assert_eq!(editor.prompt.len(), before + 1, "typing at {size:?}");
        }
    }

    // Exercise the real egui response, including the release frame where the
    // pointer position still exists but press_origin() has already been cleared.
    fn drag_gesture(initial: Crop, from: Pos2, to: Pos2, touch: bool) -> (Crop, Crop, Crop) {
        let ctx = egui::Context::default();
        let mut crop = initial;
        let mut drag = None;
        let mut time = 0.0;
        let mut frame = |events| {
            time += 1.0 / 60.0;
            let mut image = Rect::NOTHING;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(600.0, 400.0), Sense::click_and_drag());
                    image = rect;
                    crop_overlay(ui, image, &response, &mut crop, &mut drag, true);
                },
            );
            output.textures_delta.clear();
            (crop, image, drag.is_some())
        };
        frame(vec![]);
        let (_, image, _) = frame(vec![]);
        let events = |phase: egui::TouchPhase, relative: Pos2| {
            let pos = image.min + relative.to_vec2();
            let mut events = vec![egui::Event::PointerMoved(pos)];
            if touch {
                events.push(egui::Event::Touch {
                    device_id: egui::TouchDeviceId(0),
                    id: egui::TouchId(1),
                    phase,
                    pos,
                    force: None,
                });
            }
            if phase != egui::TouchPhase::Move {
                events.push(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: phase == egui::TouchPhase::Start,
                    modifiers: Default::default(),
                });
            }
            if touch && phase == egui::TouchPhase::End {
                events.push(egui::Event::PointerGone);
            }
            events
        };
        frame(events(egui::TouchPhase::Start, from));
        frame(events(egui::TouchPhase::Move, from.lerp(to, 0.5)));
        let (held, _, active) = frame(events(egui::TouchPhase::Move, to));
        assert!(
            active,
            "The gesture must grab a crop handle or its interior"
        );
        let (released, _, active) = frame(events(egui::TouchPhase::End, to));
        assert!(!active, "Release must finish the gesture");
        let (idle, _, _) = frame(vec![]);
        (held, released, idle)
    }

    #[test]
    fn crop_resize_stays_at_all_four_corners_after_release() {
        let initial = Crop {
            left: 0.2,
            top: 0.2,
            right: 0.8,
            bottom: 0.8,
        };
        for touch in [false, true] {
            for (from, to) in [
                (pos2(120.0, 80.0), pos2(180.0, 120.0)),
                (pos2(480.0, 80.0), pos2(420.0, 120.0)),
                (pos2(480.0, 320.0), pos2(420.0, 280.0)),
                (pos2(120.0, 320.0), pos2(180.0, 280.0)),
            ] {
                let (held, released, idle) = drag_gesture(initial, from, to, touch);
                assert!((held.width() - 0.5).abs() < 0.00001);
                assert!((held.height() - 0.5).abs() < 0.00001);
                assert_eq!(released, held, "Lifting the pointer must not undo a resize");
                assert_eq!(idle, held);
            }
        }
    }

    #[test]
    fn square_crop_keeps_its_position_after_release_and_at_image_edge() {
        let initial = Crop::aspect(600, 400, 1.0);
        for touch in [false, true] {
            for (to, left) in [(pos2(240.0, 200.0), 1.0 / 15.0), (pos2(-50.0, 200.0), 0.0)] {
                let (held, released, idle) = drag_gesture(initial, pos2(300.0, 200.0), to, touch);
                assert!((held.left - left).abs() < 0.00001);
                assert_eq!(held.pixels(600, 400).2, 400);
                assert_eq!(held.pixels(600, 400).3, 400);
                assert_eq!(
                    released, held,
                    "Lifting the pointer must not recenter the crop"
                );
                assert_eq!(idle, held);
            }
        }
    }
}
