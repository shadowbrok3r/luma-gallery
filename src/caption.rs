//! Caption and picture rasterizer shared by the editor preview and the export overlays.
use crate::studio::{Align, Picture, TextStyle};
use harfrust::{ShaperData, ShaperInstance, Tag, UnicodeBuffer, Variation};
use image::RgbaImage;
use skrifa::bitmap::{BitmapData, BitmapGlyph, BitmapStrikes, Origin};
use skrifa::instance::{Location, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use tiny_skia::{
    FillRule, FilterQuality, LineCap, LineJoin, Paint, PathBuilder, Pattern, Pixmap, PixmapPaint,
    SpreadMode, Stroke, Transform,
};

const SYSTEM_DIRS: [&str; 2] = ["/system/fonts", "/product/fonts"];
const SYSTEM_FALLBACKS: [&str; 12] = [
    "/system/fonts/Roboto-Regular.ttf",
    "/system/fonts/NotoColorEmoji.ttf",
    "/system/fonts/NotoColorEmojiLegacy.ttf",
    "/system/fonts/SamsungColorEmoji.ttf",
    "/system/fonts/NotoSansSymbols-Regular-Subsetted.ttf",
    "/system/fonts/NotoSansSymbols-Regular-Subsetted2.ttf",
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/system/fonts/NotoSansArabic-Regular.ttf",
    "/system/fonts/NotoSansHebrew-Regular.ttf",
    "/system/fonts/NotoSansDevanagari-Regular.ttf",
    "/system/fonts/NotoSansThai-Regular.ttf",
    "/system/fonts/DroidSansFallback.ttf",
];

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Embedded(&'static [u8]),
    File(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    pub source: Source,
    pub index: u32,
    pub weight: u16,
    pub italic: bool,
    /// Range of the `wght` axis.
    pub wght: Option<(f32, f32)>,
}

#[derive(Clone, Debug)]
pub struct Family {
    pub name: String,
    pub faces: Vec<Face>,
}

pub struct FontBook {
    families: Vec<Family>,
    fallbacks: Vec<Face>,
}

fn embedded(bytes: &'static [u8], weight: u16) -> Face {
    Face {
        source: Source::Embedded(bytes),
        index: 0,
        weight,
        italic: false,
        wght: None,
    }
}

/// Families shown in the font picker; script-specific Noto faces only serve as fallbacks.
fn listed(name: &str) -> bool {
    let hidden = ["Emoji", "Symbols", "Math", "Music", "Fallback", "Color", "Clock"];
    !hidden.iter().any(|h| name.contains(h))
        && !(name.starts_with("Noto Sans ") || name.starts_with("Noto Serif "))
}

impl FontBook {
    pub fn embedded() -> Self {
        Self {
            families: vec![
                Family {
                    name: "Ubuntu".into(),
                    faces: vec![embedded(epaint_default_fonts::UBUNTU_LIGHT, 300)],
                },
                Family {
                    name: "Hack".into(),
                    faces: vec![embedded(epaint_default_fonts::HACK_REGULAR, 400)],
                },
            ],
            fallbacks: vec![
                embedded(epaint_default_fonts::UBUNTU_LIGHT, 300),
                embedded(epaint_default_fonts::NOTO_EMOJI_REGULAR, 400),
                embedded(epaint_default_fonts::EMOJI_ICON, 400),
            ],
        }
    }

    /// Embedded fonts plus every listed family found in `dirs`.
    pub fn scan(dirs: &[&str], fallbacks: &[&str]) -> Self {
        let mut book = Self::embedded();
        let mut found: Vec<(String, Face)> = vec![];
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                let extension = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if matches!(extension.as_str(), "ttf" | "otf" | "ttc") {
                    found.extend(read_faces(&path).unwrap_or_default());
                }
            }
        }
        let mut system: Vec<Family> = vec![];
        for (name, face) in found {
            if !listed(&name) {
                continue;
            }
            match system.iter_mut().find(|f| f.name == name) {
                Some(family) if !family.faces.contains(&face) => family.faces.push(face),
                Some(_) => {}
                None => system.push(Family {
                    name,
                    faces: vec![face],
                }),
            }
        }
        system.sort_by(|a, b| {
            (a.name != "Roboto")
                .cmp(&(b.name != "Roboto"))
                .then(a.name.cmp(&b.name))
        });
        system.append(&mut book.families);
        book.families = system;
        let mut chain: Vec<Face> = fallbacks
            .iter()
            .filter(|p| Path::new(p).exists())
            .map(|p| Face {
                source: Source::File(PathBuf::from(p)),
                index: 0,
                weight: 400,
                italic: false,
                wght: None,
            })
            .collect();
        chain.append(&mut book.fallbacks);
        book.fallbacks = chain;
        book
    }

    pub fn families(&self) -> &[Family] {
        &self.families
    }

    pub fn default_family(&self) -> &str {
        &self.families[0].name
    }

    fn family(&self, name: &str) -> &Family {
        self.families
            .iter()
            .find(|f| f.name == name)
            .unwrap_or(&self.families[0])
    }

    /// Closest face; returns the face, a `wght` setting, and whether bold/italic must be synthesized.
    pub fn resolve(&self, name: &str, bold: bool, italic: bool) -> (Face, Option<f32>, bool, bool) {
        let family = self.family(name);
        let target = if bold { 700 } else { 400 };
        let face = family
            .faces
            .iter()
            .min_by_key(|f| {
                let weight = match f.wght {
                    Some((low, high)) => (target as f32).clamp(low, high) as i32,
                    None => f.weight as i32,
                };
                ((f.italic != italic) as i32 * 1000) + (weight - target).abs()
            })
            .cloned()
            .unwrap_or_else(|| embedded(epaint_default_fonts::UBUNTU_LIGHT, 300));
        let wght = face.wght.map(|(low, high)| (target as f32).clamp(low, high));
        let weight = wght.map_or(face.weight as f32, |w| w);
        let fake_bold = bold && weight < 600.0;
        let fake_italic = italic && !face.italic;
        (face, wght, fake_bold, fake_italic)
    }
}

