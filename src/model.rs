use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct MediaItem {
    pub uri: String,
    pub name: String,
    pub album: String,
    pub kind: String,
    pub width: u32,
    pub height: u32,
    pub duration: i64,
    pub size: u64,
    pub modified: u64,
    /// Seconds since the epoch when a trashed item is purged; zero outside the trash.
    pub expires: u64,
    /// Parent document of an item inside a linked folder's subfolder.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub folder: String,
}

impl MediaItem {
    pub fn is_video(&self) -> bool {
        self.kind == "video"
    }
    pub fn is_media_store(&self) -> bool {
        self.uri.starts_with("content://media/")
    }
    pub fn is_trashed(&self) -> bool {
        self.expires > 0
    }
    pub fn thumb_key(&self, at: i64) -> String {
        format!("{}:{}:{}:{at}", self.uri, self.modified, self.size)
    }
}

pub fn plural(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

/// Multi-selection over list keys, with press-and-drag range gestures.
#[derive(Default)]
pub struct Selection {
    pub active: bool,
    pub picked: HashSet<String>,
    drag: Option<Drag>,
}

struct Drag {
    anchor: usize,
    adding: bool,
    base: HashSet<String>,
}

impl Selection {
    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn contains(&self, key: &str) -> bool {
        self.picked.contains(key)
    }
    pub fn toggle(&mut self, key: &str) {
        if !self.picked.remove(key) {
            self.picked.insert(key.to_owned());
        }
    }
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
    /// Starts a range gesture at `index`; starting on a picked key deselects the range instead.
    pub fn begin_drag(&mut self, keys: &[String], index: usize) {
        let Some(key) = keys.get(index) else {
            return;
        };
        self.active = true;
        self.drag = Some(Drag {
            anchor: index,
            adding: !self.picked.contains(key),
            base: self.picked.clone(),
        });
        self.drag_to(keys, index);
    }
    pub fn drag_to(&mut self, keys: &[String], index: usize) {
        let Some(drag) = &self.drag else {
            return;
        };
        let mut picked = drag.base.clone();
        let range = drag.anchor.min(index)..=drag.anchor.max(index);
        for key in keys.iter().enumerate().filter(|(i, _)| range.contains(i)) {
            if drag.adding {
                picked.insert(key.1.clone());
            } else {
                picked.remove(key.1);
            }
        }
        self.picked = picked;
    }
    pub fn end_drag(&mut self) {
        self.drag = None;
    }
    /// Picks every key, or unpicks them all when every key is already picked.
    pub fn toggle_all(&mut self, keys: &[String]) {
        if keys.iter().all(|key| self.picked.contains(key)) {
            for key in keys {
                self.picked.remove(key);
            }
        } else {
            self.picked.extend(keys.iter().cloned());
        }
    }
}

/// Splits a file name into stem and extension, keeping the dot with the extension.
pub fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    }
}

/// New names for `names`: one keeps its extension, several are numbered in list order.
pub fn sequence_names(base: &str, names: &[&str]) -> Vec<String> {
    let base = base.trim();
    if let [name] = names {
        return vec![format!("{base}{}", split_name(name).1)];
    }
    let width = names.len().to_string().len().max(3);
    names
        .iter()
        .enumerate()
        .map(|(i, name)| format!("{base}_{:0width$}{}", i + 1, split_name(name).1))
        .collect()
}

/// Why Android storage would refuse or rewrite a file or folder name.
pub fn name_problem(name: &str) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        Some("Enter a name")
    } else if name.starts_with('.') {
        Some("Names cannot start with a dot")
    } else if name
        .chars()
        .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
    {
        Some("Names cannot contain \\ / : * ? \" < > |")
    } else if name.len() > 200 {
        Some("Name is too long")
    } else {
        None
    }
}

