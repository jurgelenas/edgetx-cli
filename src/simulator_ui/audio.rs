use std::num::NonZero;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::Receiver;

/// Maximum firmware volume level (EdgeTX volume range is 0..=23).
pub(crate) const VOLUME_LEVEL_MAX: i32 = 23;

/// Firmware audio sample rate in Hz.
const SAMPLE_RATE: u32 = 32_000;

/// Shared mute/volume state between the UI, WASM and audio pump threads.
#[derive(Clone)]
pub struct AudioControls {
    /// Set by the UI mute checkbox, read by the pump.
    muted: Arc<AtomicBool>,
    /// Firmware volume level (0..=23), written by the WASM thread.
    volume: Arc<AtomicI32>,
}

impl AudioControls {
    pub fn new() -> Self {
        Self {
            muted: Arc::new(AtomicBool::new(false)),
            volume: Arc::new(AtomicI32::new(VOLUME_LEVEL_MAX)),
        }
    }

    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }

    pub fn set_volume_level(&self, level: i32) {
        self.volume.store(level, Ordering::Relaxed);
    }

    pub fn volume_percent(&self) -> u8 {
        let level = self.volume.load(Ordering::Relaxed);
        (level as f32 / VOLUME_LEVEL_MAX as f32 * 100.0) as u8
    }

    fn effective_volume(&self) -> f32 {
        if self.is_muted() {
            0.0
        } else {
            let level = self.volume.load(Ordering::Relaxed);
            (level as f32 / VOLUME_LEVEL_MAX as f32).clamp(0.0, 1.0)
        }
    }
}

impl Default for AudioControls {
    fn default() -> Self {
        Self::new()
    }
}

/// Audio player for simulator PCM playback.
pub struct AudioPlayer {
    player: Option<rodio::Player>,
    _sink: Option<rodio::MixerDeviceSink>,
}

impl AudioPlayer {
    pub fn new() -> Self {
        match rodio::DeviceSinkBuilder::from_default_device().and_then(|b| b.open_stream()) {
            Ok(sink) => {
                let player = rodio::Player::connect_new(sink.mixer());
                Self {
                    player: Some(player),
                    _sink: Some(sink),
                }
            }
            Err(e) => {
                log::warn!("audio output unavailable: {e}");
                Self {
                    player: None,
                    _sink: None,
                }
            }
        }
    }

    /// Queue PCM samples (mono, 16-bit signed, 32kHz) scaled by `volume` (0.0..1.0).
    pub fn play_samples(&self, samples: &[i16], sample_rate: u32, volume: f32) {
        if let Some(ref player) = self.player {
            let samples_f32: Vec<f32> = samples
                .iter()
                .map(|&s| s as f32 / 32768.0 * volume)
                .collect();
            let channels = NonZero::new(1u16).unwrap();
            let rate = NonZero::new(sample_rate).unwrap();
            let source = rodio::buffer::SamplesBuffer::new(channels, rate, samples_f32);
            player.append(source);
        }
    }

    #[allow(dead_code)]
    pub fn stop(&self) {
        if let Some(ref player) = self.player {
            player.clear();
        }
    }
}

/// Spawn the audio pump thread: drains firmware audio chunks and plays them
/// in real time, independent of the UI render loop (so audio keeps playing
/// while the window is minimized and no backlog can accumulate).
///
/// The loop drains unconditionally even when no audio device is available,
/// keeping the channel empty. The thread is detached; the sender lives in a
/// static, so `recv()` blocks until process exit.
pub fn spawn_audio_pump(rx: Receiver<Vec<i16>>, controls: AudioControls) {
    std::thread::Builder::new()
        .name("audio-pump".to_owned())
        .spawn(move || {
            let player = AudioPlayer::new();
            while let Ok(samples) = rx.recv() {
                player.play_samples(&samples, SAMPLE_RATE, controls.effective_volume());
            }
        })
        .expect("failed to spawn audio pump thread");
}