pub fn book() -> &'static FontBook {
    static BOOK: OnceLock<FontBook> = OnceLock::new();
    BOOK.get_or_init(|| FontBook::scan(&SYSTEM_DIRS, &SYSTEM_FALLBACKS))
}

fn read_at(file: &mut std::fs::File, offset: u64, length: usize) -> Option<Vec<u8>> {
    let mut buffer = vec![0; length];
    file.seek(SeekFrom::Start(offset)).ok()?;
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// Family names and styles from the `name`, `OS/2` and `fvar` tables without loading whole files.
fn read_faces(path: &Path) -> Option<Vec<(String, Face)>> {
    let mut file = std::fs::File::open(path).ok()?;
    let header = read_at(&mut file, 0, 12)?;
    let offsets: Vec<u32> = if &header[..4] == b"ttcf" {
        let count = u32_at(&header, 8)?.min(64) as usize;
        let table = read_at(&mut file, 12, count * 4)?;
        (0..count).filter_map(|i| u32_at(&table, i * 4)).collect()
    } else {
        vec![0]
    };
    let mut faces = vec![];
    for (index, offset) in offsets.into_iter().enumerate() {
        let directory = read_at(&mut file, offset as u64, 12)?;
        let tables = u16_at(&directory, 4)? as usize;
        let records = read_at(&mut file, offset as u64 + 12, tables.min(256) * 16)?;
        let table = |tag: &[u8; 4]| {
            (0..tables.min(256)).find_map(|i| {
                let r = &records[i * 16..i * 16 + 16];
                (&r[..4] == tag).then(|| (u32_at(r, 8).unwrap_or(0), u32_at(r, 12).unwrap_or(0)))
            })
        };
        let Some((name_at, name_len)) = table(b"name") else {
            continue;
        };
        let names = read_at(&mut file, name_at as u64, (name_len as usize).min(1 << 20))?;
        let Some(family) = name(&names, 16).or_else(|| name(&names, 1)) else {
            continue;
        };
        let subfamily = name(&names, 17)
            .or_else(|| name(&names, 2))
            .unwrap_or_default();
        let (mut weight, mut italic) = (400, subfamily.to_lowercase().contains("italic"));
        if let Some((at, len)) = table(b"OS/2").filter(|(_, len)| *len >= 64) {
            let os2 = read_at(&mut file, at as u64, len.min(128) as usize)?;
            weight = u16_at(&os2, 4).unwrap_or(400);
            italic |= u16_at(&os2, 62).unwrap_or(0) & 1 != 0;
        }
        let mut wght = None;
        if let Some((at, len)) = table(b"fvar") {
            let fvar = read_at(&mut file, at as u64, (len as usize).min(4096))?;
            let start = u16_at(&fvar, 4)? as usize;
            let (count, size) = (u16_at(&fvar, 8)? as usize, u16_at(&fvar, 10)? as usize);
            for axis in 0..count {
                let a = start + axis * size;
                if fvar.get(a..a + 4) == Some(b"wght") {
                    let fixed = |at| u32_at(&fvar, at).map(|v| v as i32 as f32 / 65536.0);
                    wght = fixed(a + 4).zip(fixed(a + 12));
                }
            }
        }
        faces.push((
            family,
            Face {
                source: Source::File(path.to_owned()),
                index: index as u32,
                weight,
                italic,
                wght,
            },
        ));
    }
    Some(faces)
}

fn name(table: &[u8], id: u16) -> Option<String> {
    let count = u16_at(table, 2)? as usize;
    let storage = u16_at(table, 4)? as usize;
    let mut best: Option<(u8, String)> = None;
    for i in 0..count {
        let r = 6 + i * 12;
        let (platform, encoding, language, name_id) = (
            u16_at(table, r)?,
            u16_at(table, r + 2)?,
            u16_at(table, r + 4)?,
            u16_at(table, r + 6)?,
        );
        if name_id != id {
            continue;
        }
        let (length, offset) = (u16_at(table, r + 8)? as usize, u16_at(table, r + 10)? as usize);
        let Some(bytes) = table.get(storage + offset..storage + offset + length) else {
            continue;
        };
        let rank = match (platform, language) {
            (3, 0x409) => 0,
            (3, _) | (0, _) => 1,
            (1, 0) => 2,
            _ => continue,
        };
        let text = if platform == 1 {
            if encoding != 0 {
                continue;
            }
            bytes.iter().map(|&b| b as char).collect()
        } else {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16_lossy(&units)
        };
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, text));
        }
    }
    best.map(|(_, text)| text.trim().to_owned())
        .filter(|t| !t.is_empty())
}

#[derive(Clone)]
enum Blob {
    Static(&'static [u8]),
    Shared(Arc<Vec<u8>>),
}

impl std::ops::Deref for Blob {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Blob::Static(bytes) => bytes,
            Blob::Shared(bytes) => bytes,
        }
    }
}

