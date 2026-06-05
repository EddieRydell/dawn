use std::sync::{Arc, Mutex};

use dawn_backend::{AppBackend, AppUpdate, BackendTask};
use tauri::AppHandle;

use crate::{events, state::CommandResult};

pub(crate) fn handle_backend_update(
    app: &AppHandle,
    backend: Arc<Mutex<AppBackend>>,
    update: AppUpdate,
) -> CommandResult<()> {
    events::emit_app_view(app, update.view)?;
    submit_backend_tasks(app.clone(), backend, update.tasks);
    Ok(())
}

fn submit_backend_tasks(app: AppHandle, backend: Arc<Mutex<AppBackend>>, tasks: Vec<BackendTask>) {
    for task in tasks {
        spawn_backend_task(app.clone(), Arc::clone(&backend), task);
    }
}

fn spawn_backend_task(app: AppHandle, backend: Arc<Mutex<AppBackend>>, task: BackendTask) {
    tauri::async_runtime::spawn_blocking(move || {
        let output = match task.run() {
            Ok(output) => output,
            Err(error) => {
                events::emit_backend_error(&app, error.to_string());
                return;
            }
        };

        let update = {
            let mut backend = match backend.lock() {
                Ok(backend) => backend,
                Err(_) => {
                    events::emit_backend_error(&app, "backend lock is poisoned".to_string());
                    return;
                }
            };
            match backend.complete_task(output) {
                Ok(update) => update,
                Err(error) => {
                    events::emit_backend_error(&app, error.to_string());
                    return;
                }
            }
        };

        if let Err(error) = events::emit_app_view(&app, update.view) {
            events::emit_backend_error(&app, error);
        }
        submit_backend_tasks(app, backend, update.tasks);
    });
}