/// Storage path for a new album; input containing slashes is a full path.
pub fn album_path(input: &str) -> String {
    let parts: Vec<&str> = input
        .split('/')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    match parts.as_slice() {
        [name] => format!("Pictures/{name}"),
        _ => parts.join("/"),
    }
}

/// Why `album` cannot hold media of `kinds`; MediaStore limits each kind's top-level folders.
pub fn album_problem<'a>(album: &str, kinds: impl IntoIterator<Item = &'a str>) -> Option<String> {
    if let Some(problem) = album.split('/').find_map(name_problem) {
        return Some(problem.into());
    }
    let primary = album.split('/').next().unwrap_or_default();
    for kind in kinds {
        let allowed: &[&str] = if kind == "video" {
            &["DCIM", "Movies", "Pictures"]
        } else {
            &["DCIM", "Pictures"]
        };
        if !allowed.iter().any(|a| a.eq_ignore_ascii_case(primary)) {
            return Some(format!(
                "{} must be inside {}",
                if kind == "video" { "Videos" } else { "Photos" },
                allowed.join(", ")
            ));
        }
    }
    None
}

/// Album path after renaming its last folder; top-level folders cannot be renamed.
pub fn renamed_album(album: &str, name: &str) -> Option<String> {
    let (parent, _) = album.rsplit_once('/')?;
    Some(format!("{parent}/{}", name.trim()))
}

pub fn timecode(ms: i64, precise: bool) -> String {
    let ms = ms.max(0);
    let seconds = ms / 1000;
    let base = if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    };
    if precise {
        format!("{base}.{:03}", ms % 1000)
    } else {
        base
    }
}

pub fn size_label(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1e9)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1e6)
    } else {
        format!("{} KB", bytes.div_ceil(1000))
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub enum DragKind {
    #[default]
    Seek,
    In,
    Out,
}

pub struct Timeline {
    pub start: i64,
    pub end: i64,
    pub position: i64,
    pub fps: f64,
    pub zoom: f64,
    pub window_start: i64,
    pub dragging: Option<DragKind>,
    pub drag_origin: Option<(egui::Pos2, i64)>,
    pub resume_after_drag: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            start: 0,
            end: 0,
            position: 0,
            fps: 30.0,
            zoom: 1.0,
            window_start: 0,
            dragging: None,
            drag_origin: None,
            resume_after_drag: false,
        }
    }
}

