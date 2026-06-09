use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::SystemTime;

use dawn_app_core::document::SequenceAudioDocument;
use dawn_app_core::preview_session::AudioPlaybackStatus;
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::FromFileError;
use kira::sound::PlaybackState;
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween};

#[derive(Debug, Clone)]
pub struct AudioClock {
    pub position_seconds: f64,
    pub ended: bool,
    pub status: AudioPlaybackStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioKey {
    path: PathBuf,
    modified_epoch_millis: u128,
    len: u64,
}

#[derive(Debug)]
struct LoadResult {
    generation: u64,
    key: AudioKey,
    result: Result<(), String>,
}

pub struct AudioRuntime {
    inner: Mutex<AudioRuntimeInner>,
}

struct AudioRuntimeInner {
    manager: Option<AudioManager<DefaultBackend>>,
    sender: mpsc::Sender<LoadResult>,
    receiver: mpsc::Receiver<LoadResult>,
    generation: u64,
    active_key: Option<AudioKey>,
    asset_state: AssetState,
    handle: Option<StreamingSoundHandle<FromFileError>>,
    transport_intent: TransportIntent,
    position_seconds: f64,
    ended: bool,
    status: AudioPlaybackStatus,
    error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AssetState {
    Empty,
    Loading,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum TransportIntent {
    None,
    PlayWhenReady {
        position_seconds: f64,
    },
    Seek {
        position_seconds: f64,
        playback: SeekPlayback,
    },
    Pause {
        position_seconds: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SeekPlayback {
    Playing,
    Paused,
}

const SEEK_SETTLED_TOLERANCE_SECONDS: f64 = 0.1;

impl Default for AudioRuntime {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        let manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .map_err(|error| format!("failed to initialize native audio: {error}"));
        let inner = match manager {
            Ok(manager) => AudioRuntimeInner {
                manager: Some(manager),
                sender,
                receiver,
                generation: 0,
                active_key: None,
                asset_state: AssetState::Empty,
                handle: None,
                transport_intent: TransportIntent::None,
                position_seconds: 0.0,
                ended: false,
                status: AudioPlaybackStatus::None,
                error: None,
            },
            Err(error) => AudioRuntimeInner {
                manager: None,
                sender,
                receiver,
                generation: 0,
                active_key: None,
                asset_state: AssetState::Empty,
                handle: None,
                transport_intent: TransportIntent::None,
                position_seconds: 0.0,
                ended: false,
                status: AudioPlaybackStatus::Error,
                error: Some(error),
            },
        };
        Self {
            inner: Mutex::new(inner),
        }
    }
}

impl AudioRuntime {
    pub fn preload(&self, audio: &SequenceAudioDocument) -> Result<AudioClock, String> {
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        inner.preload(audio)?;
        Ok(inner.clock())
    }

    pub fn play(
        &self,
        audio: &SequenceAudioDocument,
        position_seconds: f64,
    ) -> Result<AudioClock, String> {
        validate_position_seconds(position_seconds)?;
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        inner.ensure_active(audio, position_seconds)?;
        if inner.status == AudioPlaybackStatus::LoadingToPlay {
            inner.transport_intent = TransportIntent::None;
            inner.position_seconds = position_seconds;
            inner.status = AudioPlaybackStatus::Loading;
        } else if inner.handle.is_some() {
            inner.play_handle(position_seconds)?;
        } else if inner.asset_state == AssetState::Ready {
            inner.start_stream(position_seconds)?;
        } else if inner.status.is_loading() {
            inner.transport_intent = TransportIntent::PlayWhenReady { position_seconds };
            inner.position_seconds = position_seconds;
            inner.status = AudioPlaybackStatus::LoadingToPlay;
        }
        Ok(inner.clock())
    }

    pub fn pause(&self) -> Result<AudioClock, String> {
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        let position_seconds = inner.current_position_seconds();
        inner.transport_intent = TransportIntent::None;
        inner.pause_handle(position_seconds);
        inner.position_seconds = position_seconds;
        inner.ended = false;
        inner.status = if inner.is_ready_to_play() {
            AudioPlaybackStatus::Ready
        } else if inner.active_key.is_some() {
            AudioPlaybackStatus::Loading
        } else {
            AudioPlaybackStatus::None
        };
        inner.error = None;
        Ok(inner.clock())
    }

    pub fn stop(&self, position_seconds: f64) -> Result<AudioClock, String> {
        validate_position_seconds(position_seconds)?;
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        inner.transport_intent = TransportIntent::None;
        inner.stop_handle();
        inner.position_seconds = position_seconds;
        inner.ended = false;
        inner.status = if inner.asset_state == AssetState::Ready {
            AudioPlaybackStatus::Ready
        } else if inner.active_key.is_some() {
            AudioPlaybackStatus::Loading
        } else {
            AudioPlaybackStatus::None
        };
        inner.error = None;
        Ok(inner.clock())
    }

    pub fn seek(
        &self,
        audio: &SequenceAudioDocument,
        position_seconds: f64,
        playing: bool,
    ) -> Result<AudioClock, String> {
        validate_position_seconds(position_seconds)?;
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        inner.ensure_active(audio, position_seconds)?;
        if let Some(handle) = inner.handle.as_mut() {
            handle.seek_to(position_seconds);
            if playing {
                handle.resume(Tween::default());
                inner.status = AudioPlaybackStatus::Playing;
                inner.transport_intent = TransportIntent::Seek {
                    position_seconds,
                    playback: SeekPlayback::Playing,
                };
            } else {
                handle.pause(Tween::default());
                inner.status = AudioPlaybackStatus::Ready;
                inner.transport_intent = TransportIntent::Seek {
                    position_seconds,
                    playback: SeekPlayback::Paused,
                };
            }
            inner.position_seconds = position_seconds;
            inner.ended = false;
            inner.error = None;
        } else if playing && inner.asset_state == AssetState::Ready {
            inner.start_stream(position_seconds)?;
        } else {
            inner.transport_intent = if playing && inner.status.is_loading() {
                TransportIntent::PlayWhenReady { position_seconds }
            } else {
                TransportIntent::None
            };
            inner.stop_handle();
            inner.position_seconds = position_seconds;
            inner.ended = false;
            if inner.asset_state == AssetState::Ready {
                inner.status = AudioPlaybackStatus::Ready;
            } else if matches!(
                inner.transport_intent,
                TransportIntent::PlayWhenReady { .. }
            ) {
                inner.status = AudioPlaybackStatus::LoadingToPlay;
            }
        }
        Ok(inner.clock())
    }

    pub fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.clear();
        }
    }

    pub fn clock(&self) -> Result<AudioClock, String> {
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        Ok(inner.clock())
    }

    fn lock_inner(&self) -> Result<std::sync::MutexGuard<'_, AudioRuntimeInner>, String> {
        self.inner
            .lock()
            .map_err(|_| "audio runtime lock is poisoned".to_string())
    }
}

fn validate_position_seconds(position_seconds: f64) -> Result<(), String> {
    if position_seconds.is_finite() && position_seconds >= 0.0 {
        Ok(())
    } else {
        Err("audio position seconds must be finite and non-negative".to_string())
    }
}

impl AudioRuntimeInner {
    fn preload(&mut self, audio: &SequenceAudioDocument) -> Result<(), String> {
        self.transport_intent = TransportIntent::None;
        self.ensure_active(audio, 0.0)
    }

