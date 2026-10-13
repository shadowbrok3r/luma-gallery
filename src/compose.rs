//! FFmpeg arguments that render a project: segment cuts, fades, crop, overlays and audio.
use crate::studio::Project;

/// One overlay layer: an ffconcat list of equally sized frames covering the timeline, drawn with its top left at `x`,`y` pixels.
pub struct Overlay {
    pub list: String,
    pub x: i32,
    pub y: i32,
}

pub struct Settings {
    /// Output height limit; 0 keeps the cropped source height.
    pub max_height: u32,
    pub crf: u8,
    /// Frame rate for still overlay inputs, as FFmpeg's rational text.
    pub fps: String,
    /// Directory holding `segment{i}.cube` for each color-adjusted segment `i`.
    pub luts: Option<String>,
}

/// File name of segment `index`'s color lookup table inside `Settings::luts`.
pub fn lut_name(index: usize) -> String {
    format!("segment{index}.cube")
}

/// Arguments with `{sourceN}` (project source N) and `{output}` placeholders.
pub struct Job {
    pub args: Vec<String>,
    pub duration: i64,
    pub size: (u32, u32),
}

impl Job {
    pub fn command(&self) -> String {
        std::iter::once("ffmpeg".to_owned())
            .chain(self.args.iter().map(|a| {
                if a.contains([' ', ';', '\'', '[']) {
                    format!("\"{a}\"")
                } else {
                    a.clone()
                }
            }))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn sec(ms: i64) -> String {
    format!("{:.3}", ms as f64 / 1000.0)
}

/// Shortest decimal text of `value` with up to four places.
fn number(value: f64) -> String {
    let text = format!("{value:.4}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// `atempo` filters, each within 0.5 to 2, that play audio at `speed` with its pitch kept.
fn tempo(speed: f64) -> String {
    let mut chain = String::new();
    let mut left = speed;
    while left > 2.0 + 1e-9 {
        chain.push_str(",atempo=2");
        left /= 2.0;
    }
    while left < 0.5 - 1e-9 {
        chain.push_str(",atempo=0.5");
        left /= 0.5;
    }
    if (left - 1.0).abs() > 1e-6 {
        chain.push_str(&format!(",atempo={}", number(left)));
    }
    chain
}

pub fn job(project: &Project, overlays: &[Overlay], settings: &Settings) -> Job {
    let length = project.length();
    let audio = project.has_audio() && project.audio.keep;
    let count = project.segments.len();
    let used = project.used_sources();
    // Anything besides the opened video alone is fitted to the canvas with common formats.
    let uniform = used == [0];
    let (cw, ch) = (project.width.max(2) & !1, project.height.max(2) & !1);
    let mut args: Vec<String> = ["-v", "error", "-nostdin", "-y", "-threads", "4"]
        .map(String::from)
        .to_vec();
    let mut graph: Vec<String> = vec![];
    // Input index and earliest source time of each used source.
    let mut bases = vec![];
    for (k, &n) in used.iter().enumerate() {
        let users: Vec<usize> = (0..count).filter(|&i| project.segments[i].source == n).collect();
        let base = users.iter().map(|&i| project.segments[i].source_in).min().unwrap_or(0);
        if base > 0 {
            args.extend(["-ss".into(), sec(base)]);
        }
        args.extend(["-i".into(), format!("{{source{n}}}")]);
        let has_audio = audio && project.sources.get(n).is_some_and(|v| v.has_audio);
        if users.len() > 1 {
            let labels = |prefix: &str| users.iter().map(|i| format!("[{prefix}{i}]")).collect::<String>();
            graph.push(format!("[{k}:v]split={}{}", users.len(), labels("v")));
            if has_audio {
                graph.push(format!("[{k}:a]asplit={}{}", users.len(), labels("a")));
            }
        }
        bases.push((n, base, users.len() > 1, has_audio));
    }
    let mut pieces = String::new();
    for (i, segment) in project.segments.iter().enumerate() {
        let k = used.iter().position(|&n| n == segment.source).expect("used source");
        let (_, base, shared, has_audio) = bases[k];
        let (from, to) = (sec(segment.source_in - base), sec(segment.source_out - base));
        let color = if segment.white { ":color=white" } else { "" };
        let speed = segment.speed();
        let sped = (speed - 1.0).abs() > 1e-6;
        let mut video = format!(
            "{}trim=start={from}:end={to},setpts={}",
            if shared { format!("[v{i}]") } else { format!("[{k}:v]") },
            if sped { format!("(PTS-STARTPTS)/{}", number(speed)) } else { "PTS-STARTPTS".into() }
        );
        if sped && uniform {
            video.push_str(&format!(",fps={}", settings.fps));
        }
        let mut sound = if has_audio {
            format!(
                "{}atrim=start={from}:end={to},asetpts=PTS-STARTPTS{}",
                if shared { format!("[a{i}]") } else { format!("[{k}:a]") },
                tempo(speed)
            )
        } else {
            format!("anullsrc=r=48000:cl=stereo,atrim=duration={}", sec(segment.length()))
        };
        if !uniform {
            video.push_str(&format!(
                ",scale={cw}:{ch}:force_original_aspect_ratio=decrease:force_divisible_by=2:flags=bicubic,\
                 pad={cw}:{ch}:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,fps={},format=yuv420p",
                settings.fps
            ));
            sound.push_str(",aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo");
        }
        if let Some(dir) = settings.luts.as_ref().filter(|_| !segment.color.is_neutral()) {
            video.push_str(&format!(
                ",format=gbrp16le,lut3d=file='{dir}/{}':interp=tetrahedral,format=yuv420p",
                lut_name(i)
            ));
        }
        let mut fades = vec![];
        if segment.fade_in > 0 {
            fades.push(("in", 0, segment.fade_in));
        }
        if segment.fade_out > 0 {
            fades.push(("out", segment.length() - segment.fade_out, segment.fade_out));
        }
        for (kind, start, duration) in fades {
            let (start, duration) = (sec(start.max(0)), sec(duration));
            video.push_str(&format!(",fade=t={kind}:st={start}:d={duration}{color}"));
            sound.push_str(&format!(",afade=t={kind}:st={start}:d={duration}"));
        }
        graph.push(format!("{video}[s{i}]"));
        pieces.push_str(&format!("[s{i}]"));
        if audio {
            graph.push(format!("{sound}[t{i}]"));
            pieces.push_str(&format!("[t{i}]"));
        }
    }
    graph.push(format!(
        "{pieces}concat=n={count}:v=1:a={}[cv]{}",
        audio as u8,
        if audio { "[ca]" } else { "" }
    ));
    let (width, height) = project.output_size(settings.max_height);
    let mut chain = vec![];
    let crop = project.crop;
    if !crop.full() {
        chain.push(format!(
            "crop=w='max(2,trunc(iw*{:.8}/2)*2)':h='max(2,trunc(ih*{:.8}/2)*2)':x='trunc(iw*{:.8}/2)*2':y='trunc(ih*{:.8}/2)*2'",
            crop.width(),
            crop.height(),
            crop.left,
            crop.top
        ));
    }
    if (width, height) != project.output_size(0) {
        chain.push(format!("scale={width}:{height}:flags=bicubic"));
    }
    let mut current = "[cv]".to_owned();
    if !chain.is_empty() {
        graph.push(format!("[cv]{}[base]", chain.join(",")));
        current = "[base]".into();
    }
    let matrix = if height >= 720 { "bt709" } else { "bt601" };
    let mut input = used.len();
    for (n, overlay) in overlays.iter().enumerate() {
        args.extend(["-threads", "1", "-f", "concat", "-safe", "0", "-i", &overlay.list].map(String::from));
        graph.push(format!(
            "[{input}:v]format=rgba,scale=out_color_matrix={matrix},format=yuva420p[o{n}]"
        ));
        graph.push(format!(
            "{current}[o{n}]overlay=x={}:y={}:eof_action=repeat[b{n}]",
            overlay.x, overlay.y
        ));
        current = format!("[b{n}]");
        input += 1;
    }
    graph.push(format!("{current}format=yuv420p[vout]"));
    if audio {
        let mut sound = vec![];
        if (project.audio.volume - 1.0).abs() > 0.001 {
            sound.push(format!("volume={:.3}", project.audio.volume.max(0.0)));
        }
        if project.audio.fade_in > 0 {
            sound.push(format!("afade=t=in:st=0:d={}", sec(project.audio.fade_in)));
        }
        if project.audio.fade_out > 0 {
            sound.push(format!(
                "afade=t=out:st={}:d={}",
                sec((length - project.audio.fade_out).max(0)),
                sec(project.audio.fade_out)
            ));
        }
        if sound.is_empty() {
            sound.push("anull".into());
        }
        graph.push(format!("[ca]{}[aout]", sound.join(",")));
    }
    args.extend(["-filter_complex".into(), graph.join(";")]);
    args.extend(["-map", "[vout]"].map(String::from));
    if audio {
        args.extend(["-map", "[aout]", "-c:a", "aac", "-b:a", "192k"].map(String::from));
    } else {
        args.push("-an".into());
    }
    args.extend(
        [
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            &settings.crf.to_string(),
            "-pix_fmt",
            "yuv420p",
            "-map_metadata",
            "0",
            "-movflags",
            "+faststart",
            "-progress",
            "pipe:1",
            "-nostats",
            "{output}",
        ]
        .map(String::from),
    );
    Job {
        args,
        duration: length,
        size: (width, height),
    }
}

/// ffconcat list showing each `(file, start, end)` in its span and `blank` elsewhere until `length`.
pub fn track_list(frames: &[(String, i64, i64)], blank: &str, length: i64) -> String {
    let quote = |name: &str| format!("file '{}'\n", name.replace('\'', "'\\''"));
    let mut list = String::from("ffconcat version 1.0\n");
    let mut at = 0;
    for (file, start, end) in frames {
        let (start, end) = ((*start).max(at), (*end).min(length));
        if end <= start {
            continue;
        }
        if start > at {
            list.push_str(&quote(blank));
            list.push_str(&format!("duration {}\n", sec(start - at)));
        }
        list.push_str(&quote(file));
        list.push_str(&format!("duration {}\n", sec(end - start)));
        at = end;
    }
    list.push_str(&quote(blank));
    list.push_str(&format!("duration {}\n", sec((length - at).max(1))));
    list
}

/// Arguments that write the timeline's audio as 16 kHz mono WAV for speech recognition; `None` without audio.
pub fn speech_job(project: &Project) -> Option<Job> {
    if !project.has_audio() {
        return None;
    }
    let mut args: Vec<String> = ["-v", "error", "-nostdin", "-y", "-threads", "2"].map(String::from).to_vec();
    let mut graph = vec![];
    let mut inputs: Vec<(usize, i64, Vec<usize>)> = vec![];
    for n in (0..project.sources.len()).filter(|&n| project.sources[n].has_audio) {
        let users: Vec<usize> = (0..project.segments.len()).filter(|&i| project.segments[i].source == n).collect();
        let Some(base) = users.iter().map(|&i| project.segments[i].source_in).min() else {
            continue;
        };
        if base > 0 {
            args.extend(["-ss".into(), sec(base)]);
        }
        args.extend(["-vn".into(), "-i".into(), format!("{{source{n}}}")]);
        let k = inputs.len();
        if users.len() > 1 {
            graph.push(format!("[{k}:a]asplit={}{}", users.len(), users.iter().map(|i| format!("[a{i}]")).collect::<String>()));
        }
        inputs.push((n, base, users));
    }
    let mut pieces = String::new();
    for (i, segment) in project.segments.iter().enumerate() {
        let format = "aresample=16000,aformat=sample_fmts=s16:channel_layouts=mono";
        let piece = match inputs.iter().enumerate().find(|(_, (n, ..))| *n == segment.source) {
            Some((k, (_, base, users))) => format!(
                "{}atrim=start={}:end={},asetpts=PTS-STARTPTS{},{format}",
                if users.len() > 1 { format!("[a{i}]") } else { format!("[{k}:a]") },
                sec(segment.source_in - base),
                sec(segment.source_out - base),
                tempo(segment.speed())
            ),
            None => format!("anullsrc=r=16000:cl=mono,atrim=duration={},{format}", sec(segment.length())),
        };
        graph.push(format!("{piece}[t{i}]"));
        pieces.push_str(&format!("[t{i}]"));
    }
    graph.push(format!("{pieces}concat=n={}:v=0:a=1[aout]", project.segments.len()));
    args.extend(["-filter_complex".into(), graph.join(";")]);
    args.extend(
        ["-map", "[aout]", "-c:a", "pcm_s16le", "-f", "wav", "-progress", "pipe:1", "-nostats", "{output}"]
            .map(String::from),
    );
    Some(Job {
        args,
        duration: project.length(),
        size: (0, 0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Crop;
    use crate::studio::{Source, TextStyle};

    fn settings() -> Settings {
        Settings {
            max_height: 0,
            crf: 18,
            fps: "30".into(),
            luts: None,
        }
    }

    fn graph(job: &Job) -> &str {
        let at = job.args.iter().position(|a| a == "-filter_complex").unwrap();
        &job.args[at + 1]
    }

    #[test]
    fn single_segment_keeps_audio_and_placeholders() {
        let project = Project::new("x", "x.mp4", 4000, 640, 360);
        let job = job(&project, &[], &settings());
        assert_eq!(job.duration, 4000);
        assert!(job.args.contains(&"{source0}".to_owned()) && job.args.last().unwrap() == "{output}");
        assert!(!job.args.contains(&"-ss".to_owned()));
        assert_eq!(
            graph(&job),
            "[0:v]trim=start=0.000:end=4.000,setpts=PTS-STARTPTS[s0];\
             [0:a]atrim=start=0.000:end=4.000,asetpts=PTS-STARTPTS[t0];\
             [s0][t0]concat=n=1:v=1:a=1[cv][ca];[cv]format=yuv420p[vout];[ca]anull[aout]"
        );
    }

    #[test]
    fn cuts_fades_crop_and_overlays_compose_in_order() {
        let mut project = Project::new("x", "x.mp4", 10_000, 1920, 1080);
        project.split(2000);
        project.split(6000);
        project.remove_segment(0);
        project.segments[0].fade_in = 500;
        project.segments[1].fade_out = 1000;
        project.segments[1].white = true;
        project.crop = Crop::aspect(1920, 1080, 1.0);
        project.audio.volume = 0.5;
        project.audio.fade_out = 800;
        let job = job(
            &project,
            &[
                Overlay {
                    list: "/c/t0.ffconcat".into(),
                    x: 120,
                    y: 500,
                },
                Overlay {
                    list: "/c/t1.ffconcat".into(),
                    x: 40,
                    y: 600,
                },
            ],
            &Settings {
                max_height: 720,
                ..settings()
            },
        );
        assert_eq!(job.duration, 8000);
        assert_eq!(job.size, (720, 720));
        let ss = job.args.iter().position(|a| a == "-ss").unwrap();
        assert_eq!(job.args[ss + 1], "2.000");
        let list = job.args.iter().position(|a| a == "/c/t1.ffconcat").unwrap();
        assert_eq!(job.args[list - 7..list], ["-threads", "1", "-f", "concat", "-safe", "0", "-i"]);
        let g = graph(&job);
        for part in [
            "[0:v]split=2[v0][v1]",
            "[v0]trim=start=0.000:end=4.000,setpts=PTS-STARTPTS,fade=t=in:st=0.000:d=0.500[s0]",
            "[v1]trim=start=4.000:end=8.000,setpts=PTS-STARTPTS,fade=t=out:st=3.000:d=1.000:color=white[s1]",
            "[a1]atrim=start=4.000:end=8.000,asetpts=PTS-STARTPTS,afade=t=out:st=3.000:d=1.000[t1]",
            "[s0][t0][s1][t1]concat=n=2:v=1:a=1[cv][ca]",
            "[cv]crop=w='max(2,trunc(iw*0.56250000/2)*2)'",
            "scale=720:720:flags=bicubic[base]",
            "[1:v]format=rgba,scale=out_color_matrix=bt709,format=yuva420p[o0]",
            "[base][o0]overlay=x=120:y=500:eof_action=repeat[b0]",
            "[2:v]format=rgba,scale=out_color_matrix=bt709,format=yuva420p[o1]",
            "[b0][o1]overlay=x=40:y=600:eof_action=repeat[b1]",
            "[b1]format=yuv420p[vout]",
            "[ca]volume=0.500,afade=t=out:st=7.200:d=0.800[aout]",
        ] {
            assert!(g.contains(part), "Missing {part}\nin {g}");
        }
    }

    #[test]
    fn track_lists_fill_gaps_with_the_blank_frame() {
        let frames = [
            ("a.png".to_owned(), 500, 1200),
            ("b.png".to_owned(), 1200, 2000),
            ("c's.png".to_owned(), 4000, 9000),
        ];
        assert_eq!(
            track_list(&frames, "blank.png", 6000),
            "ffconcat version 1.0\nfile 'blank.png'\nduration 0.500\nfile 'a.png'\nduration 0.700\n\
             file 'b.png'\nduration 0.800\nfile 'blank.png'\nduration 2.000\nfile 'c'\\''s.png'\nduration 2.000\n\
             file 'blank.png'\nduration 0.001\n"
        );
    }

    #[test]
    fn speech_audio_follows_the_cuts_with_silence_for_silent_videos() {
        let mut project = Project::new("x", "x.mp4", 10_000, 1920, 1080);
        assert!(speech_job(&Project { sources: vec![Source::default()], ..project.clone() }).is_none());
        project.split(4000);
        project.insert_video(
            Source {
                uri: "y".into(),
                duration: 2000,
                has_audio: false,
                ..Source::default()
            },
            4000,
        );
        project.trim_segment(0, true, 1000);
        let speech = speech_job(&project).unwrap();
        assert_eq!(speech.duration, 11_000);
        assert_eq!(speech.args.iter().filter(|a| a.starts_with("{source")).collect::<Vec<_>>(), ["{source0}"]);
        let ss = speech.args.iter().position(|a| a == "-ss").unwrap();
        assert_eq!(speech.args[ss + 1..ss + 5], ["1.000", "-vn", "-i", "{source0}"]);
        assert_eq!(
            graph(&speech),
            "[0:a]asplit=2[a0][a2];\
             [a0]atrim=start=0.000:end=3.000,asetpts=PTS-STARTPTS,aresample=16000,aformat=sample_fmts=s16:channel_layouts=mono[t0];\
             anullsrc=r=16000:cl=mono,atrim=duration=2.000,aresample=16000,aformat=sample_fmts=s16:channel_layouts=mono[t1];\
             [a2]atrim=start=3.000:end=9.000,asetpts=PTS-STARTPTS,aresample=16000,aformat=sample_fmts=s16:channel_layouts=mono[t2];\
             [t0][t1][t2]concat=n=3:v=0:a=1[aout]"
        );
        assert!(speech.args.ends_with(&["-f", "wav", "-progress", "pipe:1", "-nostats", "{output}"].map(String::from)));
    }

    #[test]
    fn silent_sources_drop_the_audio_graph() {
        let mut project = Project::new("x", "x.webm", 3000, 640, 360);
        project.sources[0].has_audio = false;
        project.split(1000);
        let job = job(&project, &[], &settings());
        assert!(job.args.contains(&"-an".to_owned()));
        assert!(!graph(&job).contains("[0:a]") && graph(&job).contains("concat=n=2:v=1:a=0[cv];"));
    }

    #[test]
    fn added_videos_get_their_own_inputs_fitted_to_the_canvas() {
        let mut project = Project::new("x", "x.mp4", 10_000, 1920, 1080);
        project.split(4000);
        let portrait = Source {
            uri: "y".into(),
            duration: 3000,
            width: 1080,
            height: 1920,
            has_audio: false,
            ..Source::default()
        };
        project.insert_video(portrait, 4000);
        project.trim_segment(1, true, 500);
        let mixed = job(&project, &[], &settings());
        assert_eq!(mixed.duration, 12_500);
        let inputs: Vec<&String> = mixed.args.iter().filter(|a| a.starts_with("{source")).collect();
        assert_eq!(inputs, ["{source0}", "{source1}"]);
        let ss = mixed.args.iter().position(|a| a == "-ss").unwrap();
        assert_eq!(mixed.args[ss + 1..ss + 4], ["0.500", "-i", "{source1}"]);
        let g = graph(&mixed);
        for part in [
            "[0:v]split=2[v0][v2]",
            "[0:a]asplit=2[a0][a2]",
            "[1:v]trim=start=0.000:end=2.500,setpts=PTS-STARTPTS,scale=1920:1080:force_original_aspect_ratio=decrease",
            "pad=1920:1080:(ow-iw)/2:(oh-ih)/2:color=black,setsar=1,fps=30,format=yuv420p[s1]",
            "anullsrc=r=48000:cl=stereo,atrim=duration=2.500,aresample=48000",
            "[v2]trim=start=4.000:end=10.000",
            "[a2]atrim=start=4.000:end=10.000,asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[t2]",
            "[s0][t0][s1][t1][s2][t2]concat=n=3:v=1:a=1[cv][ca]",
        ] {
            assert!(g.contains(part), "Missing {part}\nin {g}");
        }
        project.remove_segment(0);
        project.remove_segment(1);
        let alone = job(&project, &[], &settings());
        assert!(graph(&alone).contains("[0:v]trim=start=0.000:end=2.500,setpts=PTS-STARTPTS,scale="), "A lone added video is still fitted");
        assert!(alone.args.contains(&"-an".to_owned()), "No source left with audio");
    }

    fn ffmpeg(args: &[&str]) -> Vec<u8> {
        let output = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y"])
            .args(args)
            .output()
            .expect("ffmpeg runs");
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        output.stdout
    }

    /// RGB of the frame at `seconds`, sampled at a fractional position.
    fn sample(path: &str, seconds: f64, x: f32, y: f32) -> [u8; 3] {
        let raw = ffmpeg(&[
            "-ss", &format!("{seconds:.3}"), "-i", path, "-frames:v", "1",
            "-f", "rawvideo", "-pix_fmt", "rgb24", "-",
        ]);
        let (w, h) = (360usize, 360usize);
        assert_eq!(raw.len(), w * h * 3);
        let (px, py) = ((x * w as f32) as usize, (y * h as f32) as usize);
        let at = (py * w + px) * 3;
        [raw[at], raw[at + 1], raw[at + 2]]
    }

    #[test]
    fn desktop_ffmpeg_joins_videos_of_other_sizes_and_silent_ones() {
        if std::process::Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("luma-join-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (main, added, output) = (dir.join("main.mp4"), dir.join("added.mp4"), dir.join("out.mp4"));
        let path = |p: &std::path::Path| p.to_str().unwrap().to_owned();
        ffmpeg(&[
            "-f", "lavfi", "-i", "color=c=blue:s=640x360:r=30:d=4",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=4",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", &path(&main),
        ]);
        ffmpeg(&[
            "-f", "lavfi", "-i", "color=c=red:s=180x320:r=25:d=2",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", &path(&added),
        ]);
        let mut project = Project::new(&path(&main), "main.mp4", 4000, 640, 360);
        project.split(2000);
        project.insert_video(
            Source {
                uri: path(&added),
                duration: 2000,
                width: 180,
                height: 320,
                has_audio: false,
                probed: true,
                ..Source::default()
            },
            2000,
        );
        let job = job(&project, &[], &settings());
        let args: Vec<String> = job
            .args
            .iter()
            .map(|a| match a.as_str() {
                "{source0}" => path(&main),
                "{source1}" => path(&added),
                "{output}" => path(&output),
                _ => a.clone(),
            })
            .collect();
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        ffmpeg(&refs);
        let output = path(&output);
        let probe = String::from_utf8(
            std::process::Command::new("ffprobe")
                .args(["-v", "error", "-show_entries", "format=duration:stream=codec_type,width,height", "-of", "csv=p=0", &output])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        assert!(probe.contains("video,640,360") && probe.contains("audio"), "{probe}");
        let duration: f64 = probe.lines().last().unwrap().trim().parse().unwrap();
        assert!((duration - 6.0).abs() < 0.1, "Duration {duration}");
        let frame = |seconds: f64, x: f32| {
            let raw = ffmpeg(&[
                "-ss", &format!("{seconds:.3}"), "-i", &output, "-frames:v", "1",
                "-f", "rawvideo", "-pix_fmt", "rgb24", "-",
            ]);
            let at = (180 * 640 + (x * 640.0) as usize) * 3;
            [raw[at], raw[at + 1], raw[at + 2]]
        };
        assert!(frame(1.0, 0.5)[2] > 180, "Main video first");
        let red = frame(3.0, 0.5);
        assert!(red[0] > 180 && red[2] < 60, "Added video in the middle: {red:?}");
        assert!(frame(3.0, 0.1).iter().all(|c| *c < 30), "Letterboxed sides are black");
        assert!(frame(5.0, 0.5)[2] > 180, "Main video resumes");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tempo_chains_stay_within_atempo_limits() {
        assert_eq!(tempo(1.0), "");
        assert_eq!(tempo(1.5), ",atempo=1.5");
        assert_eq!(tempo(4.0), ",atempo=2,atempo=2");
        assert_eq!(tempo(3.0), ",atempo=2,atempo=1.5");
        assert_eq!(tempo(0.25), ",atempo=0.5,atempo=0.5");
    }

    #[test]
    fn desktop_ffmpeg_plays_segments_at_their_speed() {
        if std::process::Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("luma-speed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (main, output) = (dir.join("main.mp4"), dir.join("out.mp4"));
        let path = |p: &std::path::Path| p.to_str().unwrap().to_owned();
        ffmpeg(&[
            "-f", "lavfi", "-i", "sine=frequency=440:duration=6",
            "-filter_complex",
            "color=c=red:s=320x180:r=30:d=2[r];color=c=lime:s=320x180:r=30:d=2[g];color=c=blue:s=320x180:r=30:d=2[b];[r][g][b]concat=n=3[v]",
            "-map", "[v]", "-map", "0:a", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", &path(&main),
        ]);
        let mut project = Project::new(&path(&main), "main.mp4", 6000, 320, 180);
        project.split(2000);
        project.split(4000);
        project.set_speed(0, 2.0);
        project.set_speed(2, 0.5);
        assert_eq!(project.length(), 7000);
        let job = job(&project, &[], &settings());
        let graph = graph(&job).to_owned();
        assert!(graph.contains("setpts=(PTS-STARTPTS)/2,fps=30") && graph.contains("asetpts=PTS-STARTPTS,atempo=2"), "{graph}");
        assert!(graph.contains("setpts=(PTS-STARTPTS)/0.5,fps=30") && graph.contains("atempo=0.5"), "{graph}");
        let args: Vec<String> = job
            .args
            .iter()
            .map(|a| match a.as_str() {
                "{source0}" => path(&main),
                "{output}" => path(&output),
                _ => a.clone(),
            })
            .collect();
        ffmpeg(&args.iter().map(String::as_str).collect::<Vec<_>>());
        let output = path(&output);
        let probe = String::from_utf8(
            std::process::Command::new("ffprobe")
                .args(["-v", "error", "-show_entries", "stream=codec_type,duration", "-of", "csv=p=0", &output])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        for line in probe.lines() {
            let duration: f64 = line.split(',').nth(1).unwrap().parse().unwrap();
            assert!((duration - 7.0).abs() < 0.12, "{line}");
        }
        let color = |seconds: f64| {
            let raw = ffmpeg(&["-ss", &format!("{seconds:.3}"), "-i", &output, "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
            let at = (90 * 320 + 160) * 3;
            [raw[at], raw[at + 1], raw[at + 2]]
        };
        let strongest = |c: [u8; 3]| (0..3).max_by_key(|&i| c[i]).unwrap();
        for (seconds, channel) in [(0.5, 0), (1.3, 1), (2.8, 1), (3.3, 2), (6.6, 2)] {
            assert_eq!(strongest(color(seconds)), channel, "{seconds} s: {:?}", color(seconds));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn desktop_ffmpeg_colors_each_segment_like_the_preview() {
        if std::process::Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("luma-color-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (main, output) = (dir.join("main.mp4"), dir.join("out.mp4"));
        let path = |p: &std::path::Path| p.to_str().unwrap().to_owned();
        ffmpeg(&[
            "-f", "lavfi", "-i", "color=c=0x8c5a3c:s=320x180:r=30:d=4",
            "-c:v", "libx264", "-pix_fmt", "yuv420p", "-color_primaries", "bt709", "-color_trc", "bt709", "-colorspace", "bt709", &path(&main),
        ]);
        let mut project = Project::new(&path(&main), "main.mp4", 4000, 320, 180);
        project.sources[0].has_audio = false;
        project.split(2000);
        project.segments[1].color = crate::color::Adjust { exposure: 0.7, temperature: -0.6, saturation: 0.4, ..Default::default() };
        let cube = project.segments[1].color.cube();
        std::fs::write(dir.join(lut_name(1)), cube).unwrap();
        let job = job(&project, &[], &Settings { luts: Some(path(&dir)), ..settings() });
        assert!(graph(&job).contains(&format!("lut3d=file='{}/segment1.cube'", path(&dir))), "{}", graph(&job));
        assert!(!graph(&job).contains("segment0.cube"), "Neutral segments skip the table");
        let args: Vec<String> = job
            .args
            .iter()
            .map(|a| match a.as_str() {
                "{source0}" => path(&main),
                "{output}" => path(&output),
                _ => a.clone(),
            })
            .collect();
        ffmpeg(&args.iter().map(String::as_str).collect::<Vec<_>>());
        let pixel = |seconds: f64| {
            let raw = ffmpeg(&["-ss", &format!("{seconds:.3}"), "-i", &path(&output), "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "-"]);
            let at = (90 * 320 + 160) * 3;
            [raw[at], raw[at + 1], raw[at + 2]].map(|v| v as f32 / 255.0)
        };
        let before = pixel(1.0);
        let expected = project.segments[1].color.apply(before);
        let after = pixel(3.0);
        assert!((0..3).all(|i| (after[i] - expected[i]).abs() < 0.035), "Export {after:?}, preview math {expected:?} from {before:?}");
        assert!(after.iter().sum::<f32>() > before.iter().sum::<f32>() + 0.15, "Brighter: {before:?} -> {after:?}");
        assert!(((0..3).all(|i| (pixel(1.5)[i] - before[i]).abs() < 0.01)), "The first segment is untouched");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn desktop_ffmpeg_renders_cuts_fades_and_captions() {
        if std::process::Command::new("ffmpeg").arg("-version").output().is_err() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("luma-compose-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.mp4");
        let source = source.to_str().unwrap();
        // Solid blue for 4 s, then solid green; a 440 Hz tone throughout.
        ffmpeg(&[
            "-f", "lavfi", "-i", "color=c=blue:s=640x360:r=30:d=4",
            "-f", "lavfi", "-i", "color=c=green:s=640x360:r=30:d=4",
            "-f", "lavfi", "-i", "sine=frequency=440:duration=8",
            "-filter_complex", "[0:v][1:v]concat=n=2:v=1:a=0[v]",
            "-map", "[v]", "-map", "2:a", "-c:v", "libx264", "-pix_fmt", "yuv420p",
            "-c:a", "aac", source,
        ]);
        let mut project = Project::new(source, "source.mp4", 8000, 640, 360);
        project.split(3000);
        project.split(5000);
        project.remove_segment(1);
        project.segments[0].fade_in = 600;
        project.crop = Crop::aspect(640, 360, 1.0);
        let style = TextStyle {
            text: "LUMA".into(),
            background: [255, 0, 0, 255],
            outline: 0.0,
            size: 0.12,
            ..TextStyle::default()
        };
        let (w, h) = project.output_size(0);
        let book = crate::caption::FontBook::embedded();
        let first = crate::caption::render_text_with(&book, &style, w as f32, h as f32).image;
        let white = TextStyle {
            text: "TWO".into(),
            background: [255, 255, 255, 255],
            ..style
        };
        let second = crate::caption::render_text_with(&book, &white, w as f32, h as f32).image;
        // Both captions centered at (0.5, 0.25) on frames of their combined size.
        let (bw, bh) = (first.width().max(second.width()), first.height().max(second.height()));
        let frame = |image: &image::RgbaImage, name: &str| {
            let mut canvas = image::RgbaImage::new(bw, bh);
            let (x, y) = ((bw - image.width()) / 2, (bh - image.height()) / 2);
            image::imageops::replace(&mut canvas, image, x as i64, y as i64);
            canvas.save(dir.join(name)).unwrap();
            name.to_owned()
        };
        let frames = [(frame(&first, "one.png"), 1000, 2500), (frame(&second, "two.png"), 4000, 5000)];
        image::RgbaImage::new(bw, bh).save(dir.join("blank.png")).unwrap();
        let list = dir.join("track.ffconcat");
        std::fs::write(&list, track_list(&frames, "blank.png", project.length())).unwrap();
        let output = dir.join("out.mp4");
        let job = job(
            &project,
            &[Overlay {
                list: list.to_str().unwrap().into(),
                x: (w - bw) as i32 / 2,
                y: (h as f32 * 0.25) as i32 - bh as i32 / 2,
            }],
            &settings(),
        );
        let args: Vec<String> = job
            .args
            .iter()
            .map(|a| match a.as_str() {
                "{source0}" => source.to_owned(),
                "{output}" => output.to_str().unwrap().to_owned(),
                _ => a.clone(),
            })
            .collect();
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        ffmpeg(&refs);
        let output = output.to_str().unwrap();
        ffmpeg(&["-i", output, "-map", "0:a", "-f", "null", "-"]);
        let duration = String::from_utf8(
            std::process::Command::new("ffprobe")
                .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", output])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap();
        let duration: f64 = duration.trim().parse().unwrap();
        assert!((duration - 6.0).abs() < 0.1, "Duration {duration}");
        let dark = sample(output, 0.0, 0.5, 0.8);
        assert!(dark.iter().all(|c| *c < 40), "Fade starts black: {dark:?}");
        let blue = sample(output, 0.9, 0.5, 0.8);
        assert!(blue[2] > 180 && blue[1] < 60, "Blue source: {blue:?}");
        let caption = sample(output, 1.5, 0.5, 0.25);
        assert!(caption[0] > 200 && caption[2] < 60, "Red caption box: {caption:?}");
        let after = sample(output, 2.7, 0.5, 0.25);
        assert!(after[2] > 180, "Caption ends: {after:?}");
        let two = sample(output, 4.5, 0.5, 0.25);
        assert!(two.iter().all(|c| *c > 200), "Second caption from the same track: {two:?}");
        let gone = sample(output, 5.5, 0.5, 0.25);
        assert!(gone[1] > 100 && gone[0] < 60, "Blank frame after the second caption: {gone:?}");
        let cut = sample(output, 3.5, 0.5, 0.8);
        assert!(cut[1] > 100 && cut[2] < 60, "Green after the removed 3-5 s: {cut:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
