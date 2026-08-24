use crate::dto::{AppSettings, WorkspaceLayoutState};

pub(crate) fn sanitize_workspace_layout(state: WorkspaceLayoutState) -> WorkspaceLayoutState {
    WorkspaceLayoutState {
        sidebar_width_px: clamp_f64(state.sidebar_width_px, 220.0, 520.0),
        inspector_width_px: clamp_f64(state.inspector_width_px, 240.0, 560.0),
        sidebar_collapsed: state.sidebar_collapsed,
        inspector_collapsed: state.inspector_collapsed,
        active_sidebar_view: state.active_sidebar_view,
    }
}

fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    if !value.is_finite() {
        return min;
    }
    value.clamp(min, max)
}

pub(crate) fn sanitize_app_settings(mut settings: AppSettings) -> AppSettings {
    if !settings.sequence_initial_px_per_second.is_finite() {
        settings.sequence_initial_px_per_second = 80.0;
    }
    if !settings.sequence_initial_lane_height_px.is_finite() {
        settings.sequence_initial_lane_height_px = 42.0;
    }
    if !settings.effect_raster.render_scale.is_finite() {
        settings.effect_raster.render_scale = 1.0;
    }
    settings.sequence_initial_px_per_second =
        settings.sequence_initial_px_per_second.clamp(20.0, 12000.0);
    settings.sequence_initial_lane_height_px =
        settings.sequence_initial_lane_height_px.clamp(24.0, 120.0);
    settings.effect_raster.render_scale = settings.effect_raster.render_scale.clamp(0.25, 2.0);
    settings.effect_raster.max_columns = settings.effect_raster.max_columns.clamp(16, 1024);
    settings.effect_raster.max_rows = settings.effect_raster.max_rows.clamp(1, 200);
    settings.effect_raster.min_frame_stride = settings.effect_raster.min_frame_stride.clamp(1, 16);
    settings
}
