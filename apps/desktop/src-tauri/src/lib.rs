use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use walkdir::WalkDir;

#[derive(Default)]
struct AppState {
    project: Mutex<ProjectSession>,
    language_service: Mutex<Option<LanguageService>>,
}

#[derive(Default)]
struct ProjectSession {
    root: Option<PathBuf>,
    project_file: Option<PathBuf>,
    active_sequence: Option<PathBuf>,
}

struct LanguageService {
    stop: watch::Sender<bool>,
    accept_task: JoinHandle<()>,
    connection_tasks: Arc<tokio::sync::Mutex<Vec<JoinHandle<()>>>>,
}

#[derive(Serialize)]
struct ProjectState {
    root: String,
    files: Vec<String>,
    entries: Vec<ProjectEntryDto>,
    diagnostics: Vec<DiagnosticDto>,
}

#[derive(Serialize)]
struct ProjectEntryDto {
    path: String,
    kind: String,
}

#[derive(Serialize)]
struct FileOperationState {
    project: ProjectState,
    moved: Vec<FileMoveDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileMoveDto {
    old_path: String,
    new_path: String,
}

#[derive(Serialize)]
struct DiagnosticDto {
    severity: String,
    path: String,
    message: String,
}

#[derive(Serialize)]
struct LanguageServiceState {
    url: String,
    status: LanguageServiceStatus,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
enum LanguageServiceStatus {
    Starting,
    Ready,
    Disconnected,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LanguageServiceStatusEvent {
    status: LanguageServiceStatus,
    message: Option<String>,
}

#[derive(Debug, Clone)]
struct LspProcessConfig {
    binary: PathBuf,
    project_root: PathBuf,
}

#[derive(Serialize)]
struct FrameSummary {
    pixels: usize,
    fixture_spans: usize,
    warnings: Option<Vec<String>>,
}

#[tauri::command]
fn open_project(path: String, state: State<'_, AppState>) -> Result<ProjectState, String> {
    let path = PathBuf::from(path);
    let project_file = if path.is_dir() {
        path.join("project.dawn")
    } else {
        path
    };
    let root = project_file
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "project file has no parent".to_string())?;
    {
        let mut session = state
            .project
            .lock()
            .map_err(|_| "project state lock failed")?;
        session.root = Some(root.clone());
        session.project_file = Some(project_file.clone());
    }
    check_project(state)
}

#[tauri::command]
fn check_project(state: State<'_, AppState>) -> Result<ProjectState, String> {
    let root = {
        let session = state
            .project
            .lock()
            .map_err(|_| "project state lock failed")?;
        session
            .root
            .clone()
            .ok_or_else(|| "no project open".to_string())?
    };
    Ok(ProjectState {
        root: root.display().to_string(),
        files: list_source_files(&root),
        entries: list_project_entries(&root),
        diagnostics: Vec::new(),
    })
}

#[tauri::command]
fn read_file(path: String) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| err.to_string())
}

#[tauri::command]
fn write_file(path: String, content: String) -> Result<(), String> {
    fs::write(path, content).map_err(|err| err.to_string())
}

#[tauri::command]
fn rename_path(
    path: String,
    new_name: String,
    state: State<'_, AppState>,
) -> Result<FileOperationState, String> {
    let root = project_root(&state)?;
    validate_file_name(&new_name)?;
    let old_path = ensure_inside_root(&root, PathBuf::from(path))?;
    let new_path = old_path
        .parent()
        .ok_or_else(|| "path has no parent".to_string())?
        .join(new_name);
    let new_path = ensure_inside_root(&root, new_path)?;
    if new_path.exists() {
        return Err("target path already exists".to_string());
    }
    fs::rename(&old_path, &new_path).map_err(|err| err.to_string())?;
    update_active_sequence_after_move(&state, &old_path, &new_path)?;
    Ok(FileOperationState {
        project: check_project(state)?,
        moved: vec![FileMoveDto {
            old_path: old_path.display().to_string(),
            new_path: new_path.display().to_string(),
        }],
    })
}