fn load(face: &Face) -> Option<Blob> {
    static FILES: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>> = OnceLock::new();
    match &face.source {
        Source::Embedded(bytes) => Some(Blob::Static(bytes)),
        Source::File(path) => {
            let mut files = FILES.get_or_init(Default::default).lock().ok()?;
            if let Some(bytes) = files.get(path) {
                return Some(Blob::Shared(bytes.clone()));
            }
            let bytes = Arc::new(std::fs::read(path).ok()?);
            files.insert(path.clone(), bytes.clone());
            Some(Blob::Shared(bytes))
        }
    }
}

/// A loaded face plus variation settings for one render.
struct Loaded {
    blob: Blob,
    index: u32,
    wght: Option<f32>,
}

impl Loaded {
    fn font(&self) -> Option<FontRef<'_>> {
        FontRef::from_index(&self.blob, self.index).ok()
    }
    fn location(&self, font: &FontRef) -> Location {
        match self.wght {
            Some(w) => font.axes().location([("wght", w)]),
            None => Location::default(),
        }
    }
}

/// Faces a render may use: the requested face first, fallbacks loaded on demand.
struct Faces<'b> {
    loaded: Vec<Loaded>,
    pending: std::slice::Iter<'b, Face>,
}

impl Faces<'_> {
    /// Whether the face maps `c` to a glyph this renderer can draw.
    fn covers(loaded: &Loaded, c: char) -> bool {
        let Some(font) = loaded.font() else {
            return false;
        };
        let Some(glyph) = font.charmap().map(c).filter(|g| *g != GlyphId::NOTDEF) else {
            return false;
        };
        let strikes = BitmapStrikes::new(&font);
        if !strikes.is_empty() {
            return strikes.glyph_for_size(Size::new(64.0), glyph).is_some();
        }
        font.table_data(Tag::new(b"COLR")).is_none() && font.outline_glyphs().get(glyph).is_some()
    }
    /// Index of the first face mapping `c`, or 0.
    fn pick(&mut self, c: char) -> usize {
        if let Some(i) = self.loaded.iter().position(|l| Self::covers(l, c)) {
            return i;
        }
        for face in self.pending.by_ref() {
            let Some(blob) = load(face) else { continue };
            let loaded = Loaded {
                blob,
                index: face.index,
                wght: None,
            };
            let hit = Self::covers(&loaded, c);
            self.loaded.push(loaded);
            if hit {
                return self.loaded.len() - 1;
            }
        }
        0
    }
}

#[derive(Clone, Copy, Debug)]
struct Glyph {
    face: usize,
    id: u32,
    x: f32,
    y: f32,
}

#[derive(Debug)]
struct Cluster {
    glyphs: Vec<Glyph>,
    advance: f32,
    space: bool,
    /// Index of the cluster's first character in the whole text.
    index: usize,
}

#[derive(Debug, Default)]
struct Line {
    clusters: Vec<Cluster>,
    width: f32,
}

fn sticky(c: char) -> bool {
    matches!(c, '\u{200D}' | '\u{FE0E}' | '\u{FE0F}' | '\u{20E3}' | '\u{1F3FB}'..='\u{1F3FF}')
        || matches!(c, '\u{0300}'..='\u{036F}' | '\u{E0020}'..='\u{E007F}')
}

/// Shapes one paragraph starting at character `base` into clusters in logical order; returns whether it reads right to left.
fn shape(text: &str, base: usize, faces: &mut Faces, px: f32, spacing: f32) -> (Vec<Cluster>, bool) {
    let mut runs: Vec<(usize, usize, usize)> = vec![];
    for (at, c) in text.char_indices() {
        let face = match runs.last() {
            Some(&(_, _, previous)) if c.is_whitespace() || sticky(c) => previous,
            _ => faces.pick(c),
        };
        match runs.last_mut() {
            Some(run) if run.2 == face => run.1 = at + c.len_utf8(),
            _ => runs.push((at, at + c.len_utf8(), face)),
        }
    }
    let mut clusters = vec![];
    let mut rtl = false;
    for (n, &(from, to, face)) in runs.iter().enumerate() {
        let loaded = &faces.loaded[face];
        let Some(font) = loaded.font() else { continue };
        let units = font.metrics(Size::unscaled(), &loaded.location(&font)).units_per_em;
        let scale = px / units.max(1) as f32;
        let data = ShaperData::new(&font);
        let instance = loaded.wght.map(|w| {
            ShaperInstance::from_variations(&font, [Variation {
                tag: Tag::new(b"wght"),
                value: w,
            }])
        });
        let shaper = data.shaper(&font).instance(instance.as_ref()).build();
        let mut buffer = UnicodeBuffer::new();
        let mut flags = harfrust::BufferFlags::empty();
        if n == 0 {
            flags |= harfrust::BufferFlags::BEGINNING_OF_TEXT;
        }
        if n + 1 == runs.len() {
            flags |= harfrust::BufferFlags::END_OF_TEXT;
        }
        buffer.set_flags(flags);
        buffer.push_str(&text[from..to]);
        buffer.guess_segment_properties();
        let backward = buffer.direction() == harfrust::Direction::RightToLeft;
        rtl |= backward && n == 0;
        let output = shaper.shape(buffer, harfrust::ShapeOptions::new());
        let mut groups: Vec<(u32, Cluster)> = vec![];
        for (info, pos) in output.glyph_infos().iter().zip(output.glyph_positions()) {
            if groups.last().is_none_or(|(c, _)| *c != info.cluster) {
                let at = from + info.cluster as usize;
                let space = text[at..].chars().next().is_some_and(char::is_whitespace);
                groups.push((
                    info.cluster,
                    Cluster {
                        glyphs: vec![],
                        advance: 0.0,
                        space,
                        index: base + text[..at].chars().count(),
                    },
                ));
            }
            let cluster = &mut groups.last_mut().expect("pushed above").1;
            cluster.glyphs.push(Glyph {
                face,
                id: info.glyph_id,
                x: cluster.advance + pos.x_offset as f32 * scale,
                y: pos.y_offset as f32 * scale,
            });
            cluster.advance += pos.x_advance as f32 * scale;
        }
        if backward {
            groups.reverse();
        }
        clusters.extend(groups.into_iter().map(|(_, mut c)| {
            c.advance += spacing;
            c
        }));
    }
    (clusters, rtl)
}

