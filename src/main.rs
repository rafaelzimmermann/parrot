//! hypr-speak — Speak Selection overlay for Hyprland/Wayland.
//!
//! Lifecycle: read primary selection → clean/split → TTS worker thread →
//! rodio playback → egui overlay → auto-close on completion.


use std::process::ExitCode;
use std::sync::atomic::AtomicU32;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use hypr_speak::audio::{append_pcm, queued, AudioHandle, SharedPlayer};
use hypr_speak::cli::{self, CliError, USAGE};
use hypr_speak::tts::{pcm_to_wav, EspeakNg, Pcm, TtsEngine};
use hypr_speak::ui::{Shared, SpeakApp};
use hypr_speak::{selection, textutil, EngineEvent};

fn main() -> ExitCode {
    let opts = match cli::parse_from(std::env::args().skip(1)) {
        Ok(o) => o,
        Err(CliError::Help) => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(CliError::Msg(e)) => {
            eprintln!("hypr-speak: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut builder = env_logger::Builder::new();
    // Only our crate's logs — a plain "debug" filter lets winit spam thousands of lines.
    builder.filter_module("hypr_speak", if opts.verbose { log::LevelFilter::Debug } else { log::LevelFilter::Warn });
    builder.filter_level(log::LevelFilter::Warn);
    builder.parse_default_env();
    builder.init();

    // ---- 1. acquire text --------------------------------------------------
    let raw = match &opts.text {
        Some(t) => t.clone(),
        None => match selection::get_selection_text() {
            Ok(Some(t)) => t,
            Ok(None) => {
                log::debug!("no selection text — exiting");
                return ExitCode::SUCCESS; // spec: no text → exit immediately
            }
            Err(e) => {
                // e.g. no wayland session; stay quiet like a good hotkey citizen
                log::warn!("selection read failed: {e}");
                return ExitCode::SUCCESS;
            }
        },
    };

    let cleaned = textutil::clean(&raw);
    if cleaned.is_empty() {
        log::debug!("selection was only whitespace — exiting");
        return ExitCode::SUCCESS;
    }
    let sentences = textutil::split_sentences(&cleaned);
    let total = sentences.len();
    log::debug!("speaking {total} sentence(s), {} chars", cleaned.chars().count());

    // ---- 2. headless mode ---------------------------------------------------
    if let Some(path) = &opts.wav {
        return match synth_to_wav(&cleaned, &sentences, &opts.voice, opts.speed, path) {
            Ok(info) => {
                println!("wrote {} ({info})", path.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("hypr-speak: {e}");
                ExitCode::FAILURE
            }
        };
    }

    // ---- 3. GUI mode ----------------------------------------------------
    let audio = match AudioHandle::new() {
        Ok(a) => Some(a),
        Err(e) => {
            log::error!("audio init failed: {e}");
            None // open an error overlay instead of crashing
        }
    };

    let shared = Arc::new(Shared {
        speed_milli: AtomicU32::new((opts.speed * 1000.0).round() as u32),
        restart_from: Mutex::new(None),
    });

    let (tx, rx) = mpsc::channel::<EngineEvent>();
    let worker_running = audio.is_some();
    if let Some(audio) = &audio {
        spawn_worker(sentences.clone(), opts.voice.clone(), audio.player(), shared.clone(), tx);
    } else {
        let _ = tx.send(EngineEvent::Fatal("no usable audio device".into()));
    }

    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("hypr-speak")
            .with_app_id("hypr-speak")
            .with_inner_size([440.0, 170.0])
            .with_min_inner_size([330.0, 150.0])
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_active(true),
        ..Default::default()
    };

    let app = move |cc: &eframe::CreationContext<'_>| {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        Ok(Box::new(SpeakApp::new(
            cc,
            rx,
            audio,
            shared,
            sentences,
            worker_running,
            opts.speed.clamp(0.5, 2.0),
        )) as Box<dyn eframe::App>)
    };

    match eframe::run_native("hypr-speak", native, Box::new(app)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("hypr-speak: {e}");
            ExitCode::FAILURE
        }
    }
}

/// TTS worker: owns the espeak-ng engine (single thread), synthesizes sentence
/// by sentence, appends PCM to the shared sink, honors speed restarts.
fn spawn_worker(
    sentences: Vec<String>,
    voice: String,
    player: SharedPlayer,
    shared: Arc<Shared>,
    tx: mpsc::Sender<EngineEvent>,
) {
    std::thread::spawn(move || {
        let mut engine = match EspeakNg::new(&voice) {
            Ok(e) => e,
            Err(e) => {
                let _ = tx.send(EngineEvent::Fatal(format!("TTS engine init: {e}")));
                return;
            }
        };
        let total = sentences.len();
        let mut i = 0usize;
        loop {
            if i >= total {
                break;
            }
            // honor restart requests (speed change)
            if let Some(r) = shared.restart_from.lock().unwrap().take() {
                player.lock().unwrap().clear();
                i = r.min(total.saturating_sub(1));
            }
            let speed = shared.speed_milli.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0;
            if tx
                .send(EngineEvent::SentenceStarted { idx: i, total, text: sentences[i].clone() })
                .is_err()
            {
                return; // UI gone
            }
            match engine.synthesize(&sentences[i], speed) {
                Ok(pcm) => append_pcm(&player, &pcm),
                Err(e) => {
                    if tx.send(EngineEvent::SynthError(e.to_string())).is_err() {
                        return;
                    }
                }
            }
            i += 1;
            // prefetch pacing: at most 2 sentences queued, watch for restarts
            while i < total && queued(&player) >= 2 {
                std::thread::sleep(Duration::from_millis(40));
                if shared.restart_from.lock().unwrap().is_some() {
                    break;
                }
            }
        }
        let _ = tx.send(EngineEvent::AllQueued);
    });
}

fn synth_to_wav(
    cleaned: &str,
    sentences: &[String],
    voice: &str,
    speed: f32,
    path: &std::path::Path,
) -> anyhow::Result<String> {
    let mut engine = EspeakNg::new(voice)?;
    let mut samples = Vec::new();
    let mut rate = 22050;
    for s in sentences {
        let pcm = engine.synthesize(s, speed)?;
        rate = pcm.sample_rate;
        samples.extend(pcm.samples);
    }
    let secs = samples.len() as f64 / rate as f64;
    let wav = pcm_to_wav(&Pcm { samples, sample_rate: rate });
    std::fs::write(path, &wav)?;
    Ok(format!(
        "{} sentences, {} bytes, ~{secs:.1}s of '{}…'",
        sentences.len(),
        wav.len(),
        cleaned.chars().take(40).collect::<String>()
    ))
}
