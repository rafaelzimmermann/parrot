//! TTS engine: `TtsEngine` trait + an in-process espeak-ng backend via
//! hand-rolled FFI (no extra -sys crate, no subprocess).
//!
//! ABI notes (espeak-ng `speak_lib.h`, stable for years):
//! `espeak_Initialize` returns the sample rate (22050) or -1 on error.
//! With `AUDIO_OUTPUT_SYNCHRONOUS` the synth callback fires during the
//! `espeak_Synth` call itself, which lets us collect PCM on the calling thread.

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_uint, c_void, CString};

/// Raw mono 16-bit PCM produced by an engine.
#[derive(Debug, Clone)]
pub struct Pcm {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}

pub trait TtsEngine: Send {
    /// Synthesize `text` at `speed` (1.0 = default rate).
    fn synthesize(&mut self, text: &str, speed: f32) -> anyhow::Result<Pcm>;
}

// ---------------------------------------------------------------------------
// Piper (neural TTS) — official `piper` binary driven as a subprocess.
// Model pair (.onnx + .onnx.json) from rhasspy/piper-voices.
// Speed maps to `--length-scale 1/speed` (pitch preserved).
// ---------------------------------------------------------------------------

pub mod piper_engine {
    use super::{Pcm, TtsEngine};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    pub struct PiperEngine {
        bin: PathBuf,
        model: PathBuf,
        sample_rate: u32,
    }

    impl PiperEngine {
        /// `model_path`: `.onnx` weights or `.onnx.json` config (pair must co-exist).
        /// `bin`: path to the `piper` executable.
        pub fn new(model_path: &Path, bin: &Path) -> anyhow::Result<Self> {
            let (onnx, json) = locate_model_pair(model_path)?;
            let raw = std::fs::read_to_string(&json)?;
            let sample_rate = sample_rate_from_json(&raw)?;
            if !bin.is_file() {
                anyhow::bail!(
                    "piper executable not found: {} (run ./install.sh --voice en_US-lessac-medium)",
                    bin.display()
                );
            }
            Ok(Self {
                bin: bin.to_path_buf(),
                model: onnx,
                sample_rate,
            })
        }
    }

    impl TtsEngine for PiperEngine {
        fn synthesize(&mut self, text: &str, speed: f32) -> anyhow::Result<Pcm> {
            let length_scale = 1.0 / speed.clamp(0.25, 4.0);
            let mut child = Command::new(&self.bin)
                .arg("--model")
                .arg(&self.model)
                .arg("--length-scale")
                .arg(format!("{length_scale:.4}"))
                .arg("--output-raw")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| anyhow::anyhow!("spawn {}: {e}", self.bin.display()))?;
            {
                let mut stdin = child.stdin.take().expect("piped stdin");
                // write text; dropping stdin closes it → piper speaks and exits
                stdin.write_all(text.as_bytes())?;
            }
            let out = child.wait_with_output()?;
            if !out.status.success() {
                anyhow::bail!("piper exited with {}", out.status);
            }
            let bytes = out.stdout;
            if bytes.is_empty() {
                anyhow::bail!("piper produced no audio");
            }
            let samples: Vec<i16> = bytes
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            Ok(Pcm {
                samples,
                sample_rate: self.sample_rate,
            })
        }
    }

    /// Given a path to either file of the model pair, return (onnx, json).
    pub fn locate_model_pair(p: &Path) -> anyhow::Result<(PathBuf, PathBuf)> {
        let (onnx, json) = if p.extension().is_some_and(|e| e == "json") {
            (p.with_extension(""), p.to_path_buf()) // strips only last ext → .onnx
        } else {
            let mut json = p.as_os_str().to_owned();
            json.push(".json");
            (p.to_path_buf(), PathBuf::from(json))
        };
        for f in [&onnx, &json] {
            if !f.is_file() {
                anyhow::bail!("model file missing: {}", f.display());
            }
        }
        Ok((onnx, json))
    }

    /// Extract `audio.sample_rate` from a piper `.onnx.json` config.
    pub fn sample_rate_from_json(raw: &str) -> anyhow::Result<u32> {
        let v: serde_json::Value = serde_json::from_str(raw)?;
        v.pointer("/audio/sample_rate")
            .and_then(|s| s.as_u64())
            .and_then(|s| u32::try_from(s).ok())
            .filter(|s| (8000..=192000).contains(s))
            .ok_or_else(|| anyhow::anyhow!("missing audio.sample_rate in piper config"))
    }
}
pub use piper_engine::PiperEngine;