    fn ensure_active(
        &mut self,
        audio: &SequenceAudioDocument,
        position_seconds: f64,
    ) -> Result<(), String> {
        if !audio.exists {
            self.clear();
            self.status = AudioPlaybackStatus::Missing;
            return Ok(());
        }
        if self.manager.is_none() {
            self.status = AudioPlaybackStatus::Error;
            if self.error.is_none() {
                self.error = Some("native audio is not available".to_string());
            }
            return Ok(());
        }
        let key = audio_key(&audio.resolved_path)?;
        if self.active_key.as_ref() == Some(&key) {
            return Ok(());
        }
        self.generation = self.generation.saturating_add(1);
        self.stop_handle();
        self.active_key = Some(key.clone());
        self.asset_state = AssetState::Loading;
        self.transport_intent = TransportIntent::None;
        self.position_seconds = position_seconds;
        self.ended = false;
        self.status = AudioPlaybackStatus::Loading;
        self.error = None;
        self.spawn_loader(key, self.generation);
        Ok(())
    }

    fn spawn_loader(&self, key: AudioKey, generation: u64) {
        let sender = self.sender.clone();
        thread::spawn(move || {
            let result = StreamingSoundData::from_file(&key.path)
                .map(|_| ())
                .map_err(|error| {
                    format!(
                        "failed to prepare audio stream `{}`: {error}",
                        key.path.display()
                    )
                });
            let _ = sender.send(LoadResult {
                generation,
                key,
                result,
            });
        });
    }

