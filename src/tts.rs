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
        cb: Option<unsafe extern "C" fn(wav: *mut c_void, num: c_int, events: *mut c_void) -> c_int>,
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

// SAFETY: espeak-ng is not thread-safe, but we guarantee a single owning
// thread (the TTS worker). The type is never shared across threads.
unsafe impl Send for EspeakNg {}

impl EspeakNg {
    pub fn new(voice: &str) -> anyhow::Result<Self> {
        unsafe {
            let rate = espeak_Initialize(AUDIO_OUTPUT_SYNCHRONOUS, 0, std::ptr::null(), 0);
            if rate <= 0 {
                anyhow::bail!("espeak_Initialize failed ({rate}); is espeak-ng data installed?");
            }
            let mut eng = Self {
                sample_rate: rate as u32,
                initialized: true,
            };
            let cname = CString::new(voice).map_err(|_| anyhow::anyhow!("voice name contains NUL"))?;
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
            let len = ctext.as_bytes().len();
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// espeak-ng keeps global state; tests must not hit it concurrently.
    static ESPEAK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        ESPEAK_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn espeak_synthesizes_nonempty_pcm() {
        let _g = lock();
        let mut eng = EspeakNg::new("en").expect("espeak-ng available on this host");
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
        let slow = eng.synthesize("The quick brown fox jumps over the lazy dog.", 0.6).unwrap();
        let fast = eng.synthesize("The quick brown fox jumps over the lazy dog.", 2.0).unwrap();
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
        assert_eq!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]), 8000);
        // first sample = -1 → 0xFFFF
        assert_eq!(&wav[44..46], &[0xFF, 0xFF]);
        // byte rate = rate * channels * 2
        assert_eq!(u32::from_le_bytes([wav[28], wav[29], wav[30], wav[31]]), 16000);
    }
}