#[tauri::command]
fn move_paths(
    paths: Vec<String>,
    new_parent: String,
    state: State<'_, AppState>,
) -> Result<FileOperationState, String> {
    let root = project_root(&state)?;
    let new_parent = ensure_inside_root(&root, PathBuf::from(new_parent))?;
    if !new_parent.is_dir() {
        return Err("drop target is not a directory".to_string());
    }

    let mut moved = Vec::new();
    for path in paths {
        let old_path = ensure_inside_root(&root, PathBuf::from(path))?;
        let name = old_path
            .file_name()
            .ok_or_else(|| "path has no file name".to_string())?;
        let new_path = ensure_inside_root(&root, new_parent.join(name))?;
        if old_path == new_path {
            continue;
        }
        if old_path.is_dir() && new_path.starts_with(&old_path) {
            return Err("cannot move a directory into itself".to_string());
        }
        if new_path.exists() {
            return Err(format!("target already exists: {}", new_path.display()));
        }
        fs::rename(&old_path, &new_path).map_err(|err| err.to_string())?;
        update_active_sequence_after_move(&state, &old_path, &new_path)?;
        moved.push(FileMoveDto {
            old_path: old_path.display().to_string(),
            new_path: new_path.display().to_string(),
        });
    }

    Ok(FileOperationState {
        project: check_project(state)?,
        moved,
    })
}

#[tauri::command]
fn open_sequence(path: String, state: State<'_, AppState>) -> Result<(), String> {
    let mut session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    session.active_sequence = Some(PathBuf::from(path));
    Ok(())
}

#[tauri::command]
fn render_frame(_time: f64, state: State<'_, AppState>) -> Result<FrameSummary, String> {
    {
        let session = state
            .project
            .lock()
            .map_err(|_| "project state lock failed")?;
        session
            .active_sequence
            .clone()
            .ok_or_else(|| "no sequence open".to_string())?
    };
    Ok(FrameSummary {
        pixels: 0,
        fixture_spans: 0,
        warnings: Some(Vec::new()),
    })
}

#[tauri::command]
fn play() {}

#[tauri::command]
fn pause() {}

#[tauri::command]
fn seek(_time: f64) {}

#[tauri::command]
async fn start_language_service(
    project_root: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LanguageServiceState, String> {
    stop_language_service_inner(&state, Some(&app)).await;

    let project_root = PathBuf::from(project_root);
    let process_config = resolve_lsp_process_config(project_root.clone()).map_err(|message| {
        emit_language_service_status(&app, LanguageServiceStatus::Failed, Some(message.clone()));
        message
    })?;
    emit_language_service_status(&app, LanguageServiceStatus::Starting, None);

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|err| err.to_string())?;
    let addr = listener.local_addr().map_err(|err| err.to_string())?;
    let url = format!("ws://{addr}");
    let (stop, stop_rx) = watch::channel(false);
    let connection_tasks = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let accept_task = tokio::spawn(accept_lsp_connections(
        listener,
        app.clone(),
        process_config,
        stop_rx,
        Arc::clone(&connection_tasks),
    ));

    let mut service = state
        .language_service
        .lock()
        .map_err(|_| "language service lock failed")?;
    *service = Some(LanguageService {
        stop,
        accept_task,
        connection_tasks,
    });

    Ok(LanguageServiceState {
        url,
        status: LanguageServiceStatus::Starting,
    })
}

#[tauri::command]
async fn stop_language_service(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    stop_language_service_inner(&state, Some(&app)).await;
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_project,
            check_project,
            read_file,
            write_file,
            rename_path,
            move_paths,
            open_sequence,
            render_frame,
            play,
            pause,
            seek,
            start_language_service,
            stop_language_service
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Dawn");
}