impl Timeline {
    pub fn frame_ms(&self) -> i64 {
        (1000.0 / self.fps.max(1.0)).round().max(1.0) as i64
    }
    pub fn step_time(&self, frames: i32, duration: i64) -> i64 {
        let fps = self.fps.max(1.0);
        let frame = (self.position as f64 * fps / 1000.0).round() + f64::from(frames);
        (frame * 1000.0 / fps)
            .round()
            .clamp(0.0, duration.max(0) as f64) as i64
    }
    pub fn window(&self, duration: i64) -> (i64, i64) {
        let span = ((duration as f64 / self.zoom).round() as i64)
            .max(1000)
            .min(duration.max(1));
        let start = self.window_start.clamp(0, (duration - span).max(0));
        (start, start + span)
    }
    pub fn zoom_at(&mut self, duration: i64, factor: f64) {
        self.zoom = (self.zoom * factor).clamp(1.0, 64.0);
        self.window_start = (self.position - (duration as f64 / self.zoom / 2.0) as i64).max(0);
    }
    pub fn set_mark(&mut self, kind: DragKind, time: i64, duration: i64) {
        let duration = duration.max(0);
        let time = time.clamp(0, duration);
        let frame = self.frame_ms();
        match kind {
            DragKind::In => self.start = time.min((self.end - frame).max(0)),
            DragKind::Out => self.end = time.max(self.start + frame).min(duration),
            DragKind::Seek => self.position = time,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_frame_steps_do_not_accumulate_rounding_error() {
        let mut timeline = Timeline {
            fps: 30_000.0 / 1001.0,
            ..Timeline::default()
        };
        for _ in 0..60 {
            timeline.position = timeline.step_time(1, 10_000);
        }
        assert_eq!(timeline.position, 2002);
        for _ in 0..60 {
            timeline.position = timeline.step_time(-1, 10_000);
        }
        assert_eq!(timeline.position, 0);
        assert_eq!(timeline.step_time(-1, 10_000), 0);
        timeline.position = 9990;
        assert_eq!(timeline.step_time(1, 10_000), 10_000);
    }

    #[test]
    fn trim_marks_remain_ordered_and_inside_clip() {
        let mut timeline = Timeline {
            end: 10_000,
            ..Timeline::default()
        };
        timeline.set_mark(DragKind::In, 20_000, 10_000);
        assert_eq!(timeline.start, 10_000 - timeline.frame_ms());
        timeline.set_mark(DragKind::Out, -10, 10_000);
        assert_eq!(timeline.end, 10_000);
        timeline.set_mark(DragKind::In, -10, 10_000);
        timeline.set_mark(DragKind::Out, -10, 10_000);
        assert_eq!((timeline.start, timeline.end), (0, timeline.frame_ms()));
        timeline.set_mark(DragKind::Seek, 20_000, 10_000);
        assert_eq!(timeline.position, 10_000);
        timeline.set_mark(DragKind::Seek, -1, 0);
        assert_eq!(timeline.position, 0);
    }

    #[test]
    fn zoom_window_is_bounded_for_short_and_empty_clips() {
        let mut timeline = Timeline {
            position: 9800,
            ..Timeline::default()
        };
        timeline.zoom_at(10_000, 4.0);
        assert_eq!(timeline.window(10_000), (7500, 10_000));
        timeline.zoom_at(10_000, 100.0);
        assert_eq!(timeline.zoom, 64.0);
        assert_eq!(timeline.window(10_000), (9000, 10_000));
        assert_eq!(timeline.window(300), (0, 300));
        assert_eq!(timeline.window(0), (0, 1));
        timeline.zoom_at(10_000, 0.0);
        assert_eq!(timeline.window(10_000), (0, 10_000));
    }

    #[test]
    fn time_labels_keep_milliseconds_and_hours() {
        assert_eq!(timecode(-1, true), "00:00.000");
        assert_eq!(timecode(8129, true), "00:08.129");
        assert_eq!(timecode(3_661_002, true), "1:01:01.002");
        assert_eq!(timecode(61_900, false), "01:01");
    }

    fn keys(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("k{i}")).collect()
    }

    fn picked(selection: &Selection) -> Vec<String> {
        let mut keys: Vec<_> = selection.picked.iter().cloned().collect();
        keys.sort();
        keys
    }

    #[test]
    fn drag_selects_ranges_in_both_directions_and_restores_skipped_keys() {
        let keys = keys(8);
        let mut selection = Selection::default();
        selection.toggle("k7");
        selection.begin_drag(&keys, 2);
        assert!(selection.active && selection.dragging());
        selection.drag_to(&keys, 5);
        assert_eq!(picked(&selection), ["k2", "k3", "k4", "k5", "k7"]);
        selection.drag_to(&keys, 0);
        assert_eq!(picked(&selection), ["k0", "k1", "k2", "k7"]);
        selection.drag_to(&keys, 99);
        assert_eq!(picked(&selection), ["k2", "k3", "k4", "k5", "k6", "k7"]);
        selection.end_drag();
        // Starting on a picked key removes the range and keeps the rest.
        selection.begin_drag(&keys, 4);
        selection.drag_to(&keys, 6);
        selection.end_drag();
        assert_eq!(picked(&selection), ["k2", "k3", "k7"]);
        selection.begin_drag(&keys, 50);
        assert!(!selection.dragging());
    }

    #[test]
    fn toggle_all_clears_only_when_everything_is_picked() {
        let keys = keys(3);
        let mut selection = Selection::default();
        selection.toggle("k1");
        selection.toggle_all(&keys);
        assert_eq!(picked(&selection), ["k0", "k1", "k2"]);
        selection.toggle("other");
        selection.toggle_all(&keys);
        assert_eq!(picked(&selection), ["other"]);
        selection.clear();
        assert!(!selection.active && selection.picked.is_empty());
    }

    #[test]
    fn renames_keep_extensions_and_number_batches() {
        assert_eq!(split_name("IMG_1.JPG"), ("IMG_1", ".JPG"));
        assert_eq!(split_name("clip.final.mp4"), ("clip.final", ".mp4"));
        assert_eq!(split_name(".nomedia"), (".nomedia", ""));
        assert_eq!(split_name("README"), ("README", ""));
        assert_eq!(sequence_names(" Trip ", &["a.ARW"]), ["Trip.ARW"]);
        assert_eq!(
            sequence_names("Trip", &["a.jpg", "b.mp4"]),
            ["Trip_001.jpg", "Trip_002.mp4"]
        );
        let many: Vec<&str> = std::iter::repeat_n("x.dng", 1200).collect();
        assert_eq!(sequence_names("S", &many)[1199], "S_1200.dng");
        assert_eq!(name_problem("  "), Some("Enter a name"));
        assert!(name_problem("a/b").is_some() && name_problem("a:b").is_some());
        assert!(name_problem(".hidden").is_some() && name_problem("a\tb").is_some());
        assert_eq!(name_problem(" Summer 2026 "), None);
    }

    #[test]
    fn album_targets_follow_media_store_folder_rules() {
        assert_eq!(album_path(" Trips "), "Pictures/Trips");
        assert_eq!(album_path("/DCIM//Trips/"), "DCIM/Trips");
        assert_eq!(album_problem("Pictures/Trips", ["photo", "video"]), None);
        assert_eq!(album_problem("dcim/Camera", ["raw"]), None);
        assert_eq!(album_problem("Movies/Luma", ["video"]), None);
        assert!(album_problem("Movies/Luma", ["video", "photo"]).is_some());
        assert!(album_problem("Download/Saved", ["photo"]).is_some());
        assert!(album_problem("Trips", ["photo"]).is_some());
        assert_eq!(album_problem("Pictures", ["photo"]), None);
        assert!(album_problem("Pictures/a:b", ["photo"]).is_some());
        assert_eq!(
            renamed_album("DCIM/Old", " New "),
            Some("DCIM/New".to_owned())
        );
        assert_eq!(renamed_album("Pictures", "New"), None);
    }

    #[test]
    fn library_items_report_storage_and_trash_state() {
        let mut item = MediaItem {
            uri: "content://media/external/images/media/4".into(),
            ..MediaItem::default()
        };
        assert!(item.is_media_store() && !item.is_trashed());
        item.expires = 1;
        assert!(item.is_trashed());
        item.uri = "content://com.android.externalstorage.documents/tree/x".into();
        assert!(!item.is_media_store());
        assert_eq!(plural(1, "item"), "1 item");
        assert_eq!(size_label(25_350), "26 KB");
        assert_eq!(size_label(16_759_778), "16.8 MB");
        assert_eq!(size_label(2_500_000_000), "2.5 GB");
        assert_eq!(plural(2, "album"), "2 albums");
        let json = serde_json::to_value(&item).unwrap();
        assert!(json.get("folder").is_none() && json["expires"] == 1);
    }

    #[test]
    fn thumbnails_invalidate_after_media_changes() {
        let mut item = MediaItem {
            uri: "content://media/7".into(),
            size: 12,
            modified: 34,
            ..MediaItem::default()
        };
        let key = item.thumb_key(0);
        assert_ne!(key, item.thumb_key(1000));
        item.modified += 1;
        assert_ne!(key, item.thumb_key(0));
    }
}
