use std::{
    num::NonZero,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use rodio::microphone::MicrophoneBuilder;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RecordedAudio {
    pub(crate) samples: Vec<f32>,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioInputError {
    DeviceUnavailable,
    RecordingFailed,
    AlreadyRecording,
    NotRecording,
}

pub(crate) trait AudioInputPlatform {
    fn start(&mut self, input_device_id: Option<&str>) -> Result<(), AudioInputError>;
    fn stop(&mut self) -> Result<RecordedAudio, AudioInputError>;
}

struct RecordingHandle {
    stop: Arc<AtomicBool>,
    thread: thread::JoinHandle<Result<Vec<f32>, AudioInputError>>,
    sample_rate: u32,
    channels: u16,
}

#[derive(Default)]
pub(crate) struct NativeAudioInputPlatform {
    recording: Option<RecordingHandle>,
    #[cfg(all(test, target_os = "linux"))]
    synthetic_recording: Option<bool>,
}

impl NativeAudioInputPlatform {
    /// Only the private desktop test entry installs this; no environment switch
    /// or production command can replace physical input with these samples.
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn for_desktop_fixture() -> Self {
        Self {
            recording: None,
            synthetic_recording: Some(false),
        }
    }

    fn start_default(&mut self) -> Result<(), AudioInputError> {
        if self.recording.is_some() {
            return Err(AudioInputError::AlreadyRecording);
        }
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let builder = MicrophoneBuilder::new()
            .default_device()
            .and_then(|builder| builder.default_config())
            .map_err(|_| AudioInputError::DeviceUnavailable)?;
        let builder = builder
            .prefer_channel_counts([NonZero::new(1).expect("one is non-zero")])
            .prefer_sample_rates([NonZero::new(16_000).expect("sample rate is non-zero")]);
        let config = builder.get_config();
        let mut microphone = builder
            .open_stream()
            .map_err(|_| AudioInputError::RecordingFailed)?;
        let sample_rate = config.sample_rate.get();
        let channels = config.channel_count.get();
        let thread = thread::spawn(move || {
            let mut samples = Vec::new();
            while !thread_stop.load(Ordering::Acquire) {
                let Some(sample) = microphone.next() else {
                    return Err(AudioInputError::RecordingFailed);
                };
                samples.push(sample);
            }
            Ok(samples)
        });
        self.recording = Some(RecordingHandle {
            stop,
            thread,
            sample_rate,
            channels,
        });
        Ok(())
    }

    fn stop_recording(&mut self) -> Result<RecordedAudio, AudioInputError> {
        let recording = self.recording.take().ok_or(AudioInputError::NotRecording)?;
        recording.stop.store(true, Ordering::Release);
        let samples = recording
            .thread
            .join()
            .map_err(|_| AudioInputError::RecordingFailed)??;
        if samples.is_empty() {
            return Err(AudioInputError::RecordingFailed);
        }
        Ok(RecordedAudio {
            samples,
            sample_rate: recording.sample_rate,
            channels: recording.channels,
        })
    }
}

impl AudioInputPlatform for NativeAudioInputPlatform {
    fn start(&mut self, input_device_id: Option<&str>) -> Result<(), AudioInputError> {
        if input_device_id.is_some() {
            return Err(AudioInputError::DeviceUnavailable);
        }
        #[cfg(all(test, target_os = "linux"))]
        if let Some(recording) = &mut self.synthetic_recording {
            if *recording {
                return Err(AudioInputError::AlreadyRecording);
            }
            *recording = true;
            return Ok(());
        }
        self.start_default()
    }

    fn stop(&mut self) -> Result<RecordedAudio, AudioInputError> {
        #[cfg(all(test, target_os = "linux"))]
        if let Some(recording) = &mut self.synthetic_recording {
            if !*recording {
                return Err(AudioInputError::NotRecording);
            }
            *recording = false;
            return Ok(RecordedAudio {
                samples: vec![0.25; 1600],
                sample_rate: 16_000,
                channels: 1,
            });
        }
        self.stop_recording()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn desktop_synthetic_input_has_fixed_samples_and_recording_state() {
        let mut input = NativeAudioInputPlatform::for_desktop_fixture();
        assert_eq!(input.stop(), Err(AudioInputError::NotRecording));
        assert_eq!(
            input.start(Some("never-open-this-device")),
            Err(AudioInputError::DeviceUnavailable)
        );
        assert_eq!(input.start(None), Ok(()));
        assert_eq!(input.start(None), Err(AudioInputError::AlreadyRecording));
        let expected = RecordedAudio {
            samples: vec![0.25; 1600],
            sample_rate: 16_000,
            channels: 1,
        };
        assert_eq!(input.stop(), Ok(expected.clone()));
        assert_eq!(input.stop(), Err(AudioInputError::NotRecording));
        assert_eq!(input.start(None), Ok(()));
        assert_eq!(input.stop(), Ok(expected));
        assert!(input.recording.is_none());
    }
}