// ---------------------------------------------------------------------------
// WAV container writer (PCM16 mono)
// ---------------------------------------------------------------------------

pub fn pcm_to_wav(pcm: &Pcm) -> Vec<u8> {
    let data_len = pcm.samples.len() * 2;
    let mut w = Vec::with_capacity(44 + data_len);
    let push_u32 = |w: &mut Vec<u8>, v: u32| w.extend_from_slice(&v.to_le_bytes());
    let push_u16 = |w: &mut Vec<u8>, v: u16| w.extend_from_slice(&v.to_le_bytes());

    w.extend_from_slice(b"RIFF");
    push_u32(&mut w, (36 + data_len) as u32);
    w.extend_from_slice(b"WAVE");
    w.extend_from_slice(b"fmt ");
    push_u32(&mut w, 16); // fmt chunk size
    push_u16(&mut w, 1); // PCM
    push_u16(&mut w, 1); // mono
    push_u32(&mut w, pcm.sample_rate);
    push_u32(&mut w, pcm.sample_rate * 2); // byte rate = rate * 2 bytes
    push_u16(&mut w, 2); // block align
    push_u16(&mut w, 16); // bits per sample
    w.extend_from_slice(b"data");
    push_u32(&mut w, data_len as u32);
    for s in &pcm.samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

// ---------------------------------------------------------------------------
// espeak-ng FFI
// ---------------------------------------------------------------------------

// espeak_AUDIO_OUTPUT — plain C enum, values from speak_lib.h:
//   PLAYBACK=0, RETRIEVAL=1, SYNCHRONOUS=2, SYNC_PLAYBACK=3
const AUDIO_OUTPUT_SYNCHRONOUS: c_int = 2;
// espeak_PARAMETER: SILENCE=0, RATE=1, VOLUME=2, PITCH=3, RANGE=4, ...
const PARAM_RATE: c_int = 1;
const PARAM_VOLUME: c_int = 2;
// character encoding flags (#defines)
const CHARS_UTF8: c_uint = 1;
const ENDPAUSE: c_uint = 0x1000;
// espeak_POSITION_TYPE: POS_CHARACTER=1, POS_WORD=2, POS_SENTENCE=3
const POS_CHARACTER: c_int = 1;
// errors: EE_OK=0, EE_INTERNAL_ERROR=-1, EE_BUFFER_FULL=1, EE_NOT_FOUND=2
const EE_OK: c_int = 0;

const BASE_WPM: f32 = 175.0; // espeakRATE_NORMAL
const MIN_WPM: i32 = 80; // espeakRATE_MINIMUM
const MAX_WPM: i32 = 400; // below espeakRATE_MAXIMUM (450) on purpose

#[link(name = "espeak-ng")]
extern "C" {
    fn espeak_Initialize(
        output: c_int,
        buflength: c_int,
        path: *const c_char,
        options: c_int,
    ) -> c_int;
    fn espeak_SetVoiceByName(name: *const c_char) -> c_int;
    fn espeak_SetParameter(parameter: c_int, value: c_int, relative: c_int) -> c_int;
    fn espeak_SetSynthCallback(
        cb: Option<
            unsafe extern "C" fn(wav: *mut c_void, num: c_int, events: *mut c_void) -> c_int,
        >,
    );
    fn espeak_Synth(
        text: *const c_void,
        size: usize,
        position: c_uint,
        position_type: c_int,
        end_position: c_uint,
        flags: c_uint,
        unique_identifier: *mut c_uint,
        user_data: *mut c_void,
    ) -> c_int;
    fn espeak_Cancel() -> c_int;
    fn espeak_Terminate() -> c_int;
}

thread_local! {
    static PCM_SINK: RefCell<Vec<i16>> = const { RefCell::new(Vec::new()) };
}

unsafe extern "C" fn synth_callback(wav: *mut c_void, num: c_int, _events: *mut c_void) -> c_int {
    if !wav.is_null() && num > 0 {
        let samples = std::slice::from_raw_parts(wav as *const i16, num as usize);
        PCM_SINK.with(|s| s.borrow_mut().extend_from_slice(samples));
    }
    0 // NOTE: 0 = continue synthesis (non-zero aborts) — verified against
      // libespeak-ng 1.52 empirically; 17 callbacks vs 1 for a test sentence.
}

pub struct EspeakNg {
    sample_rate: u32,
    initialized: bool,
}

static ESPEAK_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl EspeakNg {
    pub fn new(voice: &str) -> anyhow::Result<Self> {
        let cname = CString::new(voice).map_err(|_| anyhow::anyhow!("voice name contains NUL"))?;
        if ESPEAK_ACTIVE
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            anyhow::bail!("an espeak-ng engine is already active");
        }
        unsafe {
            let rate = espeak_Initialize(AUDIO_OUTPUT_SYNCHRONOUS, 0, std::ptr::null(), 0);
            if rate <= 0 {
                ESPEAK_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
                anyhow::bail!("espeak_Initialize failed ({rate}); is espeak-ng data installed?");
            }
            let mut eng = Self {
                sample_rate: rate as u32,
                initialized: true,
            };
            if espeak_SetVoiceByName(cname.as_ptr()) != EE_OK {
                anyhow::bail!("voice not found: {voice}");
            }
            eng.set_volume(95);
            Ok(eng)
        }
    }

    fn set_volume(&mut self, vol: i32) {
        unsafe { espeak_SetParameter(PARAM_VOLUME, vol, 0) };
    }

    /// speed (0.5..=2.0+) → espeak words-per-minute (pitch-preserving).
    fn wpm_for(speed: f32) -> i32 {
        ((BASE_WPM * speed.clamp(0.25, 4.0)).round() as i32).clamp(MIN_WPM, MAX_WPM)
    }
}