/// Greedy word wrap; words wider than `max` break between clusters.
fn wrap(clusters: Vec<Cluster>, max: f32) -> Vec<Line> {
    let mut lines = vec![Line::default()];
    let mut word: Vec<Cluster> = vec![];
    let flush = |lines: &mut Vec<Line>, word: &mut Vec<Cluster>| {
        let width: f32 = word.iter().filter(|c| !c.space).map(|c| c.advance).sum();
        let line = lines.last_mut().expect("never empty");
        if !line.clusters.is_empty() && line.width + width > max {
            lines.push(Line::default());
        }
        for cluster in word.drain(..) {
            let line = lines.last_mut().expect("never empty");
            if !cluster.space && !line.clusters.is_empty() && line.width + cluster.advance > max
                && line.clusters.iter().all(|c| !c.space)
            {
                lines.push(Line::default());
            }
            let line = lines.last_mut().expect("never empty");
            line.width += cluster.advance;
            line.clusters.push(cluster);
        }
    };
    for cluster in clusters {
        let ends_word = cluster.space;
        word.push(cluster);
        if ends_word {
            flush(&mut lines, &mut word);
        }
    }
    flush(&mut lines, &mut word);
    for line in &mut lines {
        while line.clusters.last().is_some_and(|c| c.space) {
            let space = line.clusters.pop().expect("checked");
            line.width -= space.advance;
        }
    }
    lines
}

struct Pen<'a> {
    path: &'a mut PathBuilder,
    x: f32,
    y: f32,
    skew: f32,
}

impl Pen<'_> {
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x + y * self.skew, self.y - y)
    }
}

impl OutlinePen for Pen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.path.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.path.line_to(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let ((cx, cy), (x, y)) = (self.at(cx, cy), self.at(x, y));
        self.path.quad_to(cx, cy, x, y);
    }
    fn curve_to(&mut self, ax: f32, ay: f32, bx: f32, by: f32, x: f32, y: f32) {
        let ((ax, ay), (bx, by), (x, y)) = (self.at(ax, ay), self.at(bx, by), self.at(x, y));
        self.path.cubic_to(ax, ay, bx, by, x, y);
    }
    fn close(&mut self) {
        self.path.close();
    }
}

/// Color bitmap glyph as a premultiplied pixmap.
fn bitmap(glyph: &BitmapGlyph) -> Option<Pixmap> {
    let size = tiny_skia::IntSize::from_wh(glyph.width, glyph.height)?;
    let data = match glyph.data {
        BitmapData::Png(bytes) => {
            let image = image::load_from_memory(bytes).ok()?.to_rgba8();
            let size = tiny_skia::IntSize::from_wh(image.width(), image.height())?;
            let mut data = image.into_raw();
            for p in data.chunks_exact_mut(4) {
                let a = p[3] as u16;
                for c in &mut p[..3] {
                    *c = ((*c as u16 * a + 127) / 255) as u8;
                }
            }
            return Pixmap::from_vec(data, size);
        }
        BitmapData::Bgra(bytes) => bytes
            .chunks_exact(4)
            .flat_map(|p| [p[2], p[1], p[0], p[3]])
            .collect(),
        BitmapData::Mask(_) => return None,
    };
    Pixmap::from_vec(data, size)
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    let r = r.clamp(0.0, w.min(h) / 2.0);
    let k = r * 0.447_715;
    let mut p = PathBuilder::new();
    p.move_to(x + r, y);
    p.line_to(x + w - r, y);
    p.cubic_to(x + w - k, y, x + w, y + k, x + w, y + r);
    p.line_to(x + w, y + h - r);
    p.cubic_to(x + w, y + h - k, x + w - k, y + h, x + w - r, y + h);
    p.line_to(x + r, y + h);
    p.cubic_to(x + k, y + h, x, y + h - k, x, y + h - r);
    p.line_to(x, y + r);
    p.cubic_to(x, y + k, x + k, y, x + r, y);
    p.close();
    p.finish()
}

fn paint(rgba: [u8; 4]) -> Paint<'static> {
    let mut paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    paint.set_color_rgba8(rgba[0], rgba[1], rgba[2], rgba[3]);
    paint
}

/// Three box blurs approximate a Gaussian of standard deviation `sigma`.
fn blur(pixmap: &mut Pixmap, sigma: f32) {
    let radius = ((sigma * 0.9).round() as usize).max(1);
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let data = pixmap.data_mut();
    let mut line = vec![0u8; w.max(h) * 4];
    for _ in 0..3 {
        for (length, count, step, stride) in [(w, h, 4, w * 4), (h, w, w * 4, 4)] {
            for n in 0..count {
                let base = n * stride;
                for i in 0..length {
                    line[i * 4..i * 4 + 4].copy_from_slice(&data[base + i * step..base + i * step + 4]);
                }
                let mut sum = [0u32; 4];
                let window = (2 * radius + 1) as u32;
                let sample = |i: isize, c: usize| -> u32 {
                    if i < 0 || i >= length as isize {
                        0
                    } else {
                        line[i as usize * 4 + c] as u32
                    }
                };
                for i in -(radius as isize)..=(radius as isize) {
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += sample(i, c);
                    }
                }
                for i in 0..length {
                    for c in 0..4 {
                        data[base + i * step + c] = (sum[c] / window) as u8;
                        sum[c] = sum[c] + sample(i as isize + radius as isize + 1, c)
                            - sample(i as isize - radius as isize, c);
                    }
                }
            }
        }
    }
}

