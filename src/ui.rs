//! The floating control overlay (eframe/egui).
//!
//! Borderless, translucent, always-on-top window matching docs/mockups.md.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{CentralPanel, Color32, RichText};

use crate::audio::AudioHandle;
use crate::textutil::elide;
use crate::EngineEvent;

const BG: Color32 = Color32::from_rgba_unmultiplied_const(16, 20, 24, 208);
const ACCENT: Color32 = Color32::from_rgb(86, 182, 194);
const TEXT: Color32 = Color32::from_rgb(230, 225, 207);
const DIM: Color32 = Color32::from_rgb(138, 133, 120);
const ERR: Color32 = Color32::from_rgb(224, 108, 117);
const BTN_BG: Color32 = Color32::from_rgba_unmultiplied_const(46, 54, 62, 255);

/// State shared between the UI thread and the TTS worker.
pub struct Shared {
    /// Current speed × 1000 (atomic to avoid lock contention).
    pub speed_milli: std::sync::atomic::AtomicU32,
    /// When set, the worker clears playback and restarts from this sentence.
    pub restart_from: Mutex<Option<usize>>,
}

pub struct SpeakApp {
    rx: std::sync::mpsc::Receiver<EngineEvent>,
    audio: Option<AudioHandle>,
    shared: Arc<Shared>,

    sentences: Vec<String>,
    char_prefix: Vec<f32>, // cumulative char fraction before sentence i

    cur: usize,
    total: usize,
    cur_text: String,
    done_queued: bool,
    finished_at: Option<Instant>,
    error_at: Option<Instant>,
    error: Option<String>,
    note: String,
    paused: bool,
    speed: f32,
}

impl SpeakApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        rx: std::sync::mpsc::Receiver<EngineEvent>,
        audio: Option<AudioHandle>,
        shared: Arc<Shared>,
        sentences: Vec<String>,
        worker_running: bool,
        speed: f32,
    ) -> Self {
        let total = sentences.len();
        let total_chars: usize = sentences.iter().map(|s| s.chars().count()).sum();
        let mut char_prefix = Vec::with_capacity(total + 1);
        let mut acc = 0.0f32;
        for s in &sentences {
            char_prefix.push(acc);
            acc += s.chars().count() as f32 / total_chars.max(1) as f32;
        }
        char_prefix.push(1.0);
        Self {
            rx,
            audio,
            shared,
            sentences,
            char_prefix,
            cur: 0,
            total,
            cur_text: String::new(),
            done_queued: !worker_running,
            finished_at: None,
            error_at: None,
            error: None,
            note: String::new(),
            paused: false,
            speed,
        }
    }

    fn close(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        loop {
            match self.rx.try_recv() {
                Ok(EngineEvent::SentenceStarted { idx, total, text }) => {
                    self.cur = idx;
                    self.total = total;
                    self.cur_text = text;
                    ctx.request_repaint();
                }
                Ok(EngineEvent::AllQueued) => {
                    self.done_queued = true;
                    ctx.request_repaint();
                }
                Ok(EngineEvent::SynthError(e)) => {
                    self.note = format!("skipped sentence: {e}");
                    ctx.request_repaint();
                }
                Ok(EngineEvent::Fatal(e)) => {
                    self.error = Some(e);
                    self.done_queued = true;
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.done_queued = true;
                    break;
                }
            }
        }
    }

    fn playback_finished(&self) -> bool {
        self.done_queued
            && !self.paused
            && self.error.is_none()
            && self.audio.as_ref().is_none_or(|a| a.is_empty())
    }

    fn toggle_pause(&mut self) {
        self.paused = !self.paused;
        if let Some(a) = &self.audio {
            if self.paused {
                a.pause();
            } else {
                a.resume();
            }
        }
    }
}

