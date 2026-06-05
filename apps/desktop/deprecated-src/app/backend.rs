use deprecated_dawn_backend::PreviewSnapshot;
use tauri::{AppHandle, Emitter};

use crate::app::state::CommandResult;
use crate::dto::{AppBackendChangedDto, AppSnapshotDto, BackendSliceDto};
use crate::preview::{PreviewStateEventDto, PreviewTimingDto};

pub(crate) fn emit_app_snapshot(
    app: &AppHandle,
    snapshot: AppSnapshotDto,
) -> CommandResult<AppSnapshotDto> {
    app.emit(
        "app_backend_changed",
        AppBackendChangedDto {
            snapshot: snapshot.clone(),
            changed_slices: BackendSliceDto::all(),
        },
    )
    .map_err(|error| error.to_string())?;
    emit_preview_state_dto(app, &snapshot)?;
    Ok(snapshot)
}

pub(crate) fn emit_preview_state_dto(
    app: &AppHandle,
    snapshot: &AppSnapshotDto,
) -> CommandResult<()> {
    let snapshot = &snapshot.preview.preview;
    app.emit(
        "preview_state_changed",
        PreviewStateEventDto {
            source_label: snapshot.source_label.clone(),
            is_playing: snapshot.is_playing,
            preview_updating: snapshot.preview_updating,
            effect_preview_active: snapshot.effect_preview_active,
            position_seconds: snapshot.position_seconds,
            home_seconds: snapshot.home_seconds,
            duration_seconds: snapshot.duration_seconds,
            audio: snapshot.audio.clone(),
            clock_source: snapshot.clock_source.clone(),
            audio_playback_status: snapshot.audio_playback_status,
            status: snapshot.status.clone(),
            timing: PreviewTimingDto::empty(0.0),
        },
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn emit_preview_state_snapshot(
    app: &AppHandle,
    snapshot: &PreviewSnapshot,
    timing: PreviewTimingDto,
) {
    let _ = app.emit(
        "preview_state_changed",
        PreviewStateEventDto {
            source_label: snapshot.source_label.clone(),
            is_playing: snapshot.is_playing,
            preview_updating: snapshot.preview_updating,
            effect_preview_active: snapshot.effect_preview_active,
            position_seconds: snapshot.position_seconds,
            home_seconds: snapshot.home_seconds,
            duration_seconds: snapshot.duration_seconds,
            audio: snapshot.audio.clone().map(Into::into),
            clock_source: snapshot.clock_source.clone(),
            audio_playback_status: snapshot.audio_playback_status,
            status: snapshot.status.clone(),
            timing,
        },
    );
}