    fn poll_load_results(&mut self) {
        while let Ok(result) = self.receiver.try_recv() {
            if result.generation != self.generation || self.active_key.as_ref() != Some(&result.key)
            {
                continue;
            }
            match result.result {
                Ok(()) => {
                    self.asset_state = AssetState::Ready;
                    self.error = None;
                    self.ended = false;
                    if let TransportIntent::PlayWhenReady { position_seconds } =
                        self.transport_intent
                    {
                        if let Err(error) = self.start_stream(position_seconds) {
                            self.stop_handle();
                            self.asset_state = AssetState::Empty;
                            self.status = AudioPlaybackStatus::Error;
                            self.error = Some(error);
                        }
                    } else {
                        self.status = AudioPlaybackStatus::Ready;
                    }
                }
                Err(error) => {
                    self.stop_handle();
                    self.asset_state = AssetState::Empty;
                    self.transport_intent = TransportIntent::None;
                    self.status = AudioPlaybackStatus::Error;
                    self.error = Some(error);
                    self.ended = false;
                }
            }
        }
    }

    fn play_handle(&mut self, position_seconds: f64) -> Result<(), String> {
        if self.is_same_logical_position(position_seconds) {
            if let Some(handle) = self.handle.as_mut() {
                handle.resume(Tween::default());
            }
            self.transport_intent = TransportIntent::None;
            self.ended = false;
            self.status = AudioPlaybackStatus::Playing;
            self.error = None;
            return Ok(());
        }
        if let Some(handle) = self.handle.as_mut() {
            handle.seek_to(position_seconds);
            handle.resume(Tween::default());
            self.transport_intent = TransportIntent::Seek {
                position_seconds,
                playback: SeekPlayback::Playing,
            };
            self.position_seconds = position_seconds;
            self.ended = false;
            self.status = AudioPlaybackStatus::Playing;
            self.error = None;
            return Ok(());
        }

        self.start_stream(position_seconds)
    }

    fn start_stream(&mut self, position_seconds: f64) -> Result<(), String> {
        if self.asset_state != AssetState::Ready {
            self.position_seconds = position_seconds;
            self.transport_intent = TransportIntent::PlayWhenReady { position_seconds };
            self.status = AudioPlaybackStatus::LoadingToPlay;
            return Ok(());
        }
        let key = self
            .active_key
            .as_ref()
            .ok_or_else(|| "native audio stream is not prepared".to_string())?;
        let data = StreamingSoundData::from_file(&key.path).map_err(|error| {
            format!(
                "failed to prepare audio stream `{}`: {error}",
                key.path.display()
            )
        })?;
        let handle = self
            .manager
            .as_mut()
            .ok_or_else(|| "native audio is not available".to_string())?
            .play(data.start_position(position_seconds))
            .map_err(|error| format!("failed to start native audio: {error:?}"))?;
        self.handle = Some(handle);
        self.transport_intent = TransportIntent::None;
        self.position_seconds = position_seconds;
        self.ended = false;
        self.status = AudioPlaybackStatus::Playing;
        self.error = None;
        Ok(())
    }

    fn pause_handle(&mut self, position_seconds: f64) {
        if let Some(handle) = self.handle.as_mut() {
            handle.pause(Tween::default());
            self.transport_intent = TransportIntent::Pause { position_seconds };
        }
    }

    fn stop_handle(&mut self) {
        if let Some(mut handle) = self.handle.take() {
            handle.stop(Tween::default());
        }
    }

    fn is_ready_to_play(&self) -> bool {
        self.asset_state == AssetState::Ready || self.handle.is_some()
    }

    fn current_position_seconds(&self) -> f64 {
        match self.transport_intent {
            TransportIntent::Seek {
                position_seconds, ..
            }
            | TransportIntent::Pause { position_seconds } => return position_seconds,
            TransportIntent::None | TransportIntent::PlayWhenReady { .. } => {}
        }
        self.handle
            .as_ref()
            .map(|handle| handle.position())
            .unwrap_or(self.position_seconds)
    }