fn list_source_files(root: &Path) -> Vec<String> {
    let mut files = WalkDir::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext == "dawn")
        })
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn list_project_entries(root: &Path) -> Vec<ProjectEntryDto> {
    let mut entries = WalkDir::new(root)
        .min_depth(1)
        .into_iter()
        .filter_map(Result::ok)
        .map(|entry| ProjectEntryDto {
            path: entry.path().display().to_string(),
            kind: if entry.file_type().is_dir() {
                "directory"
            } else {
                "file"
            }
            .to_string(),
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

fn project_root(state: &State<'_, AppState>) -> Result<PathBuf, String> {
    let session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    session
        .root
        .clone()
        .ok_or_else(|| "no project open".to_string())
}

fn ensure_inside_root(root: &Path, path: PathBuf) -> Result<PathBuf, String> {
    let canonical_root = root.canonicalize().map_err(|err| err.to_string())?;
    let canonical_path = if path.exists() {
        path.canonicalize().map_err(|err| err.to_string())?
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| "path has no parent".to_string())?;
        let canonical_parent = parent.canonicalize().map_err(|err| err.to_string())?;
        canonical_parent.join(
            path.file_name()
                .ok_or_else(|| "path has no file name".to_string())?,
        )
    };
    if canonical_path.starts_with(&canonical_root) {
        Ok(canonical_path)
    } else {
        Err("path is outside the open project".to_string())
    }
}

fn validate_file_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("name cannot contain path separators".to_string());
    }
    Ok(())
}

fn update_active_sequence_after_move(
    state: &State<'_, AppState>,
    old_path: &Path,
    new_path: &Path,
) -> Result<(), String> {
    let mut session = state
        .project
        .lock()
        .map_err(|_| "project state lock failed")?;
    if session.active_sequence.as_deref() == Some(old_path) {
        session.active_sequence = Some(new_path.to_path_buf());
    }
    Ok(())
}

async fn stop_language_service_inner(state: &State<'_, AppState>, app: Option<&AppHandle>) {
    let service = state
        .language_service
        .lock()
        .ok()
        .and_then(|mut service| service.take());
    if let Some(service) = service {
        let _ = service.stop.send(true);
        service.accept_task.abort();
        let mut tasks = service.connection_tasks.lock().await;
        for task in tasks.drain(..) {
            task.abort();
        }
        if let Some(app) = app {
            emit_language_service_status(app, LanguageServiceStatus::Disconnected, None);
        }
    }
}

async fn accept_lsp_connections(
    listener: TcpListener,
    app: AppHandle,
    process_config: LspProcessConfig,
    mut stop: watch::Receiver<bool>,
    connection_tasks: Arc<tokio::sync::Mutex<Vec<JoinHandle<()>>>>,
) {
    let active_connection: Arc<tokio::sync::Mutex<Option<tokio::task::AbortHandle>>> =
        Arc::new(tokio::sync::Mutex::new(None));
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    continue;
                };
                if let Some(previous) = active_connection.lock().await.take() {
                    previous.abort();
                }
                let task = tokio::spawn(handle_lsp_connection(
                    stream,
                    app.clone(),
                    process_config.clone(),
                    stop.clone(),
                ));
                *active_connection.lock().await = Some(task.abort_handle());
                connection_tasks.lock().await.push(task);
            }
        }
    }
}