/// A transparent RGBA overlay; `content` is the box the editor selects, in image pixels.
pub struct Rendered {
    pub image: RgbaImage,
    pub content: [f32; 4],
}

fn finish(pixmap: Pixmap, content: [f32; 4]) -> Rendered {
    let (w, h) = (pixmap.width(), pixmap.height());
    let mut image = RgbaImage::new(w, h);
    for (out, px) in image.pixels_mut().zip(pixmap.pixels()) {
        let c = px.demultiply();
        out.0 = [c.red(), c.green(), c.blue(), c.alpha()];
    }
    Rendered { image, content }
}

fn empty() -> Rendered {
    Rendered {
        image: RgbaImage::new(2, 2),
        content: [0.0, 0.0, 2.0, 2.0],
    }
}

/// Renders a caption for a `frame_width` x `frame_height` frame, unrotated and fully opaque.
pub fn render_text(style: &TextStyle, frame_width: f32, frame_height: f32) -> Rendered {
    render_text_with(book(), style, frame_width, frame_height)
}

pub fn render_text_with(
    book: &FontBook,
    style: &TextStyle,
    frame_width: f32,
    frame_height: f32,
) -> Rendered {
    let px = (style.size * frame_height).max(1.0);
    let text = if style.uppercase {
        style.text.to_uppercase()
    } else {
        style.text.clone()
    }
    .replace('\t', " ");
    if text.trim().is_empty() {
        return empty();
    }
    let (face, wght, fake_bold, fake_italic) = book.resolve(&style.font, style.bold, style.italic);
    let Some(blob) = load(&face) else {
        return empty();
    };
    let mut faces = Faces {
        loaded: vec![Loaded {
            blob,
            index: face.index,
            wght,
        }],
        pending: book.fallbacks.iter(),
    };
    let max = (style.wrap * frame_width).max(px);
    let spacing = style.spacing * px;
    let mut lines = vec![];
    let mut rtl = false;
    let mut base = 0;
    for paragraph in text.split('\n') {
        let (clusters, backward) = shape(paragraph, base, &mut faces, px, spacing);
        base += paragraph.chars().count() + 1;
        rtl |= backward;
        lines.extend(wrap(clusters, max));
    }
    let primary = &faces.loaded[0];
    let Some(font) = primary.font() else {
        return empty();
    };
    let metrics = font.metrics(Size::new(px), &primary.location(&font));
    let mut used = vec![false; faces.loaded.len()];
    used[0] = true;
    for glyph in lines.iter().flat_map(|l| &l.clusters).flat_map(|c| &c.glyphs) {
        used[glyph.face] = true;
    }
    let (mut ascent, mut descent, mut leading) = (0.0f32, 0.0f32, 0.0f32);
    for loaded in faces.loaded.iter().zip(&used).filter(|(_, u)| **u).map(|(l, _)| l) {
        if let Some(font) = loaded.font() {
            let m = font.metrics(Size::new(px), &loaded.location(&font));
            (ascent, descent, leading) = (ascent.max(m.ascent), descent.min(m.descent), leading.max(m.leading));
        }
    }
    let natural = ascent - descent + leading;
    let line_height = natural * style.line_height.max(0.5);
    let block_width = lines.iter().map(|l| l.width).fold(0.0, f32::max).max(1.0);
    let block_height = line_height * (lines.len() as f32 - 1.0) + natural;
    let pad = style.padding.max(0.0) * px;
    let box_width = block_width + pad * 2.0;
    let box_height = block_height + pad * 2.0;
    let outline = style.outline.max(0.0) * px;
    let shadow = style.shadow.max(0.0) * px;
    let offset = [style.shadow_offset[0] * px, style.shadow_offset[1] * px];
    let reach = if shadow > 0.0 && style.shadow_color[3] > 0 {
        shadow * 3.0 + offset[0].abs().max(offset[1].abs())
    } else {
        0.0
    };
    let margin = (outline + reach + 2.0).ceil();
    let width = (box_width + margin * 2.0).ceil() as u32;
    let height = (box_height + margin * 2.0).ceil() as u32;
    if width as u64 * height as u64 > 64_000_000 {
        return empty();
    }
    let Some(mut pixmap) = Pixmap::new(width, height) else {
        return empty();
    };
    let origin = (
        (width as f32 - box_width) / 2.0,
        (height as f32 - box_height) / 2.0,
    );
    let mut glyphs = PathBuilder::new();
    let mut marked = PathBuilder::new();
    let mut underlines = PathBuilder::new();
    let mut images: Vec<(Pixmap, Transform)> = vec![];
    let skew = if fake_italic { 0.2 } else { 0.0 };
    let (under_offset, under_thickness) = metrics
        .underline
        .map_or((-0.1 * px, 0.06 * px), |d| (d.offset, d.thickness.max(1.0)));
    for (n, line) in lines.iter().enumerate() {
        let x0 = origin.0
            + pad
            + match style.align {
                Align::Left => 0.0,
                Align::Center => (block_width - line.width) / 2.0,
                Align::Right => block_width - line.width,
            };
        let baseline = origin.1 + pad + ascent + line_height * n as f32;
        let mut pen = x0;
        let order: Box<dyn Iterator<Item = &Cluster>> = if rtl {
            Box::new(line.clusters.iter().rev())
        } else {
            Box::new(line.clusters.iter())
        };
        for cluster in order {
            let lit = style.highlight.is_some_and(|[a, b]| (a..b).contains(&cluster.index));
            for glyph in cluster.glyphs.iter().filter(|g| g.id != 0) {
                let loaded = &faces.loaded[glyph.face];
                let Some(font) = loaded.font() else { continue };
                let location = loaded.location(&font);
                let id = GlyphId::new(glyph.id);
                let strikes = BitmapStrikes::new(&font);
                if !strikes.is_empty() {
                    if let Some(found) = strikes.glyph_for_size(Size::new(px), id)
                        && let Some(image) = bitmap(&found)
                    {
                        let s = px / found.ppem_y.max(1.0);
                        let unit = px / font.metrics(Size::unscaled(), &location).units_per_em as f32;
                        let left = pen + glyph.x + found.bearing_x * unit + found.inner_bearing_x * s;
                        let mut top = baseline - glyph.y - found.bearing_y * unit;
                        top -= found.inner_bearing_y * s;
                        if found.placement_origin == Origin::BottomLeft {
                            top -= found.height as f32 * s;
                        }
                        images.push((image, Transform::from_scale(s, s).post_translate(left, top)));
                    }
                    continue;
                }
                let Some(outline) = font.outline_glyphs().get(id) else {
                    continue;
                };
                let mut path = Pen {
                    path: if lit { &mut marked } else { &mut glyphs },
                    x: pen + glyph.x,
                    y: baseline - glyph.y,
                    skew,
                };
                let _ = outline.draw(DrawSettings::unhinted(Size::new(px), &location), &mut path);
            }
            pen += cluster.advance;
        }
        if style.underline && line.width > 0.0 {
            if let Some(rect) = tiny_skia::Rect::from_xywh(
                x0,
                baseline - under_offset,
                line.width,
                under_thickness,
            ) {
                underlines.push_rect(rect);
            }
        }
    }
    let glyphs = glyphs.finish();
    let marked = marked.finish();
    let underlines = underlines.finish();
    let backdrop = rounded_rect(
        origin.0,
        origin.1,
        box_width,
        box_height,
        style.radius.max(0.0) * px,
    );
    let has_box = style.background[3] > 0;
    let identity = Transform::identity();
    if reach > 0.0 {
        if let Some(mut layer) = Pixmap::new(width, height) {
            let color = paint(style.shadow_color);
            let at = Transform::from_translate(offset[0], offset[1]);
            if has_box {
                if let Some(path) = &backdrop {
                    layer.fill_path(path, &color, FillRule::Winding, at, None);
                }
            } else {
                for path in [&glyphs, &marked, &underlines].into_iter().flatten() {
                    layer.fill_path(path, &color, FillRule::Winding, at, None);
                    if outline > 0.0 {
                        let stroke = Stroke {
                            width: outline * 2.0,
                            line_join: LineJoin::Round,
                            line_cap: LineCap::Round,
                            ..Stroke::default()
                        };
                        layer.stroke_path(path, &color, &stroke, at, None);
                    }
                }
            }
            blur(&mut layer, shadow);
            pixmap.draw_pixmap(0, 0, layer.as_ref(), &PixmapPaint::default(), identity, None);
        }
    }
    if has_box && let Some(path) = &backdrop {
        pixmap.fill_path(path, &paint(style.background), FillRule::Winding, identity, None);
    }
    for path in [&glyphs, &marked, &underlines].into_iter().flatten() {
        if outline > 0.0 && style.outline_color[3] > 0 {
            let stroke = Stroke {
                width: outline * 2.0,
                line_join: LineJoin::Round,
                line_cap: LineCap::Round,
                ..Stroke::default()
            };
            pixmap.stroke_path(path, &paint(style.outline_color), &stroke, identity, None);
        }
    }
    for (path, color) in [(&glyphs, style.color), (&marked, style.highlight_color), (&underlines, style.color)] {
        let Some(path) = path else { continue };
        let fill = paint(color);
        pixmap.fill_path(path, &fill, FillRule::Winding, identity, None);
        if fake_bold {
            let stroke = Stroke {
                width: px * 0.045,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            pixmap.stroke_path(path, &fill, &stroke, identity, None);
        }
    }
    let quality = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    for (image, at) in &images {
        pixmap.draw_pixmap(0, 0, image.as_ref(), &quality, *at, None);
    }
    finish(pixmap, [origin.0, origin.1, box_width, box_height])
}

/// Renders a picture overlay scaled to its frame width with rounded corners.
pub fn render_picture(picture: &Picture, source: &RgbaImage, frame_width: f32) -> Rendered {
    let (sw, sh) = source.dimensions();
    if sw == 0 || sh == 0 {
        return empty();
    }
    let width = (picture.width * frame_width).max(2.0);
    let height = width * sh as f32 / sw as f32;
    let (w, h) = (width.ceil() as u32, height.ceil() as u32);
    if w as u64 * h as u64 > 64_000_000 {
        return empty();
    }
    let mut data = Vec::with_capacity((sw * sh * 4) as usize);
    for p in source.pixels() {
        let a = p[3] as u16;
        data.extend([0, 1, 2].map(|c| ((p[c] as u16 * a + 127) / 255) as u8));
        data.push(p[3]);
    }
    let (Some(size), Some(mut pixmap)) = (tiny_skia::IntSize::from_wh(sw, sh), Pixmap::new(w, h))
    else {
        return empty();
    };
    let Some(texture) = Pixmap::from_vec(data, size) else {
        return empty();
    };
    let scale = Transform::from_scale(width / sw as f32, height / sh as f32);
    let shader = Pattern::new(
        texture.as_ref(),
        SpreadMode::Pad,
        FilterQuality::Bicubic,
        1.0,
        scale,
    );
    let fill = Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    };
    if let Some(path) = rounded_rect(0.0, 0.0, width, height, picture.radius * width.min(height)) {
        pixmap.fill_path(&path, &fill, FillRule::Winding, Transform::identity(), None);
    }
    finish(pixmap, [0.0, 0.0, width, height])
}

