use super::{Gallery, View, action_bar, text_at};
use crate::{ACCENT, MUTED, TEXT, bridge, icons, model::*};
use egui::{Align, Align2, Color32, Rect, Sense, Vec2, vec2};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::Instant;

pub(super) enum Dialog {
    Target {
        copy: bool,
        items: Vec<MediaItem>,
        name: String,
    },
    Rename {
        items: Vec<MediaItem>,
        name: String,
        focus: bool,
    },
    RenameAlbum {
        album: String,
        items: Vec<MediaItem>,
        name: String,
        focus: bool,
    },
    Delete {
        items: Vec<MediaItem>,
        action: &'static str,
        title: String,
        detail: String,
    },
}

impl Dialog {
    pub(super) fn target(copy: bool, items: Vec<MediaItem>) -> Self {
        Self::Target {
            copy,
            items,
            name: String::new(),
        }
    }
    pub(super) fn rename(items: Vec<MediaItem>) -> Self {
        let name = match items.as_slice() {
            [item] => split_name(&item.name).0.to_owned(),
            _ => String::new(),
        };
        Self::Rename {
            items,
            name,
            focus: true,
        }
    }
}

fn button(ui: &mut egui::Ui, icon: &str, label: &str, color: Color32, enabled: bool) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::image_and_text(icons::image(icon, color, 18.0), label)
            .min_size(vec2(120.0, 38.0)),
    )
    .clicked()
}

/// Fixed-width dialog; text-entry dialogs sit near the top, clear of the keyboard.
fn window(title: impl Into<egui::WidgetText>, width: f32, typing: bool) -> egui::Window<'static> {
    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(
            if typing {
                Align2::CENTER_TOP
            } else {
                Align2::CENTER_CENTER
            },
            vec2(0.0, if typing { 64.0 } else { 0.0 }),
        )
        .min_width(width)
        .max_width(width)
}

