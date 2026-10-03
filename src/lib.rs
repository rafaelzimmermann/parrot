pub mod audio;
pub mod cli;
pub mod selection;
pub mod textutil;
pub mod tts;
pub mod ui;

/// Events sent from the TTS worker thread to the UI.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// Started synthesizing/playing sentence `idx` of `total`.
    SentenceStarted {
        idx: usize,
        total: usize,
        text: String,
    },
    /// Worker finished enqueueing every sentence.
    AllQueued,
    /// Non-fatal synthesis failure (skips sentence).
    SynthError(String),
    /// Fatal: engine unusable.
    Fatal(String),
}
