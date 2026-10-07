use egui::{Color32, Image, Response, Ui, Vec2};

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
        "waveform"
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
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
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
