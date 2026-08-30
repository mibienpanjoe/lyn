use std::io::Cursor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioPlaybackError {
    Unavailable,
    NotPlaying,
}

pub(crate) trait AudioPlaybackPlatform {
    fn play_wav(&mut self, target_id: &str, bytes: Vec<u8>) -> Result<(), AudioPlaybackError>;
    fn stop(&mut self, target_id: &str) -> Result<(), AudioPlaybackError>;
}

struct ActivePlayback {
    _output: rodio::MixerDeviceSink,
    player: rodio::Player,
    target_id: String,
}

#[derive(Default)]
pub(crate) struct NativeAudioPlaybackPlatform {
    active: Option<ActivePlayback>,
}

impl AudioPlaybackPlatform for NativeAudioPlaybackPlatform {
    fn play_wav(&mut self, target_id: &str, bytes: Vec<u8>) -> Result<(), AudioPlaybackError> {
        if let Some(active) = self.active.take() {
            active.player.stop();
        }
        let output = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|_| AudioPlaybackError::Unavailable)?;
        let player = rodio::play(output.mixer(), Cursor::new(bytes))
            .map_err(|_| AudioPlaybackError::Unavailable)?;
        self.active = Some(ActivePlayback {
            _output: output,
            player,
            target_id: target_id.to_owned(),
        });
        Ok(())
    }

    fn stop(&mut self, target_id: &str) -> Result<(), AudioPlaybackError> {
        if self.active.as_ref().map(|active| active.target_id.as_str()) != Some(target_id) {
            return Err(AudioPlaybackError::NotPlaying);
        }
        let active = self.active.take().ok_or(AudioPlaybackError::NotPlaying)?;
        active.player.stop();
        Ok(())
    }
}