impl TtsEngine for EspeakNg {
    fn synthesize(&mut self, text: &str, speed: f32) -> anyhow::Result<Pcm> {
        if text.trim().is_empty() {
            return Ok(Pcm {
                samples: Vec::new(),
                sample_rate: self.sample_rate,
            });
        }
        let ctext = CString::new(text).map_err(|_| anyhow::anyhow!("text contains NUL byte"))?;
        unsafe {
            espeak_SetParameter(PARAM_RATE, Self::wpm_for(speed), 0);
            PCM_SINK.with(|s| s.borrow_mut().clear());
            espeak_SetSynthCallback(Some(synth_callback));
            let len = ctext.as_bytes_with_nul().len();
            let rc = espeak_Synth(
                ctext.as_ptr() as *const c_void,
                len, // include NUL terminator per API docs
                0,
                POS_CHARACTER,
                0,
                CHARS_UTF8 | ENDPAUSE,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            if rc != EE_OK {
                return Err(anyhow::anyhow!("espeak_Synth failed ({rc})"));
            }
            let samples = PCM_SINK.with(|s| std::mem::take(&mut *s.borrow_mut()));
            Ok(Pcm {
                samples,
                sample_rate: self.sample_rate,
            })
        }
    }
}

impl Drop for EspeakNg {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                espeak_SetSynthCallback(None);
                espeak_Cancel();
                espeak_Terminate();
            }
            ESPEAK_ACTIVE.store(false, std::sync::atomic::Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piper_sample_rate_from_json_parses() {
        let cfg = r#"{"audio":{"sample_rate":22050},"espeak":{"voice":"en-us"}}"#;
        assert_eq!(piper_engine::sample_rate_from_json(cfg).unwrap(), 22050);
    }

    #[test]
    fn piper_sample_rate_from_json_missing_fails() {
        assert!(piper_engine::sample_rate_from_json(r#"{"audio":{"sample_rate":0}}"#).is_err());
        assert!(
            piper_engine::sample_rate_from_json(r#"{"audio":{"sample_rate":4294989346}}"#).is_err()
        );
        assert!(piper_engine::sample_rate_from_json("{}").is_err());
        assert!(piper_engine::sample_rate_from_json("not json").is_err());
    }

    #[test]
    fn piper_locate_model_pair_from_both_sides() {
        let dir = std::env::temp_dir().join("parrot-pair-test");
        std::fs::create_dir_all(&dir).unwrap();
        let onnx = dir.join("v.onnx");
        let json = dir.join("v.onnx.json");
        std::fs::write(&onnx, b"").unwrap();
        std::fs::write(&json, b"").unwrap();
        let (a, b) = piper_engine::locate_model_pair(&json).unwrap();
        assert_eq!(a, onnx);
        assert_eq!(b, json);
        let (a2, b2) = piper_engine::locate_model_pair(&onnx).unwrap();
        assert_eq!((a2, b2), (onnx.clone(), json.clone()));
    }

    #[test]
    fn piper_locate_model_pair_missing_side_fails() {
        let dir = std::env::temp_dir().join("parrot-pair-missing");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("only.onnx"), b"").unwrap();
        assert!(piper_engine::locate_model_pair(&dir.join("only.onnx")).is_err());
    }

    /// espeak-ng keeps global state; tests must not hit it concurrently.
    static ESPEAK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ESPEAK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn espeak_synthesizes_nonempty_pcm() {
        let _g = lock();
        let mut eng = EspeakNg::new("en").expect("espeak-ng available on this host");
        assert!(
            EspeakNg::new("en").is_err(),
            "global FFI state must have one owner"
        );
        let pcm = eng.synthesize("Hello world.", 1.0).expect("synth ok");
        assert_eq!(pcm.sample_rate, 22050);
        assert!(!pcm.samples.is_empty(), "expected non-empty PCM");
        // ~0.6–1.5s of audio at 22050 Hz
        let secs = pcm.samples.len() as f32 / pcm.sample_rate as f32;
        assert!((0.2..3.0).contains(&secs), "suspicious duration: {secs}s");
    }

    #[test]
    fn wav_header_is_wellformed() {
        let pcm = Pcm {
            samples: vec![0i16; 2205],
            sample_rate: 22050,
        };
        let wav = pcm_to_wav(&pcm);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(wav.len(), 44 + 4410);
    }
    #[test]
    fn unknown_voice_fails_cleanly() {
        let _g = lock();
        assert!(EspeakNg::new("definitely-not-a-voice-xyz").is_err());
    }

    #[test]
    fn speed_maps_to_wpm_sensibly() {
        assert_eq!(EspeakNg::wpm_for(1.0), 175);
        assert_eq!(EspeakNg::wpm_for(2.0), 350);
        assert_eq!(EspeakNg::wpm_for(0.1), 80); // clamped low
        assert_eq!(EspeakNg::wpm_for(10.0), 400); // clamped high
    }

    #[test]
    fn empty_text_yields_empty_pcm() {
        let _g = lock();
        let mut eng = EspeakNg::new("en").expect("espeak-ng available on this host");
        let pcm = eng.synthesize("   ", 1.0).unwrap();
        assert!(pcm.samples.is_empty());
    }

    #[test]
    fn speed_changes_output_length() {
        let _g = lock();
        // Faster speech should produce fewer samples for the same sentence
        // (pitch-preserving wpm change, not tape speed).
        let mut eng = EspeakNg::new("en").expect("espeak-ng available");
        let slow = eng
            .synthesize("The quick brown fox jumps over the lazy dog.", 0.6)
            .unwrap();
        let fast = eng
            .synthesize("The quick brown fox jumps over the lazy dog.", 2.0)
            .unwrap();
        assert!(
            fast.samples.len() < slow.samples.len(),
            "fast={} slow={}",
            fast.samples.len(),
            slow.samples.len()
        );
    }

    #[test]
    fn wav_encodes_little_endian_samples() {
        let pcm = Pcm {
            samples: vec![-1i16, 0, 1],
            sample_rate: 8000,
        };
        let wav = pcm_to_wav(&pcm);
        // mono @ 8000 Hz
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            8000
        );
        // first sample = -1 → 0xFFFF
        assert_eq!(&wav[44..46], &[0xFF, 0xFF]);
        // byte rate = rate * channels * 2
        assert_eq!(
            u32::from_le_bytes([wav[28], wav[29], wav[30], wav[31]]),
            16000
        );
    }
}