    fn clock(&mut self) -> AudioClock {
        let handle_position_seconds = self.handle.as_ref().map(|handle| handle.position());
        let position_seconds = self.current_position_seconds();
        let stream_error = self.handle.as_mut().and_then(|handle| handle.pop_error());
        if let Some(error) = stream_error {
            self.handle = None;
            self.transport_intent = TransportIntent::None;
            self.position_seconds = position_seconds;
            self.ended = false;
            self.status = AudioPlaybackStatus::Error;
            self.error = Some(format!("audio stream decode error: {error}"));
            return AudioClock {
                position_seconds: self.position_seconds,
                ended: self.ended,
                status: self.status,
                error: self.error.clone(),
            };
        }
        if let Some(state) = self.handle.as_ref().map(|handle| handle.state()) {
            match state {
                PlaybackState::Stopped => {
                    self.handle = None;
                    self.transport_intent = TransportIntent::None;
                    self.position_seconds = position_seconds;
                    self.ended = true;
                    self.status = AudioPlaybackStatus::Ended;
                }
                _ if self.reconcile_transport_intent(state, handle_position_seconds) => {}
                state if state.is_advancing() => {
                    self.position_seconds = position_seconds;
                    self.ended = false;
                    if self.status != AudioPlaybackStatus::Ready {
                        self.status = AudioPlaybackStatus::Playing;
                    }
                }
                _ => {
                    self.position_seconds = position_seconds;
                }
            }
        }
        AudioClock {
            position_seconds: self.position_seconds,
            ended: self.ended,
            status: self.status,
            error: self.error.clone(),
        }
    }

    fn clear(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.stop_handle();
        self.active_key = None;
        self.asset_state = AssetState::Empty;
        self.transport_intent = TransportIntent::None;
        self.position_seconds = 0.0;
        self.ended = false;
        self.status = AudioPlaybackStatus::None;
        self.error = None;
        while self.receiver.try_recv().is_ok() {}
    }

    fn reconcile_transport_intent(
        &mut self,
        state: PlaybackState,
        handle_position_seconds: Option<f64>,
    ) -> bool {
        match self.transport_intent {
            TransportIntent::None | TransportIntent::PlayWhenReady { .. } => false,
            TransportIntent::Pause { position_seconds } => {
                self.position_seconds = position_seconds;
                self.ended = false;
                self.status = AudioPlaybackStatus::Ready;
                if !state.is_advancing() {
                    self.transport_intent = TransportIntent::None;
                }
                true
            }
            TransportIntent::Seek {
                position_seconds,
                playback,
            } => {
                let settled = handle_position_seconds
                    .map(|handle_position_seconds| {
                        (handle_position_seconds - position_seconds).abs()
                            <= SEEK_SETTLED_TOLERANCE_SECONDS
                    })
                    .unwrap_or(false);
                if settled {
                    self.transport_intent = TransportIntent::None;
                    self.position_seconds = handle_position_seconds.unwrap_or(position_seconds);
                    self.ended = false;
                    self.status = match playback {
                        SeekPlayback::Playing if state.is_advancing() => {
                            AudioPlaybackStatus::Playing
                        }
                        SeekPlayback::Playing => AudioPlaybackStatus::Playing,
                        SeekPlayback::Paused => AudioPlaybackStatus::Ready,
                    };
                } else {
                    self.position_seconds = position_seconds;
                    self.ended = false;
                    self.status = match playback {
                        SeekPlayback::Playing => AudioPlaybackStatus::Playing,
                        SeekPlayback::Paused => AudioPlaybackStatus::Ready,
                    };
                }
                true
            }
        }
    }

    fn is_same_logical_position(&self, position_seconds: f64) -> bool {
        (position_seconds - self.position_seconds).abs() <= SEEK_SETTLED_TOLERANCE_SECONDS
    }
}

fn audio_key(path: &str) -> Result<AudioKey, String> {
    let path = PathBuf::from(path);
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("failed to inspect audio file `{}`: {error}", path.display()))?;
    let modified_epoch_millis = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    Ok(AudioKey {
        path,
        modified_epoch_millis,
        len: metadata.len(),
    })
}
