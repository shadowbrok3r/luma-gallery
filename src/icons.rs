use egui::{Color32, Image, Rect, Response, Ui, Vec2};
use std::cell::RefCell;
use std::sync::OnceLock;

thread_local! {
    /// Enabled controls drawn this pass: label and pixel bounds.
    static SHOWN: RefCell<Vec<(String, [i32; 4])>> = const { RefCell::new(vec![]) };
    static LOGGED: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Whether the system property `debug.luma.automation` is 1.
pub fn automation() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        #[cfg(target_os = "android")]
        {
            unsafe extern "C" {
                fn __system_property_get(name: *const std::ffi::c_char, value: *mut std::ffi::c_char) -> i32;
            }
            let mut value = [0 as std::ffi::c_char; 92];
            let length = unsafe { __system_property_get(c"debug.luma.automation".as_ptr(), value.as_mut_ptr()) };
            length == 1 && value[0] == b'1' as std::ffi::c_char
        }
        #[cfg(not(target_os = "android"))]
        false
    })
}

/// Records a labeled control for the automation log.
pub fn note(ui: &Ui, label: &str, rect: Rect) {
    if !automation() || !ui.is_enabled() || !ui.is_rect_visible(rect) {
        return;
    }
    let r = rect.intersect(ui.clip_rect()) * ui.ctx().pixels_per_point();
    let bounds = [r.left(), r.top(), r.right(), r.bottom()].map(|v| v.round() as i32);
    SHOWN.with(|shown| shown.borrow_mut().push((label.to_owned(), bounds)));
}

/// Starts a pass's automation record.
pub fn begin() {
    SHOWN.with(|shown| shown.borrow_mut().clear());
}

/// Logs the pass's controls after a `LumaWidgets` prefix when they changed.
pub fn report() {
    if !automation() {
        return;
    }
    let text = SHOWN.with(|shown| serde_json::to_string(&*shown.borrow()).unwrap_or_default());
    LOGGED.with(|logged| {
        if *logged.borrow() != text {
            log::info!("LumaWidgets {text}");
            *logged.borrow_mut() = text;
        }
    });
}

pub fn source(name: &str) -> &'static [u8] {
    macro_rules! icons { ($($key:literal),* $(,)?) => { match name { $($key => include_bytes!(concat!("../assets/icons/", $key, ".svg")),)* _ => include_bytes!("../assets/icons/info.svg") } }; }
    icons!(
        "back",
        "add",
        "folder",
        "refresh",
        "settings",
        "albums",
        "grid",
        "star",
        "video",
        "photo",
        "camera",
        "raw",
        "search",
        "sort",
        "play",
        "pause",
        "previous",
        "next",
        "step_back",
        "step_forward",
        "volume",
        "mute",
        "loop",
        "cut",
        "save",
        "share",
        "info",
        "close",
        "zoom_in",
        "zoom_out",
        "fit",
        "in_mark",
        "out_mark",
        "chip",
        "chevron",
        "check",
        "delete",
        "move",
        "copy",
        "edit",
        "more",
        "select",
        "select_all",
        "restore",
        "crop",
        "rotate",
        "flip",
        "brush",
        "eraser",
        "undo",
        "sparkles",
        "waveform",
        "speed",
        "adjust",
        "text",
        "layers",
        "eye",
        "eye_off",
        "lock",
        "unlock",
        "redo",
        "fade",
        "align_left",
        "align_center",
        "align_right",
        "captions"
    )
}

pub fn image(name: &str, color: Color32, size: f32) -> Image<'static> {
    Image::from_bytes(format!("bytes://luma/{name}.svg"), source(name))
        .fit_to_exact_size(Vec2::splat(size))
        .tint(color)
}

pub fn button(ui: &mut Ui, name: &str, label: &str, active: bool) -> Response {
    let glow = ui.painter().add(egui::Shape::Noop);
    let tint = if active { crate::ACCENT } else { crate::TEXT };
    let mut button = egui::Button::image(image(name, tint, 21.0))
        .min_size(egui::vec2(40.0, 40.0))
        .frame(active);
    if active {
        button = button.fill(crate::PANEL);
    }
    let response = ui.add(button).on_hover_text(label);
    if active && ui.is_enabled() {
        ui.painter().set(glow, selection_glow(response.rect));
        ui.painter().rect_stroke(
            response.rect,
            5,
            egui::Stroke::new(1.0, crate::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label)
    });
    note(ui, label, response.rect);
    response
}

pub fn selection_glow(rect: egui::Rect) -> egui::epaint::RectShape {
    egui::Shadow {
        offset: [0, 0],
        blur: 6,
        spread: 1,
        color: crate::ACCENT.gamma_multiply(0.5),
    }
    .as_shape(rect, 5)
}
