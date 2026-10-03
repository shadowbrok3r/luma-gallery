mod app;
mod bridge;
mod icons;
mod model;

use egui::Color32;
const BG: Color32 = Color32::BLACK;
const PANEL: Color32 = Color32::from_rgb(16, 11, 22);
const LINE: Color32 = Color32::from_rgb(62, 35, 51);
const TEXT: Color32 = Color32::from_rgb(233, 233, 239);
const MUTED: Color32 = Color32::from_rgb(161, 155, 174);
const ACCENT: Color32 = Color32::from_rgb(255, 61, 139);
const TRIM: Color32 = Color32::from_rgb(43, 226, 214);

#[cfg(target_os = "android")]
egui_mobile::app!(app::Gallery::new, egui_mobile::Backend::Glow);
