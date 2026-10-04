//! CLI argument parsing — pure and testable (takes an iterator, touches no
//! globals).

use std::path::PathBuf;

pub const USAGE: &str = "\
parrot — Speak Selection for Hyprland/Wayland

USAGE:
    parrot [OPTIONS] [TXT]

INPUT:
    Quoted TXT takes precedence over piped/redirected stdin.
    Without either, read the selection (then clipboard).
    Use -- before text beginning with a dash.

OPTIONS:
    --voice <NAME>  espeak-ng voice, e.g. `en`, `de`, `en+whisper`   [default: en]
    --speed <F>     Initial velocity 0.5–2.0                        [default: 1.0]
    --engine <E>    auto | espeak | piper                          [default: auto]
    --model <PATH>  piper voice model (.onnx or .onnx.json)
    --wav <FILE>    Synthesize to a .wav file and exit (headless test)
    --verbose       Log to stderr
    -h, --help      This help

Bind example (hyprland.conf):
    bind = ALT, Escape, exec, parrot
";

#[derive(Debug, Clone, PartialEq)]
pub struct Opts {
    pub text: Option<String>,
    pub voice: String,
    pub speed: f32,
    pub engine: String,
    pub model: Option<String>,
    pub wav: Option<PathBuf>,
    pub verbose: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self {
            text: None,
            voice: "en".into(),
            speed: 1.0,
            engine: "auto".into(),
            model: None,
            wav: None,
            verbose: false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum CliError {
    /// `-h` / `--help` was requested.
    Help,
    /// Bad usage; the string is the human message.
    Msg(String),
}

/// Parse arguments from `it` (skip the program name before calling).
pub fn parse_from<I>(it: I) -> Result<Opts, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut o = Opts::default();
    let mut it = it.into_iter();
    let mut positional_only = false;
    while let Some(a) = it.next() {
        if positional_only || !a.starts_with('-') {
            if o.text.replace(a).is_some() {
                return Err(CliError::Msg(
                    "expected one TXT argument; quote multiword text".into(),
                ));
            }
            continue;
        }
        match a.as_str() {
            "--" => positional_only = true,
            "--voice" => o.voice = it.next().ok_or_missing("--voice")?,
            "--engine" => {
                let v = it.next().ok_or_missing("--engine")?;
                if !["auto", "espeak", "piper"].contains(&v.as_str()) {
                    return Err(CliError::Msg(format!(
                        "bad --engine: {v} (auto|espeak|piper)"
                    )));
                }
                o.engine = v;
            }
            "--model" => o.model = Some(it.next().ok_or_missing("--model")?),
            "--speed" => {
                let v = it.next().ok_or_missing("--speed")?;
                o.speed = v
                    .parse()
                    .map_err(|_| CliError::Msg(format!("bad --speed: {v}")))?;
            }
            "--wav" => {
                let v = it.next().ok_or_missing("--wav")?;
                o.wav = Some(PathBuf::from(v));
            }
            "--verbose" | "-v" => o.verbose = true,
            "-h" | "--help" => return Err(CliError::Help),
            other => {
                return Err(CliError::Msg(format!(
                    "unknown argument: {other} (see --help)"
                )))
            }
        }
    }
    if !(0.25..=4.0).contains(&o.speed) {
        return Err(CliError::Msg(format!(
            "--speed out of range (0.25–4.0): {}",
            o.speed
        )));
    }
    Ok(o)
}

trait NextOrMissing {
    fn ok_or_missing(self, flag: &str) -> Result<String, CliError>;
}
impl NextOrMissing for Option<String> {
    fn ok_or_missing(self, flag: &str) -> Result<String, CliError> {
        self.ok_or_else(|| CliError::Msg(format!("{flag} needs a value")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn engine_and_model_flags_parse() {
        let o = parse_from(args(&[
            "--engine",
            "piper",
            "--model",
            "/v/en_US-lessac-medium.onnx",
        ]))
        .unwrap();
        assert_eq!(o.engine, "piper");
        assert_eq!(o.model.as_deref(), Some("/v/en_US-lessac-medium.onnx"));
    }

    #[test]
    fn bad_engine_rejected() {
        assert!(
            matches!(parse_from(args(&["--engine", "sam"])), Err(CliError::Msg(m)) if m.contains("bad --engine"))
        );
    }

    #[test]
    fn defaults() {
        let o = parse_from(args(&[])).unwrap();
        assert_eq!(o, Opts::default());
        assert_eq!(o.voice, "en");
        assert_eq!(o.speed, 1.0);
        assert!(o.wav.is_none() && !o.verbose && o.text.is_none());
    }

    #[test]
    fn all_flags() {
        let o = parse_from(args(&[
            "hi there",
            "--voice",
            "de",
            "--speed",
            "1.5",
            "--wav",
            "/tmp/a.wav",
            "-v",
        ]))
        .unwrap();
        assert_eq!(o.text.as_deref(), Some("hi there"));
        assert_eq!(o.voice, "de");
        assert_eq!(o.speed, 1.5);
        assert_eq!(o.wav, Some(PathBuf::from("/tmp/a.wav")));
        assert!(o.verbose);
    }

    #[test]
    fn help_requested() {
        assert_eq!(parse_from(args(&["--help"])), Err(CliError::Help));
        assert_eq!(parse_from(args(&["-h"])), Err(CliError::Help));
    }

    #[test]
    fn missing_values() {
        assert!(
            matches!(parse_from(args(&["--voice"])), Err(CliError::Msg(m)) if m.contains("needs a value"))
        );
        assert!(
            matches!(parse_from(args(&["--speed"])), Err(CliError::Msg(m)) if m.contains("needs a value"))
        );
    }

    #[test]
    fn bad_speed() {
        assert!(
            matches!(parse_from(args(&["--speed", "fast"])), Err(CliError::Msg(m)) if m.contains("bad --speed"))
        );
        assert!(
            matches!(parse_from(args(&["--speed", "9.9"])), Err(CliError::Msg(m)) if m.contains("out of range"))
        );
    }

    #[test]
    fn unknown_argument() {
        assert!(
            matches!(parse_from(args(&["--frobnicate"])), Err(CliError::Msg(m)) if m.contains("unknown argument"))
        );
    }

    #[test]
    fn positional_text_and_separator() {
        assert_eq!(
            parse_from(args(&["--voice", "en", "hello world"]))
                .unwrap()
                .text
                .as_deref(),
            Some("hello world")
        );
        assert_eq!(
            parse_from(args(&["--", "--help"])).unwrap().text.as_deref(),
            Some("--help")
        );
        assert_eq!(parse_from(args(&[""])).unwrap().text.as_deref(), Some(""));
        assert!(parse_from(args(&["hello", "world"])).is_err());
        assert!(parse_from(args(&["--text", "hello"])).is_err());
    }
}