/// Bakes rotation (degrees clockwise) and opacity into a larger canvas centered on the original.
pub fn transform(image: &RgbaImage, rotation: f32, opacity: f32) -> RgbaImage {
    let (w, h) = image.dimensions();
    let angle = rotation.to_radians();
    let (sin, cos) = (angle.sin().abs(), angle.cos().abs());
    let (tw, th) = (
        (w as f32 * cos + h as f32 * sin).ceil() as u32 + 2,
        (w as f32 * sin + h as f32 * cos).ceil() as u32 + 2,
    );
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for p in image.pixels() {
        let a = p[3] as u16;
        data.extend([0, 1, 2].map(|c| ((p[c] as u16 * a + 127) / 255) as u8));
        data.push(p[3]);
    }
    let (Some(size), Some(mut target)) = (tiny_skia::IntSize::from_wh(w, h), Pixmap::new(tw, th))
    else {
        return image.clone();
    };
    let Some(source) = Pixmap::from_vec(data, size) else {
        return image.clone();
    };
    let at = Transform::from_translate((tw as f32 - w as f32) / 2.0, (th as f32 - h as f32) / 2.0)
        .post_rotate_at(rotation, tw as f32 / 2.0, th as f32 / 2.0);
    let paint = PixmapPaint {
        opacity: opacity.clamp(0.0, 1.0),
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    target.draw_pixmap(0, 0, source.as_ref(), &paint, at, None);
    finish(target, [0.0; 4]).image
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(text: &str) -> TextStyle {
        TextStyle {
            text: text.into(),
            font: "Ubuntu".into(),
            size: 0.06,
            ..TextStyle::default()
        }
    }

    fn opaque(image: &RgbaImage) -> (u32, u32, u32, u32) {
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
        for (x, y, p) in image.enumerate_pixels() {
            if p[3] > 128 {
                (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
            }
        }
        (left, top, right, bottom)
    }

    #[test]
    fn wraps_words_and_keeps_explicit_lines() {
        let book = FontBook::embedded();
        let one = render_text_with(&book, &style("Luma"), 1000.0, 1000.0);
        let wrapped = render_text_with(
            &book,
            &TextStyle {
                wrap: 0.3,
                ..style("Luma studio caption test")
            },
            1000.0,
            1000.0,
        );
        let manual = render_text_with(&book, &style("Luma\nLuma"), 1000.0, 1000.0);
        let line = one.content[3];
        assert!(wrapped.content[2] <= 300.0 + 2.0 * 0.3 * 60.0 + 1.0, "{:?}", wrapped.content);
        assert!(wrapped.content[3] > line * 1.8, "Wrapped onto several lines");
        assert!((manual.content[3] - (2.0 * line - 36.0)).abs() < 1.0);
        assert!(render_text_with(&book, &style("  \n "), 1000.0, 1000.0).image.width() == 2);
    }

    #[test]
    fn highlighted_word_takes_the_highlight_color_on_any_line() {
        let book = FontBook::embedded();
        let yellow = |image: &RgbaImage| -> Vec<u32> {
            image
                .enumerate_pixels()
                .filter(|(_, _, p)| p[3] > 200 && p[0] > 200 && p[1] > 180 && p[2] < 60)
                .map(|(x, _, _)| x)
                .collect()
        };
        let plain = render_text_with(&book, &style("one two\nthree"), 1000.0, 1000.0);
        assert!(yellow(&plain.image).is_empty());
        let second = render_text_with(
            &book,
            &TextStyle {
                highlight: Some([4, 7]),
                ..style("one two\nthree")
            },
            1000.0,
            1000.0,
        );
        let xs = yellow(&second.image);
        let middle = second.image.width() / 2;
        assert!(xs.len() > 200 && xs.iter().all(|&x| x > middle - 10), "Only the right-hand word is yellow");
        let third = render_text_with(
            &book,
            &TextStyle {
                highlight: Some([8, 13]),
                ..style("one two\nthree")
            },
            1000.0,
            1000.0,
        );
        let rows: Vec<u32> = third
            .image
            .enumerate_pixels()
            .filter(|(_, _, p)| p[3] > 200 && p[0] > 200 && p[1] > 180 && p[2] < 60)
            .map(|(_, y, _)| y)
            .collect();
        assert!(!rows.is_empty() && rows.iter().all(|&y| y > third.image.height() / 2), "Second line lit");
    }

    #[test]
    fn outline_surrounds_white_fill_and_canvas_centers_the_box() {
        let book = FontBook::embedded();
        let rendered = render_text_with(&book, &style("I"), 400.0, 1000.0);
        let image = &rendered.image;
        let [x, y, w, h] = rendered.content;
        assert!(((x + w / 2.0) - image.width() as f32 / 2.0).abs() < 1.0);
        assert!(((y + h / 2.0) - image.height() as f32 / 2.0).abs() < 1.0);
        let (left, top, right, bottom) = opaque(image);
        let center = image.get_pixel((left + right) / 2, (top + bottom) / 2);
        assert!(center[0] > 240 && center[3] > 240, "{center:?}");
        let edge = image.get_pixel(left, (top + bottom) / 2);
        assert!(edge[0] < 60 && edge[3] > 128, "Black outline at the left edge: {edge:?}");
        assert_eq!(image.get_pixel(0, 0)[3], 0);
    }

    #[test]
    fn background_box_fills_the_padding() {
        let book = FontBook::embedded();
        let mut s = style("Box");
        s.background = [10, 20, 200, 255];
        s.outline = 0.0;
        let rendered = render_text_with(&book, &s, 1000.0, 1000.0);
        let [x, y, ..] = rendered.content;
        let corner = rendered.image.get_pixel((x + 6.0) as u32 + 10, (y + 2.0) as u32 + 10);
        assert_eq!(corner.0, [10, 20, 200, 255]);
    }

    #[test]
    fn shadow_extends_beyond_the_text() {
        let book = FontBook::embedded();
        let mut s = style("Glow");
        s.outline = 0.0;
        let plain = render_text_with(&book, &s, 1000.0, 1000.0);
        s.shadow = 0.2;
        s.shadow_offset = [0.0, 0.0];
        s.shadow_color = [255, 0, 0, 255];
        let glow = render_text_with(&book, &s, 1000.0, 1000.0);
        assert!(glow.image.width() > plain.image.width());
        let lit = glow.image.pixels().filter(|p| p[3] > 0 && p[0] > p[2]).count();
        assert!(lit > 1000, "Red glow pixels: {lit}");
    }

    #[test]
    fn emoji_fall_back_to_noto_emoji() {
        let book = FontBook::embedded();
        let rendered = render_text_with(&book, &style("😀"), 1000.0, 1000.0);
        let (left, _, right, _) = opaque(&rendered.image);
        assert!(right > left + 20, "Emoji glyph drew nothing");
    }

    #[test]
    fn color_emoji_bitmaps_keep_their_colors() {
        let path = "/usr/share/fonts/noto/NotoColorEmoji.ttf";
        if !Path::new(path).exists() {
            return;
        }
        let book = FontBook::scan(&[], &[path]);
        let mut s = style("A😀");
        s.outline = 0.0;
        let rendered = render_text_with(&book, &s, 1000.0, 1000.0);
        let yellow = rendered
            .image
            .pixels()
            .filter(|p| p[3] > 200 && p[0] > 200 && p[1] > 150 && p[2] < 90)
            .count();
        assert!(yellow > 300, "Yellow emoji pixels: {yellow}");
    }

    #[test]
    fn scan_reads_family_and_weight() {
        let dir = std::env::temp_dir().join(format!("luma-fonts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Hack.ttf"), epaint_default_fonts::HACK_REGULAR).unwrap();
        std::fs::write(dir.join("Emoji.ttf"), epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap();
        std::fs::write(dir.join("junk.ttf"), b"not a font").unwrap();
        let book = FontBook::scan(&[dir.to_str().unwrap()], &[]);
        let names: Vec<&str> = book.families().iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Hack", "Ubuntu", "Hack"]);
        assert_eq!(book.families()[0].faces[0].weight, 400);
        let (face, wght, fake_bold, fake_italic) = book.resolve("Hack", true, true);
        assert!(matches!(face.source, Source::File(_)) && wght.is_none());
        assert!(fake_bold && fake_italic);
        let (face, ..) = book.resolve("Missing", false, false);
        assert_eq!(face, book.families()[0].faces[0]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pictures_scale_to_frame_width_with_rounded_corners() {
        let source = RgbaImage::from_pixel(100, 50, image::Rgba([255, 0, 0, 255]));
        let picture = Picture {
            width: 0.5,
            radius: 0.5,
            ..Picture::default()
        };
        let rendered = render_picture(&picture, &source, 400.0);
        assert_eq!(rendered.image.dimensions(), (200, 100));
        assert_eq!(rendered.image.get_pixel(1, 1)[3], 0, "Rounded corner");
        assert_eq!(rendered.image.get_pixel(100, 50).0, [255, 0, 0, 255]);
    }

    #[test]
    fn transform_rotates_and_fades() {
        let source = RgbaImage::from_pixel(100, 20, image::Rgba([0, 255, 0, 255]));
        let rotated = transform(&source, 90.0, 0.5);
        let (w, h) = rotated.dimensions();
        assert!(w < 30 && h > 95, "{w}x{h}");
        let p = rotated.get_pixel(w / 2, h / 2);
        assert!(p[1] > 240 && (p[3] as i32 - 128).abs() < 4, "{p:?}");
    }
}

/// `LUMA_SHEET=out.png cargo test --lib preset_sheet -- --ignored` renders every preset.
#[cfg(test)]
#[test]
#[ignore]
fn preset_sheet() {
    let Ok(path) = std::env::var("LUMA_SHEET") else {
        return;
    };
    let book = &FontBook::scan(&["/usr/share/fonts/noto"], &["/usr/share/fonts/noto/NotoColorEmoji.ttf"]);
    let mut sheet = RgbaImage::from_fn(1080, 1920, |x, y| {
        let v = 60 + ((x / 40 + y / 40) % 2) as u8 * 40;
        image::Rgba([v, v + 10, v + 30, 255])
    });
    let mut y = 40;
    for (name, preset) in crate::studio::presets() {
        let style = TextStyle {
            text: format!("{name} caption ✓ 😀\nSecond line"),
            font: "Noto Sans".into(),
            ..preset
        };
        let rendered = render_text_with(book, &style, 1080.0, 1920.0);
        image::imageops::overlay(&mut sheet, &rendered.image, 40, y);
        y += rendered.image.height() as i64 + 10;
    }
    sheet.save(path).unwrap();
}
