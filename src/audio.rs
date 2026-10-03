//! Audio output: thin wrapper over rodio 0.22's `MixerDeviceSink` + `Player`.
//!
//! rodio 0.22 replaced `OutputStream`/`Sink` with `MixerDeviceSink` (the OS
//! stream, must be kept alive) and `Player` (sequential playback controls).
//! `Player` is `Send + Sync` but not `Clone`, so the UI thread and the TTS
//! worker share it as `Arc<Mutex<Player>>` (contention is trivial).

use std::sync::{Arc, Mutex};

use crate::tts::Pcm;

pub type SharedPlayer = Arc<Mutex<rodio::Player>>;

pub struct AudioHandle {
    player: SharedPlayer,
    /// Kept alive: dropping the sink disposes the OS audio stream.
    _sink: rodio::MixerDeviceSink,
}

impl AudioHandle {
    /// Open the default audio output device.
    pub fn new() -> anyhow::Result<Self> {
        let mut sink = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|e| anyhow::anyhow!("opening default audio output: {e}"))?;
        sink.log_on_drop(false);
        let player = rodio::Player::connect_new(sink.mixer());
        Ok(Self {
            player: Arc::new(Mutex::new(player)),
            _sink: sink,
        })
    }

    /// A clonable handle for the worker thread to enqueue audio on.
    pub fn player(&self) -> SharedPlayer {
        self.player.clone()
    }

    pub fn pause(&self) {
        self.player.lock().unwrap().pause();
    }

    pub fn resume(&self) {
        self.player.lock().unwrap().play();
    }

    pub fn is_paused(&self) -> bool {
        self.player.lock().unwrap().is_paused()
    }

    pub fn clear(&self) {
        self.player.lock().unwrap().clear();
    }

    /// True when nothing is queued and nothing is playing.
    pub fn is_empty(&self) -> bool {
        self.player.lock().unwrap().empty()
    }
}

/// Append raw PCM to the shared player as a playable source.
pub fn append_pcm(player: &SharedPlayer, pcm: &Pcm) {
    if pcm.samples.is_empty() {
        return;
    }
    let channels = std::num::NonZeroU16::new(1).expect("nonzero channels");
    let rate = std::num::NonZeroU32::new(pcm.sample_rate).expect("nonzero sample rate");
    let data: Vec<f32> = pcm.samples.iter().map(|&s| s as f32 / 32768.0).collect();
    let src = rodio::buffer::SamplesBuffer::new(channels, rate, data);
    player.lock().unwrap().append(src);
}

/// Number of sources currently queued on the player (for prefetch pacing).
pub fn queued(player: &SharedPlayer) -> usize {
    player.lock().unwrap().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unconnected `Player` (no audio device needed) is enough to verify
    /// the queue bookkeeping used for prefetch pacing.
    fn unconnected_player() -> SharedPlayer {
        let (player, _output) = rodio::Player::new();
        std::mem::forget(_output); // keep the pump side alive for the test
        Arc::new(Mutex::new(player))
    }

    #[test]
    fn append_pcm_empty_is_noop() {
        let p = unconnected_player();
        append_pcm(
            &p,
            &Pcm {
                samples: vec![],
                sample_rate: 22050,
            },
        );
        assert_eq!(queued(&p), 0);
    }

    #[test]
    fn append_pcm_enqueues() {
        let p = unconnected_player();
        append_pcm(
            &p,
            &Pcm {
                samples: vec![0i16; 64],
                sample_rate: 22050,
            },
        );
        append_pcm(
            &p,
            &Pcm {
                samples: vec![0i16; 64],
                sample_rate: 22050,
            },
        );
        assert_eq!(queued(&p), 2);
    }
}