async fn handle_lsp_connection(
    stream: TcpStream,
    app: AppHandle,
    process_config: LspProcessConfig,
    mut stop: watch::Receiver<bool>,
) {
    let Ok(socket) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    let Ok(mut child) = spawn_dawn_lsp(&process_config) else {
        emit_language_service_status(
            &app,
            LanguageServiceStatus::Failed,
            Some("failed to start dawn-lsp".to_string()),
        );
        return;
    };
    emit_language_service_status(&app, LanguageServiceStatus::Ready, None);
    let Some(mut child_stdin) = child.stdin.take() else {
        let _ = child.kill().await;
        emit_language_service_status(
            &app,
            LanguageServiceStatus::Failed,
            Some("dawn-lsp stdin was unavailable".to_string()),
        );
        return;
    };
    let Some(mut child_stdout) = child.stdout.take() else {
        let _ = child.kill().await;
        emit_language_service_status(
            &app,
            LanguageServiceStatus::Failed,
            Some("dawn-lsp stdout was unavailable".to_string()),
        );
        return;
    };

    let (mut ws_writer, mut ws_reader) = socket.split();
    let mut child_exited = Box::pin(child.wait());

    loop {
        tokio::select! {
            _ = stop.changed() => break,
            status = &mut child_exited => {
                let _ = status;
                break;
            }
            message = ws_reader.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        if write_lsp_message(&mut child_stdin, text.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if write_lsp_message(&mut child_stdin, &bytes).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
            read = read_lsp_message(&mut child_stdout) => {
                match read {
                    Ok(Some(body)) => {
                        let Ok(text) = String::from_utf8(body) else {
                            break;
                        };
                        if ws_writer.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(None) | Err(_) => break,
                }
            }
        }
    }

    drop(child_exited);
    let _ = child_stdin.shutdown().await;
    let _ = child.kill().await;
    emit_language_service_status(&app, LanguageServiceStatus::Disconnected, None);
}

fn spawn_dawn_lsp(config: &LspProcessConfig) -> io::Result<tokio::process::Child> {
    let mut command = Command::new(&config.binary);
    command
        .current_dir(&config.project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
}

fn resolve_lsp_process_config(project_root: PathBuf) -> Result<LspProcessConfig, String> {
    let binary = std::env::var_os("DAWN_LSP_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(default_dev_lsp_binary);
    if !binary.is_file() {
        return Err(format!(
            "Dawn language server binary was not found at {}. Set DAWN_LSP_PATH or build dawn-lsp first.",
            binary.display()
        ));
    }
    Ok(LspProcessConfig {
        binary,
        project_root,
    })
}

fn default_dev_lsp_binary() -> PathBuf {
    let exe = if cfg!(windows) {
        "dawn-lsp.exe"
    } else {
        "dawn-lsp"
    };
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join(exe)
}

fn emit_language_service_status(
    app: &AppHandle,
    status: LanguageServiceStatus,
    message: Option<String>,
) {
    let _ = app.emit(
        "language-service/status",
        LanguageServiceStatusEvent { status, message },
    );
}

pub async fn write_lsp_message(
    writer: &mut (impl AsyncWrite + Unpin),
    body: &[u8],
) -> io::Result<()> {
    writer
        .write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes())
        .await?;
    writer.write_all(body).await?;
    writer.flush().await
}

pub async fn read_lsp_message(
    reader: &mut (impl AsyncRead + Unpin),
) -> io::Result<Option<Vec<u8>>> {
    let mut header = Vec::new();
    let mut byte = [0; 1];
    while !header.ends_with(b"\r\n\r\n") {
        let read = reader.read(&mut byte).await?;
        if read == 0 {
            return if header.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete LSP header",
                ))
            };
        }
        header.push(byte[0]);
    }

    let header = String::from_utf8_lossy(&header);
    let content_length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing Content-Length"))?;

    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).await?;
    Ok(Some(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn lsp_message_round_trips_body_with_content_length_framing() {
        let body = br#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#;
        let (mut writer, mut reader) = tokio::io::duplex(1024);

        write_lsp_message(&mut writer, body).await.unwrap();
        let read = read_lsp_message(&mut reader).await.unwrap().unwrap();

        assert_eq!(read, body);
    }

    #[tokio::test]
    async fn lsp_message_reader_accepts_case_insensitive_content_length() {
        let mut input = &b"content-length: 2\r\n\r\n{}"[..];
        let read = read_lsp_message(&mut input).await.unwrap().unwrap();

        assert_eq!(read, b"{}");
    }

    #[tokio::test]
    async fn lsp_message_reader_rejects_incomplete_headers() {
        let mut input = &b"Content-Length: 2\r\n"[..];
        let error = read_lsp_message(&mut input).await.unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
}
