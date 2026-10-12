//! FFmpeg arguments that render a project: segment cuts, fades, crop, overlays and audio.
use crate::studio::{Clip, Motion, Project};

/// A rendered overlay image (rotation and opacity baked in) and the clip it shows.
pub struct Overlay {
    pub path: String,
    pub clip: Clip,
}

pub struct Settings {
    /// Output height limit; 0 keeps the cropped source height.
    pub max_height: u32,
    pub crf: u8,
    /// Frame rate for still overlay inputs, as FFmpeg's rational text.
    pub fps: String,
}

/// Arguments with `{source}` and `{output}` placeholders.
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

/// Offset expression in frame fractions for one axis, matching `Clip::envelope`.
fn travel(clip: &Clip, enter: i64, exit: i64, axis: usize) -> String {
    let pick = |m: Motion| {
        let (x, y) = m.travel();
        if axis == 0 { x } else { y }
    };
    let mut terms = vec![];
    let (tin, tout) = (pick(clip.enter), pick(clip.exit));
    if enter > 0 && tin != 0.0 {
        let p = format!("clip((t-{})/{},0,1)", sec(clip.start), sec(enter));
        terms.push(format!("-({tin})*(1-{p})*(1-{p})"));
    }
    if exit > 0 && tout != 0.0 {
        let q = format!("clip((t-{})/{},0,1)", sec(clip.end - exit), sec(exit));
        terms.push(format!("+({tout})*{q}*{q}"));
    }
    terms.concat()
}

