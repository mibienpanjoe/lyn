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
    // Drop player before the device sink so teardown does not cut the mixer first.
    player: rodio::Player,
    _output: rodio::MixerDeviceSink,
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
        let mut output = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|_| AudioPlaybackError::Unavailable)?;
        output.log_on_drop(false);
        let player = rodio::play(output.mixer(), Cursor::new(bytes))
            .map_err(|_| AudioPlaybackError::Unavailable)?;
        self.active = Some(ActivePlayback {
            player,
            _output: output,
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
