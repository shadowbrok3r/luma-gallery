//! Words recognized by whisper.cpp and the caption clips built from them.
use crate::studio::{Body, Clip, TextStyle};
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// A pause longer than this ends a caption.
const PAUSE: i64 = 700;
/// Captions stay up until the next one when it starts within this long.
const HOLD: i64 = 1000;
/// Time a caption stays up after its last word before a longer pause.
const TAIL: i64 = 400;

/// A spoken word in timeline milliseconds.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Word {
    pub text: String,
    pub start: i64,
    pub end: i64,
}

/// Words recognized from the timeline's audio.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transcript {
    pub language: String,
    pub words: Vec<Word>,
    /// The timeline the words were recognized from, as `Project::ranges`.
    pub ranges: Vec<[i64; 3]>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptionMode {
    /// A few words at a time.
    #[default]
    Phrases,
    /// One word at a time.
    Words,
    /// A few words at a time with the spoken word in the highlight color.
    Highlight,
}

/// Whisper language codes offered for recognition, with "auto" first.
pub const LANGUAGES: [(&str, &str); 19] = [
    ("auto", "Detect"),
    ("en", "English"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("ru", "Russian"),
    ("uk", "Ukrainian"),
    ("tr", "Turkish"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("id", "Indonesian"),
    ("vi", "Vietnamese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
];

pub fn language_name(code: &str) -> &str {
    LANGUAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map_or(code, |(_, name)| name)
}

/// Whether the language separates words with spaces.
pub fn spaced(language: &str) -> bool {
    !matches!(language, "zh" | "ja" | "th" | "lo" | "my" | "km" | "yue" | "bo")
}

fn spoken(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

/// Words of whisper-cli `-ojf` output; tokens may split UTF-8 characters, so invalid bytes are kept as private-use characters until words are whole.
pub fn parse(bytes: &[u8]) -> Result<Transcript, String> {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.utf8_chunks() {
        text.push_str(chunk.valid());
        text.extend(chunk.invalid().iter().map(|&b| char::from_u32(0xF700 + b as u32).expect("private use")));
    }
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Unreadable transcript: {e}"))?;
    let language = value["result"]["language"].as_str().unwrap_or_default().to_owned();
    let spaced = spaced(&language);
    let mut words: Vec<(Vec<u8>, i64, i64)> = vec![];
    let tokens = value["transcription"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|segment| segment["tokens"].as_array().into_iter().flatten());
    for token in tokens {
        let piece = token["text"].as_str().unwrap_or_default();
        if piece.is_empty() || piece.starts_with("[_") && piece.ends_with(']') {
            continue;
        }
        let mut raw = vec![];
        for c in piece.chars() {
            match c as u32 {
                code @ 0xF700..=0xF7FF => raw.push((code - 0xF700) as u8),
                _ => raw.extend(c.to_string().bytes()),
            }
        }
        let (from, to) = (
            token["offsets"]["from"].as_i64().unwrap_or(0),
            token["offsets"]["to"].as_i64().unwrap_or(0),
        );
        let says = spoken(&String::from_utf8_lossy(&raw)) || std::str::from_utf8(&raw).is_err();
        let incomplete = words.last().is_some_and(|(current, ..)| std::str::from_utf8(current).is_err());
        let fresh = match words.last() {
            None => true,
            Some(_) if incomplete => false,
            Some(_) => says && (!spaced || raw[0] == b' '),
        };
        if fresh {
            words.push((raw, from, to.max(from)));
        } else if let Some(word) = words.last_mut() {
            word.0.extend(raw);
            if says {
                word.2 = word.2.max(to);
            }
        }
    }
    let words = words
        .into_iter()
        .map(|(raw, start, end)| Word {
            text: String::from_utf8_lossy(&raw).trim().to_owned(),
            start,
            end,
        })
        .filter(|w| spoken(&w.text))
        .collect();
    Ok(Transcript {
        language,
        words,
        ranges: vec![],
    })
}

/// Groups of at most `max` words shown together, ending at sentences, long pauses and late clauses.
pub fn phrases(words: &[Word], max: usize) -> Vec<Range<usize>> {
    let max = max.max(1);
    let mut groups = vec![];
    let mut first = 0;
    for i in 1..=words.len() {
        let ends = i == words.len() || {
            let (previous, next) = (&words[i - 1], &words[i]);
            let count = i - first;
            let sentence = previous.text.ends_with(['.', '!', '?', '…', '。', '！', '？']);
            let clause = previous.text.ends_with([',', ';', ':', '、', '，']) && count * 2 >= max;
            count >= max || next.start - previous.end > PAUSE || sentence || clause
        };
        if ends {
            groups.push(first..i);
            first = i;
        }
    }
    groups
}

/// Caption clips in timeline order with `template`'s placement and style; clips of one group share a `phrase` number.
pub fn clips(transcript: &Transcript, mode: CaptionMode, max: usize, template: &Clip, length: i64) -> Vec<Clip> {
    let words = &transcript.words;
    let groups = phrases(words, if mode == CaptionMode::Words { 1 } else { max });
    let separator = if spaced(&transcript.language) { " " } else { "" };
    let style = template.text().cloned().unwrap_or_default();
    let mut clips: Vec<Clip> = vec![];
    for (n, group) in groups.iter().enumerate() {
        let start = words[group.start].start.clamp(0, length);
        let last = words[group.end - 1].end;
        let next = groups.get(n + 1).map_or(length, |g| words[g.start].start).min(length);
        let end = if next - last < HOLD { next } else { (last + TAIL).min(next) };
        let mut text = String::new();
        let mut spans = vec![];
        for word in &words[group.clone()] {
            if !text.is_empty() {
                text.push_str(separator);
            }
            let at = text.chars().count();
            text.push_str(&word.text);
            spans.push([at, at + word.text.chars().count()]);
        }
        let mut make = |start: i64, end: i64, highlight: Option<[usize; 2]>| {
            let start = start.max(clips.last().map_or(0, |c| c.end));
            if end > start {
                clips.push(Clip {
                    id: 0,
                    start,
                    end,
                    phrase: n as u64 + 1,
                    body: Body::Text(TextStyle {
                        text: text.clone(),
                        highlight,
                        ..style.clone()
                    }),
                    ..template.clone()
                });
            }
        };
        if mode == CaptionMode::Highlight {
            for (k, j) in group.clone().enumerate() {
                let from = if k == 0 { start } else { words[j].start };
                let to = if j + 1 == group.end { end } else { words[j + 1].start.min(end) };
                make(from, to, Some(spans[k]));
            }
        } else {
            make(start, end, None);
        }
    }
    clips
}

/// Moves a highlighted character range from `old` text to the same word of `new` text.
pub fn rehighlight(old: &str, new: &str, range: [usize; 2]) -> Option<[usize; 2]> {
    let index = word_spans(old).iter().position(|s| s[0] <= range[0] && range[0] < s[1].max(s[0] + 1))?;
    word_spans(new).get(index).copied()
}

/// Character ranges of whitespace-separated words.
pub fn word_spans(text: &str) -> Vec<[usize; 2]> {
    let mut spans: Vec<[usize; 2]> = vec![];
    let mut inside = false;
    for (i, c) in text.chars().enumerate() {
        if c.is_whitespace() {
            inside = false;
        } else if inside {
            spans.last_mut().expect("inside a word")[1] = i + 1;
        } else {
            spans.push([i, i + 1]);
            inside = true;
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, start: i64, end: i64) -> Word {
        Word {
            text: text.into(),
            start,
            end,
        }
    }

    fn token(text: &str, from: i64, to: i64) -> String {
        format!(r#"{{"text": "{text}", "offsets": {{"from": {from}, "to": {to}}}}}"#)
    }

    fn output(language: &str, tokens: &[String]) -> Vec<u8> {
        format!(
            r#"{{"result": {{"language": "{language}"}}, "transcription": [{{"offsets": {{"from": 0, "to": 9000}}, "tokens": [{}]}}]}}"#,
            tokens.join(",")
        )
        .into_bytes()
    }

    #[test]
    fn tokens_join_into_words_with_punctuation_and_special_tokens_dropped() {
        let json = output(
            "en",
            &[
                token("[_BEG_]", 0, 0),
                token(" And", 220, 220),
                token(" fell", 680, 900),
                token("ow", 900, 1170),
                token(" Americans", 1170, 1980),
                token(",", 1980, 2890),
                token(" ask", 3290, 3650),
                token(" -", 3650, 3700),
                token("[_TT_530]", 10600, 10600),
            ],
        );
        let transcript = parse(&json).unwrap();
        assert_eq!(transcript.language, "en");
        assert_eq!(
            transcript.words,
            [
                word("And", 220, 220),
                word("fellow", 680, 1170),
                word("Americans,", 1170, 1980),
                word("ask -", 3290, 3650)
            ]
        );
    }

    #[test]
    fn split_utf8_characters_rejoin_and_unspaced_languages_split_per_token() {
        // "café" with the two bytes of é in separate tokens.
        let mut json = output("fr", &[token(" caf\u{1}", 0, 300), token("\u{2}", 300, 400), token(" noir", 500, 800)]);
        for b in json.iter_mut() {
            *b = match *b {
                1 => 0xC3,
                2 => 0xA9,
                other => other,
            };
        }
        let transcript = parse(&json).unwrap();
        assert_eq!(transcript.words, [word("café", 0, 400), word("noir", 500, 800)]);
        let japanese = parse(&output("ja", &[token("こんにちは", 0, 600), token("世界", 600, 900), token("。", 900, 950)])).unwrap();
        assert_eq!(japanese.words, [word("こんにちは", 0, 600), word("世界。", 600, 900)]);
        // 世 (E4 B8 96) split after its second byte.
        let mut split = output("zh", &[token("\u{3}\u{4}", 0, 100), token("\u{5}", 100, 200), token("界", 200, 300)]);
        for b in split.iter_mut() {
            *b = match *b {
                3 => 0xE4,
                4 => 0xB8,
                5 => 0x96,
                other => other,
            };
        }
        assert_eq!(parse(&split).unwrap().words, [word("世", 0, 200), word("界", 200, 300)]);
    }

    #[test]
    fn phrases_break_at_the_limit_pauses_and_sentences() {
        let words = [
            word("one", 0, 200),
            word("two", 250, 400),
            word("three.", 450, 600),
            word("four", 700, 900),
            word("five", 950, 1100),
            word("six", 1150, 1300),
            word("seven", 1350, 1500),
            word("eight", 2600, 2800),
        ];
        assert_eq!(phrases(&words, 3), [0..3, 3..6, 6..7, 7..8]);
        assert_eq!(phrases(&words, 1).len(), 8);
        let commas = [word("well,", 0, 100), word("you", 150, 200), word("know,", 250, 300), word("right", 350, 400)];
        assert_eq!(phrases(&commas, 4), [0..3, 3..4], "Clauses end phrases past half the limit");
    }

    #[test]
    fn clips_hold_over_short_pauses_and_highlight_each_word() {
        let transcript = Transcript {
            language: "en".into(),
            words: vec![word("Hello", 100, 400), word("there", 450, 700), word("friend", 3000, 3300)],
            ranges: vec![],
        };
        let template = Clip {
            y: 0.8,
            body: Body::Text(TextStyle {
                size: 0.05,
                ..TextStyle::default()
            }),
            ..Clip::default()
        };
        let phrases = clips(&transcript, CaptionMode::Phrases, 4, &template, 5000);
        let spans: Vec<(i64, i64, &str)> =
            phrases.iter().map(|c| (c.start, c.end, c.text().unwrap().text.as_str())).collect();
        assert_eq!(spans, [(100, 1100, "Hello there"), (3000, 3700, "friend")]);
        assert!(phrases.iter().all(|c| c.y == 0.8 && c.text().unwrap().size == 0.05));
        let words = clips(&transcript, CaptionMode::Words, 4, &template, 5000);
        let spans: Vec<(i64, i64)> = words.iter().map(|c| (c.start, c.end)).collect();
        assert_eq!(spans, [(100, 450), (450, 1100), (3000, 3700)]);
        let marked = clips(&transcript, CaptionMode::Highlight, 4, &template, 3500);
        let spans: Vec<(i64, i64, Option<[usize; 2]>, u64)> =
            marked.iter().map(|c| (c.start, c.end, c.text().unwrap().highlight, c.phrase)).collect();
        assert_eq!(
            spans,
            [(100, 450, Some([0, 5]), 1), (450, 1100, Some([6, 11]), 1), (3000, 3500, Some([0, 6]), 2)]
        );
    }

    #[test]
    fn highlights_follow_their_word_through_edits() {
        assert_eq!(word_spans(" big  red dog"), [[1, 4], [6, 9], [10, 13]]);
        assert_eq!(rehighlight("big red dog", "large red dog", [4, 7]), Some([6, 9]));
        assert_eq!(rehighlight("big red dog", "big", [8, 11]), None);
    }

    #[test]
    #[ignore = "needs WHISPER_CLI and WHISPER_MODEL"]
    fn desktop_whisper_transcribes_the_jfk_sample_with_word_times() {
        let (cli, model) = (std::env::var("WHISPER_CLI").unwrap(), std::env::var("WHISPER_MODEL").unwrap());
        let sample = std::env::var("WHISPER_SAMPLE").unwrap_or("native/vendor/whisper.cpp-1.9.5/samples/jfk.wav".into());
        let dir = std::env::temp_dir().join(format!("luma-whisper-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("speech");
        let status = std::process::Command::new(cli)
            .args(["-m", &model, "-f", &sample, "-l", "en", "-t", "4", "-ojf", "-sns", "-np", "-of"])
            .arg(&out)
            .status()
            .unwrap();
        assert!(status.success());
        let transcript = parse(&std::fs::read(out.with_extension("json")).unwrap()).unwrap();
        let text: Vec<&str> = transcript.words.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(text.join(" "), "And so my fellow Americans, ask not what your country can do for you, ask what you can do for your country.");
        let ask: Vec<i64> = transcript.words.iter().filter(|w| w.text == "ask").map(|w| w.start).collect();
        assert!((ask[0] - 3280).abs() < 200 && (ask[1] - 8180).abs() < 200, "Speech onsets: {ask:?}");
        assert!(transcript.words.windows(2).all(|w| w[0].start <= w[1].start));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
