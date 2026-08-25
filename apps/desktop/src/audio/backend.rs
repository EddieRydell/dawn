use std::time::Duration;

use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{FromFileError, PlaybackState};
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween};

use crate::dto::SequenceAudio;

type KiraManager = AudioManager<DefaultBackend>;
type KiraStreamingHandle = StreamingSoundHandle<FromFileError>;

pub(super) struct LoadedSource {
    pub(super) audio: SequenceAudio,
    pub(super) canonical_path: String,
    pub(super) duration_seconds: f64,
}

pub(super) struct SourceMetadata {
    pub(super) duration_seconds: f64,
}

pub(super) trait AudioDriver: Send {
    fn load_metadata(&mut self, path: &str) -> Result<SourceMetadata, String>;
    fn play(&mut self, path: &str, position_seconds: f64) -> Result<Box<dyn AudioHandle>, String>;
}

pub(super) trait AudioHandle: Send {
    fn observe(&mut self) -> BackendObservation;
    fn pause(&mut self);
    fn resume(&mut self);
    fn seek_to(&mut self, position_seconds: f64);
    fn stop(&mut self);
}

pub(super) struct BackendObservation {
    pub(super) state: BackendPlaybackState,
    pub(super) position_seconds: f64,
    pub(super) error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum BackendPlaybackState {
    Advancing,
    Paused,
    Stopped,
}

pub(super) struct KiraAudioDriver {
    manager: KiraManager,
}

impl KiraAudioDriver {
    pub(super) fn new() -> Result<Self, String> {
        AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map(|manager| Self { manager })
            .map_err(|error| error.to_string())
    }
}

impl AudioDriver for KiraAudioDriver {
    fn load_metadata(&mut self, path: &str) -> Result<SourceMetadata, String> {
        StreamingSoundData::from_file(path)
            .map(|sound| SourceMetadata {
                duration_seconds: sound.duration().as_secs_f64(),
            })
            .map_err(|error| error.to_string())
    }

    fn play(&mut self, path: &str, position_seconds: f64) -> Result<Box<dyn AudioHandle>, String> {
        let sound = StreamingSoundData::from_file(path)
            .map_err(|error| error.to_string())?
            .start_position(position_seconds);
        self.manager
            .play(sound)
            .map(|handle| Box::new(KiraAudioHandle { handle }) as Box<dyn AudioHandle>)
            .map_err(|error| error.to_string())
    }
}

pub(super) struct KiraAudioHandle {
    handle: KiraStreamingHandle,
}

impl AudioHandle for KiraAudioHandle {
    fn observe(&mut self) -> BackendObservation {
        let state = match self.handle.state() {
            PlaybackState::Playing
            | PlaybackState::Pausing
            | PlaybackState::WaitingToResume
            | PlaybackState::Resuming
            | PlaybackState::Stopping => BackendPlaybackState::Advancing,
            PlaybackState::Paused => BackendPlaybackState::Paused,
            PlaybackState::Stopped => BackendPlaybackState::Stopped,
        };
        BackendObservation {
            state,
            position_seconds: self.handle.position(),
            error: self.handle.pop_error().map(|error| error.to_string()),
        }
    }

    fn pause(&mut self) {
        self.handle.pause(instant_tween());
    }

    fn resume(&mut self) {
        self.handle.resume(instant_tween());
    }

    fn seek_to(&mut self, position_seconds: f64) {
        self.handle.seek_to(position_seconds);
    }

    fn stop(&mut self) {
        self.handle.stop(instant_tween());
    }
}

pub(super) fn canonical_audio_path(path: &str) -> Result<String, std::io::Error> {
    std::fs::canonicalize(path).map(|path| path.to_string_lossy().into_owned())
}

pub(super) fn instant_tween() -> Tween {
    Tween {
        duration: Duration::ZERO,
        ..Tween::default()
    }
}
