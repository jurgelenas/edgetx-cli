use std::num::NonZero;

use crate::simulator::SimulatorError;

/// Audio player for simulator PCM playback.
pub struct AudioPlayer {
    player: Option<rodio::Player>,
    _sink: Option<rodio::MixerDeviceSink>,
}

impl AudioPlayer {
    pub fn new() -> Result<Self, SimulatorError> {
        match rodio::DeviceSinkBuilder::from_default_device().and_then(|b| b.open_stream()) {
            Ok(sink) => {
                let player = rodio::Player::connect_new(sink.mixer());
                Ok(Self {
                    player: Some(player),
                    _sink: Some(sink),
                })
            }
            Err(e) => {
                log::warn!("audio output unavailable: {e}");
                Ok(Self {
                    player: None,
                    _sink: None,
                })
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
