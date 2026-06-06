use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use dawn_app_core::dto::{TerminalEventDto, TerminalProfileDto};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::ipc::Channel;

const READ_BUFFER_SIZE: usize = 8192;

pub(crate) struct TerminalRuntime {
    next_session_id: AtomicU32,
    sessions: HashMap<u32, TerminalSession>,
}

struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Default for TerminalRuntime {
    fn default() -> Self {
        Self {
            next_session_id: AtomicU32::new(1),
            sessions: HashMap::new(),
        }
    }
}

impl TerminalRuntime {
    pub(crate) fn create_session(
        &mut self,
        profile: TerminalProfileDto,
        project_root: PathBuf,
        cols: u16,
        rows: u16,
        output_channel: Channel<TerminalEventDto>,
    ) -> Result<u32, String> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| error.to_string())?;

        let mut command = command_builder(profile);
        command.cwd(project_root);
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| error.to_string())?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;

        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        spawn_reader(session_id, reader, output_channel);
        self.sessions.insert(
            session_id,
            TerminalSession {
                master: pair.master,
                child,
                writer: Arc::new(Mutex::new(writer)),
            },
        );
        Ok(session_id)
    }

    pub(crate) fn write_input(&self, session_id: u32, data: String) -> Result<(), String> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or_else(|| format!("terminal session `{session_id}` was not found"))?;
        let mut writer = session
            .writer
            .lock()
            .map_err(|_| "terminal writer lock is poisoned".to_string())?;
        writer
            .write_all(data.as_bytes())
            .map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())
    }

    pub(crate) fn resize_session(
        &self,
        session_id: u32,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get(&session_id)
            .ok_or_else(|| format!("terminal session `{session_id}` was not found"))?;
        session
            .master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| error.to_string())
    }

    pub(crate) fn kill_session(&mut self, session_id: u32) -> Result<(), String> {
        let Some(mut session) = self.sessions.remove(&session_id) else {
            return Ok(());
        };
        session.child.kill().map_err(|error| error.to_string())
    }

    pub(crate) fn kill_all(&mut self) {
        let sessions = std::mem::take(&mut self.sessions);
        for mut session in sessions.into_values() {
            let _ = session.child.kill();
        }
    }
}

fn command_builder(profile: TerminalProfileDto) -> CommandBuilder {
    match profile {
        TerminalProfileDto::PowerShell => CommandBuilder::new("powershell.exe"),
        TerminalProfileDto::Cmd => CommandBuilder::new("cmd.exe"),
    }
}

fn spawn_reader(
    session_id: u32,
    mut reader: Box<dyn Read + Send>,
    output_channel: Channel<TerminalEventDto>,
) {
    thread::spawn(move || {
        let mut buffer = [0_u8; READ_BUFFER_SIZE];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => {
                    let _ = output_channel.send(TerminalEventDto::Exited {
                        session_id,
                        exit_code: None,
                    });
                    return;
                }
                Ok(count) => {
                    let data = String::from_utf8_lossy(&buffer[..count]).to_string();
                    let _ = output_channel.send(TerminalEventDto::Output { session_id, data });
                }
                Err(error) => {
                    let _ = output_channel.send(TerminalEventDto::Error {
                        session_id,
                        message: error.to_string(),
                    });
                    return;
                }
            }
        }
    });
}