impl Gallery {
    pub(super) fn selection_header(&mut self, ui: &mut egui::Ui, list: &[MediaItem]) {
        let (keys, label) = if self.view == View::Albums && self.album.is_none() {
            let albums: Vec<String> = list
                .iter()
                .map(|item| item.album.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let picked = albums.iter().filter(|a| self.selection.contains(a)).count();
            let items = list
                .iter()
                .filter(|item| self.selection.contains(&item.album))
                .count();
            let label = if picked == 0 {
                "Select albums".to_owned()
            } else {
                format!("{} · {}", plural(picked, "album"), plural(items, "item"))
            };
            (albums, label)
        } else {
            let (count, bytes) = list
                .iter()
                .filter(|item| self.selection.contains(&item.uri))
                .fold((0, 0), |(count, bytes), item| (count + 1, bytes + item.size));
            let label = if count == 0 {
                "Select items".to_owned()
            } else {
                format!("{count} selected · {}", size_label(bytes))
            };
            (list.iter().map(|item| item.uri.clone()).collect(), label)
        };
        ui.horizontal(|ui| {
            if icons::button(ui, "close", "Cancel selection", false).clicked() {
                self.selection.clear();
            }
            ui.add_sized(
                [(ui.available_width() - 46.0).max(40.0), 40.0],
                egui::Label::new(egui::RichText::new(label).size(18.0).strong())
                    .halign(Align::Min)
                    .truncate(),
            );
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let all = !keys.is_empty() && keys.iter().all(|key| self.selection.contains(key));
                let label = if all { "Deselect all" } else { "Select all" };
                if icons::button(ui, "select_all", label, all).clicked() {
                    self.selection.toggle_all(&keys);
                }
            });
        });
    }

    pub(super) fn media_actions(&mut self, ui: &mut egui::Ui, list: &[MediaItem]) {
        let items: Vec<MediaItem> = list
            .iter()
            .filter(|item| self.selection.contains(&item.uri))
            .cloned()
            .collect();
        let any = !items.is_empty();
        let ready = any && !self.busy();
        let tapped = if self.view == View::Trash {
            action_bar(
                ui,
                &[("restore", "Restore", ready), ("delete", "Delete", ready)],
            )
        } else {
            let favorite = any && items.iter().all(|item| self.favorites.contains(&item.uri));
            action_bar(
                ui,
                &[
                    ("share", "Share", any),
                    ("star", if favorite { "Unfavorite" } else { "Favorite" }, any),
                    ("move", "Move", ready),
                    ("copy", "Copy", ready),
                    ("edit", "Rename", ready),
                    ("delete", "Delete", ready),
                ],
            )
        };
        match tapped {
            Some("restore") => self.send_manage("restore", &items, json!({})),
            Some("delete") => self.delete(items),
            Some("share") => bridge::send(
                json!({"op":"share_many","uris":items.iter().map(|item| &item.uri).collect::<Vec<_>>()}),
            ),
            Some("star") => {
                let add = !items.iter().all(|item| self.favorites.contains(&item.uri));
                for item in &items {
                    if add {
                        self.favorites.insert(item.uri.clone());
                    } else {
                        self.favorites.remove(&item.uri);
                    }
                }
                self.save();
                self.selection.clear();
                self.notice(if add {
                    format!("Added {} to favorites", plural(items.len(), "item"))
                } else {
                    format!("Removed {} from favorites", plural(items.len(), "item"))
                });
            }
            Some("move") => self.dialog = Some(Dialog::target(false, items)),
            Some("copy") => self.dialog = Some(Dialog::target(true, items)),
            Some("edit") => self.dialog = Some(Dialog::rename(items)),
            _ => {}
        }
    }

    pub(super) fn album_actions(&mut self, ui: &mut egui::Ui, albums: &[(String, Vec<MediaItem>)]) {
        let picked: Vec<&(String, Vec<MediaItem>)> = albums
            .iter()
            .filter(|(album, _)| self.selection.contains(album))
            .collect();
        let ready = !picked.is_empty() && !self.busy();
        let tapped = action_bar(
            ui,
            &[
                ("edit", "Rename", ready && picked.len() == 1),
                ("move", "Move", ready),
                ("delete", "Delete", ready),
            ],
        );
        let items = || picked.iter().flat_map(|(_, items)| items.iter().cloned()).collect();
        match tapped {
            Some("edit") => {
                let album = picked[0].0.clone();
                self.dialog = Some(Dialog::RenameAlbum {
                    name: album.rsplit('/').next().unwrap_or(&album).to_owned(),
                    items: self
                        .items
                        .iter()
                        .filter(|item| item.album == album)
                        .cloned()
                        .collect(),
                    album,
                    focus: true,
                });
            }
            Some("move") => self.dialog = Some(Dialog::target(false, items())),
            Some("delete") => self.delete(items()),
            _ => {}
        }
    }

    /// Trashes library items; permanent deletions are confirmed in a dialog first.
    pub(super) fn delete(&mut self, items: Vec<MediaItem>) {
        if items.is_empty() {
            return;
        }
        let count = plural(items.len(), "item");
        let permanent = format!("{count} will be permanently deleted. This can't be undone.");
        if items.iter().any(MediaItem::is_trashed) {
            self.dialog = Some(Dialog::Delete {
                title: "Delete forever?".into(),
                detail: permanent,
                action: "delete",
                items,
            });
            return;
        }
        let linked = items.iter().filter(|item| !item.is_media_store()).count();
        if self.trash_supported() && linked == 0 {
            self.send_manage("trash", &items, json!({}));
            return;
        }
        let detail = if self.trash_supported() && linked < items.len() {
            format!(
                "{} from linked folders will be permanently deleted. The rest move to the trash.",
                plural(linked, "item")
            )
        } else {
            permanent
        };
        self.dialog = Some(Dialog::Delete {
            title: format!("Delete {count}?"),
            detail,
            action: if self.trash_supported() {
                "trash"
            } else {
                "delete"
            },
            items,
        });
    }

    pub(super) fn empty_trash(&mut self) {
        self.dialog = Some(Dialog::Delete {
            title: "Empty trash?".into(),
            detail: format!(
                "{} will be permanently deleted. This can't be undone.",
                plural(self.trash.len(), "item")
            ),
            action: "delete",
            items: self.trash.clone(),
        });
    }

    pub(super) fn send_manage(&mut self, action: &str, items: &[MediaItem], extra: Value) {
        if items.is_empty() {
            return;
        }
        let mut request = json!({"op":"manage","action":action,"items":items});
        if let (Some(request), Value::Object(extra)) = (request.as_object_mut(), extra) {
            request.extend(extra);
        }
        bridge::send(request);
        self.last_change = json!({"action": action, "pending": items.len()});
    }

    /// Applies a finished change: favorites, the open item, selection and a summary notice.
    pub(super) fn managed(&mut self, event: &Value) {
        let action = event["action"].as_str().unwrap_or_default();
        let count = |key: &str| event[key].as_u64().unwrap_or(0) as usize;
        let (done, failed) = (count("done"), count("failed"));
        let uris: HashSet<&str> = event["uris"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for (old, new) in event["renamed"].as_object().into_iter().flatten() {
            let Some(new) = new.as_str() else {
                continue;
            };
            if self.favorites.remove(old) {
                self.favorites.insert(new.to_owned());
            }
            if let Some(item) = self.selected.as_mut().filter(|item| &item.uri == old) {
                item.uri = new.to_owned();
            }
        }
        if action == "delete" {
            self.favorites.retain(|uri| !uris.contains(uri.as_str()));
        }
        self.save();
        if done > 0 {
            self.selection.clear();
        }
        if matches!(action, "trash" | "delete" | "restore")
            && self
                .selected
                .as_ref()
                .is_some_and(|item| uris.contains(item.uri.as_str()))
        {
            self.leave_item(&uris);
        }
        let target = event["target"].as_str().unwrap_or_default();
        let album = target.rsplit('/').next().unwrap_or(target);
        let what = plural(done, "item");
        let linked = self
            .items
            .iter()
            .filter(|item| !item.is_media_store() && uris.contains(item.uri.as_str()))
            .count();
        let (verb, mut text) = match action {
            "trash" if linked == done => ("delete", format!("Deleted {what}")),
            "trash" if linked > 0 => (
                "delete",
                format!(
                    "Moved {} to the trash and deleted {linked}",
                    plural(done - linked, "item")
                ),
            ),
            "trash" => ("move to the trash", format!("Moved {what} to the trash")),
            "restore" => ("restore", format!("Restored {what}")),
            "delete" => ("delete", format!("Deleted {what}")),
            "move" => ("move", format!("Moved {what} to {album}")),
            "copy" => ("copy", format!("Copied {what} to {album}")),
            "rename" => ("rename", format!("Renamed {what}")),
            "rename_album" => ("rename the album", format!("Renamed the album to {album}")),
            _ => ("change", format!("Changed {what}")),
        };
        let error = event["error"].as_str().unwrap_or_default();
        let cancelled = event["cancelled"] == true;
        if done == 0 && cancelled {
            text = "Cancelled".into();
        } else if done == 0 && failed > 0 {
            text = format!("Could not {verb}: {error}");
        } else {
            if cancelled {
                text.push_str("; the rest was cancelled");
            }
            if failed > 0 {
                text.push_str(&format!(". {} failed: {error}", plural(failed, "item")));
            }
        }
        let undo = if action == "trash" {
            self.items
                .iter()
                .filter(|item| item.is_media_store() && uris.contains(item.uri.as_str()))
                .cloned()
                .collect()
        } else {
            vec![]
        };
        self.last_change = event.clone();
        self.toast = Some((text, Instant::now(), undo));
    }

    /// Opens the next remaining item after the open one was removed, or closes the viewer.
    fn leave_item(&mut self, gone: &HashSet<&str>) {
        let list = self.visible_items();
        let next = self
            .selected
            .as_ref()
            .and_then(|selected| list.iter().position(|item| item.uri == selected.uri))
            .and_then(|index| {
                list[index + 1..]
                    .iter()
                    .chain(list[..index].iter().rev())
                    .find(|item| !gone.contains(item.uri.as_str()))
                    .cloned()
            });
        match next {
            Some(item) => self.open(item),
            None => self.close_viewer(),
        }
    }

    pub(super) fn refresh_selected(&mut self) {
        let Some(selected) = self.selected.as_mut() else {
            return;
        };
        if let Some(fresh) = self
            .items
            .iter()
            .chain(&self.trash)
            .find(|item| item.uri == selected.uri)
        {
            selected.name.clone_from(&fresh.name);
            selected.album.clone_from(&fresh.album);
            selected.expires = fresh.expires;
        }
    }

    pub(super) fn manage_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let width = (ctx.content_rect().width() - 32.0).min(440.0);
        let open = match &mut dialog {
            Dialog::Target { copy, items, name } => {
                self.target_dialog(ctx, width, *copy, items, name)
            }
            Dialog::Rename { items, name, focus } => {
                self.rename_dialog(ctx, width, items, name, focus)
            }
            Dialog::RenameAlbum {
                album,
                items,
                name,
                focus,
            } => self.rename_album_dialog(ctx, width, album, items, name, focus),
            Dialog::Delete {
                items,
                action,
                title,
                detail,
            } => self.delete_dialog(ctx, width, items, action, title, detail),
        };
        if open && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }

    fn target_dialog(
        &mut self,
        ctx: &egui::Context,
        width: f32,
        copy: bool,
        items: &[MediaItem],
        name: &mut String,
    ) -> bool {
        let kinds: BTreeSet<&str> = items.iter().map(|item| item.kind.as_str()).collect();
        let mut albums: BTreeMap<&str, (usize, &MediaItem)> = BTreeMap::new();
        for item in self.items.iter().filter(|item| item.is_media_store()) {
            albums.entry(item.album.as_str()).or_insert((0, item)).0 += 1;
        }
        let targets: Vec<(String, usize, MediaItem, Option<String>)> = albums
            .into_iter()
            .map(|(album, (count, cover))| {
                let problem = album_problem(album, kinds.iter().copied()).or_else(|| {
                    (!copy && items.iter().all(|item| item.album == album))
                        .then(|| "Already in this album".to_owned())
                });
                (album.to_owned(), count, cover.clone(), problem)
            })
            .collect();
        let path = album_path(name);
        let problem = album_problem(&path, kinds.iter().copied());
        let height = (ctx.content_rect().height() - 220.0).clamp(120.0, 420.0);
        let (mut open, mut choice) = (true, None);
        let verb = if copy { "Copy" } else { "Move" };
        window(format!("{verb} {}", plural(items.len(), "item")), width, true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add_sized(
                    [(ui.available_width() - 96.0).max(80.0), 36.0],
                    egui::TextEdit::singleline(name).hint_text("New album name"),
                );
                if ui
                    .add_enabled(
                        !name.trim().is_empty() && problem.is_none(),
                        egui::Button::image_and_text(icons::image("add", ACCENT, 18.0), "Create")
                            .min_size(vec2(90.0, 36.0)),
                    )
                    .clicked()
                {
                    choice = Some(path.clone());
                }
            });
            if !name.trim().is_empty() {
                match &problem {
                    Some(problem) => ui.colored_label(ACCENT, problem),
                    None => ui.small(format!("Creates {path}")),
                };
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(height)
                .show(ui, |ui| {
                    for (album, count, cover, problem) in &targets {
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(ui.available_width(), 56.0),
                            if problem.is_none() {
                                Sense::click()
                            } else {
                                Sense::hover()
                            },
                        );
                        if problem.is_none() && response.hovered() {
                            ui.painter()
                                .rect_filled(rect, 6, Color32::from_rgb(6, 35, 33));
                        }
                        self.cover(
                            ui,
                            cover,
                            Rect::from_min_size(rect.min + vec2(4.0, 6.0), Vec2::splat(44.0)),
                            0,
                            if problem.is_none() {
                                Color32::WHITE
                            } else {
                                Color32::from_gray(80)
                            },
                        );
                        text_at(
                            ui,
                            rect.min + vec2(58.0, 9.0),
                            album.rsplit('/').next().unwrap_or(album),
                            15.0,
                            if problem.is_none() { TEXT } else { MUTED },
                            rect.width() - 62.0,
                        );
                        text_at(
                            ui,
                            rect.min + vec2(58.0, 31.0),
                            &problem
                                .clone()
                                .unwrap_or_else(|| format!("{album} · {}", plural(*count, "item"))),
                            11.0,
                            MUTED,
                            rect.width() - 62.0,
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                problem.is_none(),
                                album,
                            )
                        });
                        if response.clicked() {
                            choice = Some(album.clone());
                        }
                    }
                });
            ui.add_space(6.0);
            if button(ui, "close", "Cancel", TEXT, true) {
                open = false;
            }
        });
        if let Some(target) = choice {
            let action = if copy { "copy" } else { "move" };
            self.send_manage(action, items, json!({"target": target}));
            return false;
        }
        open
    }

    fn rename_dialog(
        &mut self,
        ctx: &egui::Context,
        width: f32,
        items: &[MediaItem],
        name: &mut String,
        focus: &mut bool,
    ) -> bool {
        let old: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
        let fresh = sequence_names(name, &old);
        let problem = name_problem(name);
        let extension = match items {
            [item] => split_name(&item.name).1,
            _ => "",
        };
        let title = match items.len() {
            1 => "Rename".to_owned(),
            count => format!("Rename {}", plural(count, "item")),
        };
        let (mut open, mut confirm) = (true, false);
        window(title, width, true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let field = if extension.is_empty() { 0.0 } else { 64.0 };
                let edit = ui.add_sized(
                    [(ui.available_width() - field).max(80.0), 36.0],
                    egui::TextEdit::singleline(name).hint_text("New name"),
                );
                if std::mem::take(focus) {
                    edit.request_focus();
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    confirm = true;
                }
                if !extension.is_empty() {
                    ui.label(egui::RichText::new(extension).color(MUTED));
                }
            });
            if let Some(problem) = problem.filter(|_| !name.is_empty()) {
                ui.colored_label(ACCENT, problem);
            } else if let [first, .., last] = fresh.as_slice() {
                ui.small(format!("{first} … {last}"));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if button(ui, "close", "Cancel", TEXT, true) {
                    open = false;
                }
                if button(ui, "edit", "Rename", ACCENT, problem.is_none()) {
                    confirm = true;
                }
            });
        });
        if confirm && problem.is_none() {
            let names: serde_json::Map<String, Value> = items
                .iter()
                .zip(&fresh)
                .filter(|(item, name)| &item.name != *name)
                .map(|(item, name)| (item.uri.clone(), Value::from(name.as_str())))
                .collect();
            let changed: Vec<MediaItem> = items
                .iter()
                .filter(|item| names.contains_key(&item.uri))
                .cloned()
                .collect();
            self.send_manage("rename", &changed, json!({"names": names}));
            return false;
        }
        open
    }

    fn rename_album_dialog(
        &mut self,
        ctx: &egui::Context,
        width: f32,
        album: &str,
        items: &[MediaItem],
        name: &mut String,
        focus: &mut bool,
    ) -> bool {
        let kinds: Vec<&str> = items
            .iter()
            .filter(|item| item.is_media_store())
            .map(|item| item.kind.as_str())
            .collect();
        let target = renamed_album(album, name).unwrap_or_else(|| name.trim().to_owned());
        let problem = name_problem(name).map(str::to_owned).or_else(|| {
            if !kinds.is_empty() && !album.contains('/') {
                Some("Top-level storage folders cannot be renamed".to_owned())
            } else if !kinds.is_empty() {
                album_problem(&target, kinds.iter().copied())
            } else if items
                .iter()
                .any(|item| !item.is_media_store() && item.folder.is_empty())
            {
                Some("A linked folder itself cannot be renamed".to_owned())
            } else {
                None
            }
        });
        let unchanged = name.trim() == album.rsplit('/').next().unwrap_or(album);
        let (mut open, mut confirm) = (true, false);
        window("Rename album", width, true).show(ctx, |ui| {
            let edit = ui.add_sized(
                [ui.available_width(), 36.0],
                egui::TextEdit::singleline(name).hint_text("Album name"),
            );
            if std::mem::take(focus) {
                edit.request_focus();
            }
            if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                confirm = true;
            }
            match &problem {
                Some(problem) => ui.colored_label(ACCENT, problem),
                None => ui.small(format!("{} in {target}", plural(items.len(), "item"))),
            };
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if button(ui, "close", "Cancel", TEXT, true) {
                    open = false;
                }
                if button(ui, "edit", "Rename", ACCENT, problem.is_none() && !unchanged) {
                    confirm = true;
                }
            });
        });
        if confirm && problem.is_none() && !unchanged {
            self.send_manage("rename_album", items, json!({"target": target}));
            return false;
        }
        open
    }

    fn delete_dialog(
        &mut self,
        ctx: &egui::Context,
        width: f32,
        items: &[MediaItem],
        action: &str,
        title: &str,
        detail: &str,
    ) -> bool {
        let mut open = true;
        window(title, width, false).show(ctx, |ui| {
            ui.label(detail);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if button(ui, "close", "Cancel", TEXT, true) {
                    open = false;
                }
                if button(ui, "delete", "Delete", ACCENT, true) {
                    self.send_manage(action, items, json!({}));
                    open = false;
                }
            });
        });
        open
    }
}