impl eframe::App for SpeakApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx();
        self.poll_events(ctx);

        // ---- lifecycle --------------------------------------------------
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close(ctx);
        }

        if self.error.is_some() {
            self.error_at.get_or_insert_with(Instant::now);
        }

        if self.error.is_none() {
            if self.playback_finished() {
                if self.finished_at.is_none() {
                    self.finished_at = Some(Instant::now());
                    ctx.request_repaint();
                } else if self
                    .finished_at
                    .is_some_and(|t| t.elapsed() > Duration::from_millis(400))
                {
                    self.close(ctx);
                }
            } else {
                self.finished_at = None;
            }
        } else if self
            .error_at
            .is_some_and(|t| t.elapsed() > Duration::from_secs(6))
        {
            self.close(ctx);
        }

        ctx.request_repaint_after(Duration::from_millis(120));

        // ---- draw -------------------------------------------------------
        CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .corner_radius(egui::CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ui, |ui| {
                if let Some(err) = self.error.clone() {
                    error_panel(ui, &err, self.error_at);
                    return;
                }

                // title row
                ui.horizontal(|ui| {
                    ui.label(RichText::new("●").color(ACCENT).size(11.0));
                    ui.label(RichText::new("parrot").color(DIM).size(12.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("✕").color(DIM).size(13.0))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            self.close(ui.ctx());
                        }
                    });
                });

                ui.add_space(6.0);

                // current sentence
                let sentence = if self.cur_text.is_empty() {
                    if self.sentences.is_empty() {
                        "(empty)".into()
                    } else {
                        self.sentences[0].clone()
                    }
                } else {
                    self.cur_text.clone()
                };
                ui.add(
                    egui::Label::new(RichText::new(elide(&sentence, 110)).color(TEXT).size(14.0))
                        .wrap_mode(egui::TextWrapMode::Wrap),
                );

                ui.add_space(6.0);

                // progress
                let frac = if self.playback_finished() || self.finished_at.is_some() {
                    1.0
                } else {
                    self.char_prefix[self.cur.min(self.char_prefix.len() - 1)]
                };
                let status = if self.paused {
                    format!("Paused — {} / {}", self.cur + 1, self.total)
                } else if self.finished_at.is_some() {
                    "Done ✓".to_string()
                } else if !self.note.is_empty() {
                    self.note.clone()
                } else {
                    format!("{} / {}", self.cur + 1, self.total)
                };
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_height(8.0)
                        .fill(ACCENT)
                        .text(status),
                );

                ui.add_space(10.0);

                // controls
                ui.horizontal(|ui| {
                    let label = if self.paused {
                        "▶  Play"
                    } else {
                        "⏸  Pause"
                    };
                    let enabled = !(self.finished_at.is_some()
                        || (self.done_queued && self.audio.as_ref().is_none_or(|a| a.is_empty())));
                    let btn = egui::Button::new(RichText::new(label).color(TEXT).size(14.0))
                        .fill(BTN_BG)
                        .corner_radius(egui::CornerRadius::same(8))
                        .min_size(egui::vec2(104.0, 32.0));
                    let resp = ui.add_enabled(enabled, btn);
                    if resp.clicked() {
                        self.toggle_pause();
                    }

                    ui.add_space(14.0);

                    ui.label(RichText::new("Velocity").color(DIM).size(13.0));
                    let slider = egui::Slider::new(&mut self.speed, 0.5..=2.0)
                        .step_by(0.05)
                        .show_value(false);
                    let resp = ui.add(slider);
                    if resp.changed() {
                        let milli = (self.speed * 1000.0).round() as u32;
                        self.shared.speed_milli.store(milli, Ordering::Relaxed);
                    }
                    if resp.drag_stopped() || resp.lost_focus() {
                        // Re-speak the current sentence at the new rate.
                        *self.shared.restart_from.lock().unwrap() =
                            Some(self.cur.min(self.total - 1));
                    }
                    ui.label(
                        RichText::new(format!("{:.2}×", self.speed))
                            .color(ACCENT)
                            .size(13.0)
                            .strong(),
                    );
                });
            });
    }
}

fn error_panel(ui: &mut egui::Ui, err: &str, error_at: Option<Instant>) {
    ui.vertical_centered(|ui| {
        ui.add_space(10.0);
        ui.label(
            RichText::new("⚠ parrot error")
                .color(ERR)
                .size(15.0)
                .strong(),
        );
        ui.add_space(8.0);
        ui.label(RichText::new(err).color(TEXT).size(13.0));
        ui.add_space(4.0);
        if let Some(t) = error_at {
            let left = 6u64.saturating_sub(t.elapsed().as_secs());
            ui.label(
                RichText::new(format!("closing in {left}s…"))
                    .color(DIM)
                    .size(11.0),
            );
        }
        ui.add_space(6.0);
        if ui
            .add(
                egui::Button::new(RichText::new("Close").color(TEXT))
                    .fill(BTN_BG)
                    .min_size(egui::vec2(90.0, 28.0)),
            )
            .clicked()
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}
