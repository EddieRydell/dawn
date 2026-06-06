use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::SystemTime;

use dawn_app_core::preview_session::AudioPlaybackStatus;
use dawn_project::document::SequenceAudioDocument;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
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
    result: Result<StaticSoundData, String>,
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
    active_data: Option<StaticSoundData>,
    handle: Option<StaticSoundHandle>,
    pending_play_seconds: Option<f64>,
    position_seconds: f64,
    ended: bool,
    status: AudioPlaybackStatus,
    error: Option<String>,
}

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
                active_data: None,
                handle: None,
                pending_play_seconds: None,
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
                active_data: None,
                handle: None,
                pending_play_seconds: None,
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
            inner.pending_play_seconds = None;
            inner.position_seconds = position_seconds;
            inner.status = AudioPlaybackStatus::Loading;
        } else if inner.active_data.is_some() {
            inner.start(position_seconds)?;
        } else if inner.status.is_loading() {
            inner.pending_play_seconds = Some(position_seconds);
            inner.position_seconds = position_seconds;
            inner.status = AudioPlaybackStatus::LoadingToPlay;
        }
        Ok(inner.clock())
    }

    pub fn pause(&self) -> Result<AudioClock, String> {
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        let position_seconds = inner.current_position_seconds();
        inner.pending_play_seconds = None;
        inner.stop_handle();
        inner.position_seconds = position_seconds;
        inner.ended = false;
        inner.status = if inner.active_data.is_some() {
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
        inner.pending_play_seconds = None;
        inner.stop_handle();
        inner.position_seconds = position_seconds;
        inner.ended = false;
        inner.status = if inner.active_data.is_some() {
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
        if playing && inner.active_data.is_some() {
            inner.start(position_seconds)?;
        } else {
            inner.pending_play_seconds = if playing && inner.status.is_loading() {
                Some(position_seconds)
            } else {
                None
            };
            inner.stop_handle();
            inner.position_seconds = position_seconds;
            inner.ended = false;
            if inner.active_data.is_some() {
                inner.status = AudioPlaybackStatus::Ready;
            } else if inner.pending_play_seconds.is_some() {
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
        self.pending_play_seconds = None;
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
        self.active_data = None;
        self.pending_play_seconds = None;
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
            let result = StaticSoundData::from_file(&key.path).map_err(|error| {
                format!(
                    "failed to load audio file `{}`: {error}",
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
                Ok(data) => {
                    self.active_data = Some(data);
                    self.error = None;
                    self.ended = false;
                    if let Some(position_seconds) = self.pending_play_seconds.take() {
                        if let Err(error) = self.start(position_seconds) {
                            self.stop_handle();
                            self.active_data = None;
                            self.status = AudioPlaybackStatus::Error;
                            self.error = Some(error);
                        }
                    } else {
                        self.status = AudioPlaybackStatus::Ready;
                    }
                }
                Err(error) => {
                    self.stop_handle();
                    self.active_data = None;
                    self.pending_play_seconds = None;
                    self.status = AudioPlaybackStatus::Error;
                    self.error = Some(error);
                    self.ended = false;
                }
            }
        }
    }

    fn start(&mut self, position_seconds: f64) -> Result<(), String> {
        self.stop_handle();
        let Some(data) = self.active_data.clone() else {
            self.position_seconds = position_seconds;
            self.pending_play_seconds = Some(position_seconds);
            self.status = AudioPlaybackStatus::LoadingToPlay;
            return Ok(());
        };
        let handle = self
            .manager
            .as_mut()
            .ok_or_else(|| "native audio is not available".to_string())?
            .play(data.start_position(position_seconds))
            .map_err(|error| format!("failed to start native audio: {error:?}"))?;
        self.handle = Some(handle);
        self.pending_play_seconds = None;
        self.position_seconds = position_seconds;
        self.ended = false;
        self.status = AudioPlaybackStatus::Playing;
        self.error = None;
        Ok(())
    }

    fn stop_handle(&mut self) {
        if let Some(mut handle) = self.handle.take() {
            handle.stop(Tween::default());
        }
    }

    fn current_position_seconds(&self) -> f64 {
        self.handle
            .as_ref()
            .map(|handle| handle.position())
            .unwrap_or(self.position_seconds)
    }

    fn clock(&mut self) -> AudioClock {
        let position_seconds = self.current_position_seconds();
        if let Some(handle) = &self.handle {
            match handle.state() {
                PlaybackState::Stopped => {
                    self.handle = None;
                    self.position_seconds = position_seconds;
                    self.ended = true;
                    self.pending_play_seconds = None;
                    self.status = AudioPlaybackStatus::Ended;
                }
                state if state.is_advancing() => {
                    self.position_seconds = position_seconds;
                    self.ended = false;
                    self.status = AudioPlaybackStatus::Playing;
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
        self.active_data = None;
        self.pending_play_seconds = None;
        self.position_seconds = 0.0;
        self.ended = false;
        self.status = AudioPlaybackStatus::None;
        self.error = None;
        while self.receiver.try_recv().is_ok() {}
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
