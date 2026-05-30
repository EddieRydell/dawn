use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;
use std::time::SystemTime;

use dawn_project::document::SequenceAudioDocument;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::PlaybackState;
use kira::{AudioManager, AudioManagerSettings, DefaultBackend, Tween};

#[derive(Debug, Clone)]
pub struct AudioClock {
    pub position_ms: u64,
    pub ended: bool,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioKey {
    path: PathBuf,
    modified_ms: u128,
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
    position_ms: u64,
    ended: bool,
    status: String,
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
                position_ms: 0,
                ended: false,
                status: "noAudio".to_string(),
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
                position_ms: 0,
                ended: false,
                status: "error".to_string(),
                error: Some(error),
            },
        };
        Self {
            inner: Mutex::new(inner),
        }
    }
}

impl AudioRuntime {
    pub fn load_active(&self, audio: &SequenceAudioDocument) -> Result<AudioClock, String> {
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        inner.load_active(audio)?;
        Ok(inner.clock())
    }

    pub fn play(
        &self,
        audio: &SequenceAudioDocument,
        position_ms: u64,
    ) -> Result<AudioClock, String> {
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        inner.ensure_active(audio, position_ms)?;
        if inner.active_data.is_some() {
            inner.start(position_ms)?;
        }
        Ok(inner.clock())
    }

    pub fn pause(&self) -> Result<AudioClock, String> {
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        let position_ms = inner.current_position_ms();
        inner.stop_handle();
        inner.position_ms = position_ms;
        inner.ended = false;
        inner.status = if inner.active_data.is_some() {
            "ready"
        } else {
            "noAudio"
        }
        .to_string();
        inner.error = None;
        Ok(inner.clock())
    }

    pub fn stop(&self, position_ms: u64) -> Result<AudioClock, String> {
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        inner.stop_handle();
        inner.position_ms = position_ms;
        inner.ended = false;
        inner.status = if inner.active_data.is_some() {
            "ready"
        } else {
            "noAudio"
        }
        .to_string();
        inner.error = None;
        Ok(inner.clock())
    }

    pub fn seek(
        &self,
        audio: &SequenceAudioDocument,
        position_ms: u64,
        playing: bool,
    ) -> Result<AudioClock, String> {
        let mut inner = self.lock_inner()?;
        inner.poll_load_results();
        inner.ensure_active(audio, position_ms)?;
        if playing && inner.active_data.is_some() {
            inner.start(position_ms)?;
        } else {
            inner.stop_handle();
            inner.position_ms = position_ms;
            inner.ended = false;
            if inner.active_data.is_some() {
                inner.status = "ready".to_string();
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

impl AudioRuntimeInner {
    fn load_active(&mut self, audio: &SequenceAudioDocument) -> Result<(), String> {
        self.ensure_active(audio, 0)
    }

    fn ensure_active(
        &mut self,
        audio: &SequenceAudioDocument,
        position_ms: u64,
    ) -> Result<(), String> {
        if !audio.exists {
            self.clear();
            self.status = "missingAudio".to_string();
            return Ok(());
        }
        if self.manager.is_none() {
            self.status = "error".to_string();
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
        self.position_ms = position_ms;
        self.ended = false;
        self.status = "loading".to_string();
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
                    self.status = "ready".to_string();
                    self.error = None;
                    self.ended = false;
                }
                Err(error) => {
                    self.stop_handle();
                    self.active_data = None;
                    self.status = "error".to_string();
                    self.error = Some(error);
                    self.ended = false;
                }
            }
        }
    }

    fn start(&mut self, position_ms: u64) -> Result<(), String> {
        self.stop_handle();
        let Some(data) = self.active_data.clone() else {
            self.position_ms = position_ms;
            self.status = "loading".to_string();
            return Ok(());
        };
        let seconds = position_ms as f64 / 1_000.0;
        let handle = self
            .manager
            .as_mut()
            .ok_or_else(|| "native audio is not available".to_string())?
            .play(data.start_position(seconds))
            .map_err(|error| format!("failed to start native audio: {error:?}"))?;
        self.handle = Some(handle);
        self.position_ms = position_ms;
        self.ended = false;
        self.status = "playing".to_string();
        self.error = None;
        Ok(())
    }

    fn stop_handle(&mut self) {
        if let Some(mut handle) = self.handle.take() {
            handle.stop(Tween::default());
        }
    }

    fn current_position_ms(&self) -> u64 {
        self.handle
            .as_ref()
            .map(|handle| seconds_to_ms(handle.position()))
            .unwrap_or(self.position_ms)
    }

    fn clock(&mut self) -> AudioClock {
        let position_ms = self.current_position_ms();
        if let Some(handle) = &self.handle {
            match handle.state() {
                PlaybackState::Stopped => {
                    self.handle = None;
                    self.position_ms = position_ms;
                    self.ended = true;
                    self.status = "ended".to_string();
                }
                state if state.is_advancing() => {
                    self.position_ms = position_ms;
                    self.ended = false;
                    self.status = "playing".to_string();
                }
                _ => {
                    self.position_ms = position_ms;
                }
            }
        }
        AudioClock {
            position_ms: self.position_ms,
            ended: self.ended,
            status: self.status.clone(),
            error: self.error.clone(),
        }
    }

    fn clear(&mut self) {
        self.generation = self.generation.saturating_add(1);
        self.stop_handle();
        self.active_key = None;
        self.active_data = None;
        self.position_ms = 0;
        self.ended = false;
        self.status = "noAudio".to_string();
        self.error = None;
        while self.receiver.try_recv().is_ok() {}
    }
}

fn audio_key(path: &str) -> Result<AudioKey, String> {
    let path = PathBuf::from(path);
    let metadata = std::fs::metadata(&path)
        .map_err(|error| format!("failed to inspect audio file `{}`: {error}", path.display()))?;
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    Ok(AudioKey {
        path,
        modified_ms,
        len: metadata.len(),
    })
}

fn seconds_to_ms(seconds: f64) -> u64 {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * 1_000.0).round().min(u64::MAX as f64) as u64
    } else {
        0
    }
}