pub fn job(project: &Project, overlays: &[Overlay], settings: &Settings) -> Job {
    let length = project.length();
    let base = project.segments.first().map_or(0, |s| s.source_in);
    let audio = project.has_audio && project.audio.keep;
    let count = project.segments.len();
    let mut args: Vec<String> = ["-v", "error", "-nostdin", "-y", "-threads", "4"]
        .map(String::from)
        .to_vec();
    if base > 0 {
        args.extend(["-ss".into(), sec(base)]);
    }
    args.extend(["-i".into(), "{source}".into()]);
    let mut graph: Vec<String> = vec![];
    let labels = |prefix: &str| (0..count).map(|i| format!("[{prefix}{i}]")).collect::<String>();
    if count > 1 {
        graph.push(format!("[0:v]split={count}{}", labels("v")));
        if audio {
            graph.push(format!("[0:a]asplit={count}{}", labels("a")));
        }
    }
    let mut pieces = String::new();
    for (i, segment) in project.segments.iter().enumerate() {
        let (from, to) = (sec(segment.source_in - base), sec(segment.source_out - base));
        let color = if segment.white { ":color=white" } else { "" };
        let mut video = format!(
            "{}trim=start={from}:end={to},setpts=PTS-STARTPTS",
            if count > 1 { format!("[v{i}]") } else { "[0:v]".into() }
        );
        let mut sound = format!(
            "{}atrim=start={from}:end={to},asetpts=PTS-STARTPTS",
            if count > 1 { format!("[a{i}]") } else { "[0:a]".into() }
        );
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
    let mut input = 1;
    for (n, overlay) in overlays.iter().enumerate() {
        let clip = &overlay.clip;
        let end = clip.end.min(length);
        if clip.start >= end {
            continue;
        }
        args.extend(
            [
                "-loop",
                "1",
                "-framerate",
                &settings.fps,
                "-t",
                &sec(end - clip.start),
                "-i",
                &overlay.path,
            ]
            .map(String::from),
        );
        let (enter, exit) = clip.motion_ms();
        let mut layer = format!("[{input}:v]format=rgba,setpts=PTS+{}/TB", sec(clip.start));
        if enter > 0 {
            layer.push_str(&format!(",fade=t=in:st={}:d={}:alpha=1", sec(clip.start), sec(enter)));
        }
        if exit > 0 {
            layer.push_str(&format!(
                ",fade=t=out:st={}:d={}:alpha=1",
                sec(clip.end - exit),
                sec(exit)
            ));
        }
        layer.push_str(&format!(",scale=out_color_matrix={matrix},format=yuva420p[o{n}]"));
        graph.push(layer);
        graph.push(format!(
            "{current}[o{n}]overlay=x='W*{:.6}-w/2+W*(0{})':y='H*{:.6}-h/2+H*(0{})':eof_action=pass:enable='gte(t,{})*lt(t,{})'[b{n}]",
            clip.x,
            travel(clip, enter, exit, 0),
            clip.y,
            travel(clip, enter, exit, 1),
            sec(clip.start),
            sec(end),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::Crop;
    use crate::studio::{Body, TextStyle};

    fn settings() -> Settings {
        Settings {
            max_height: 0,
            crf: 18,
            fps: "30".into(),
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
        assert!(job.args.contains(&"{source}".to_owned()) && job.args.last().unwrap() == "{output}");
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
        let clip = Clip {
            start: 1000,
            end: 3000,
            x: 0.5,
            y: 0.8,
            enter: Motion::SlideUp,
            enter_ms: 400,
            exit: Motion::Fade,
            exit_ms: 500,
            body: Body::Text(TextStyle::default()),
            ..Clip::default()
        };
        let late = Clip {
            start: 9000,
            end: 9500,
            ..clip.clone()
        };
        let job = job(
            &project,
            &[
                Overlay {
                    path: "/c/1.png".into(),
                    clip,
                },
                Overlay {
                    path: "/c/2.png".into(),
                    clip: late,
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
        assert_eq!(job.args.iter().filter(|a| *a == "-loop").count(), 1, "Late overlay skipped");
        let g = graph(&job);
        for part in [
            "[0:v]split=2[v0][v1]",
            "[v0]trim=start=0.000:end=4.000,setpts=PTS-STARTPTS,fade=t=in:st=0.000:d=0.500[s0]",
            "[v1]trim=start=4.000:end=8.000,setpts=PTS-STARTPTS,fade=t=out:st=3.000:d=1.000:color=white[s1]",
            "[a1]atrim=start=4.000:end=8.000,asetpts=PTS-STARTPTS,afade=t=out:st=3.000:d=1.000[t1]",
            "[s0][t0][s1][t1]concat=n=2:v=1:a=1[cv][ca]",
            "[cv]crop=w='max(2,trunc(iw*0.56250000/2)*2)'",
            "scale=720:720:flags=bicubic[base]",
            "[1:v]format=rgba,setpts=PTS+1.000/TB,fade=t=in:st=1.000:d=0.400:alpha=1,fade=t=out:st=2.500:d=0.500:alpha=1",
            "[base][o0]overlay=x='W*0.500000-w/2+W*(0)':y='H*0.800000-h/2+H*(0-(-0.08)*(1-clip((t-1.000)/0.400,0,1))*(1-clip((t-1.000)/0.400,0,1)))'",
            "enable='gte(t,1.000)*lt(t,3.000)'[b0]",
            "[b0]format=yuv420p[vout]",
            "[ca]volume=0.500,afade=t=out:st=7.200:d=0.800[aout]",
        ] {
            assert!(g.contains(part), "Missing {part}\nin {g}");
        }
    }

    #[test]
    fn silent_sources_drop_the_audio_graph() {
        let mut project = Project::new("x", "x.webm", 3000, 640, 360);
        project.has_audio = false;
        project.split(1000);
        let job = job(&project, &[], &settings());
        assert!(job.args.contains(&"-an".to_owned()));
        assert!(!graph(&job).contains("[0:a]") && graph(&job).contains("concat=n=2:v=1:a=0[cv];"));
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
        let clip = Clip {
            start: 1000,
            end: 2500,
            x: 0.5,
            y: 0.25,
            enter: Motion::None,
            exit: Motion::None,
            body: Body::Text(style.clone()),
            ..Clip::default()
        };
        let (w, h) = project.output_size(0);
        let rendered =
            crate::caption::render_text_with(&crate::caption::FontBook::embedded(), &style, w as f32, h as f32);
        let png = dir.join("caption.png");
        rendered.image.save(&png).unwrap();
        let output = dir.join("out.mp4");
        let job = job(
            &project,
            &[Overlay {
                path: png.to_str().unwrap().into(),
                clip,
            }],
            &settings(),
        );
        let args: Vec<String> = job
            .args
            .iter()
            .map(|a| match a.as_str() {
                "{source}" => source.to_owned(),
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
        let cut = sample(output, 3.5, 0.5, 0.8);
        assert!(cut[1] > 100 && cut[2] < 60, "Green after the removed 3-5 s: {cut:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
