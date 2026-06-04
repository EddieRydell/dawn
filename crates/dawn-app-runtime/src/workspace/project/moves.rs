use std::collections::HashSet;

use dawn_language::fs::WorkspaceFs;
use dawn_language::path::{PathStringExt, Utf8PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PlannedMove {
    pub(super) old_path: Utf8PathBuf,
    pub(super) new_path: Utf8PathBuf,
}

pub(super) fn plan_moves(
    fs: &WorkspaceFs,
    paths: Vec<Utf8PathBuf>,
    new_parent: Utf8PathBuf,
) -> Result<Vec<PlannedMove>, String> {
    if !fs.is_dir(&new_parent) {
        return Err("drop target is not a directory".to_string());
    }

    let mut selected_paths = Vec::new();
    let mut seen_sources = HashSet::new();
    for old_path in paths {
        if !seen_sources.insert(old_path.clone()) {
            return Err(format!(
                "duplicate source path: {}",
                old_path.to_slash_string()
            ));
        }
        selected_paths.push(old_path);
    }
    reject_nested_selected_paths(&selected_paths)?;

    let mut planned_moves = Vec::new();
    let mut seen_destinations = HashSet::new();
    for old_path in selected_paths {
        let name = old_path
            .file_name()
            .ok_or_else(|| "path has no file name".to_string())?;
        let new_path = new_parent.join(name);
        if old_path == new_path {
            continue;
        }
        if fs.is_dir(&old_path) && new_path.starts_with(&old_path) {
            return Err("cannot move a directory into itself".to_string());
        }
        if !seen_destinations.insert(new_path.clone()) {
            return Err(format!(
                "duplicate destination path: {}",
                new_path.to_slash_string()
            ));
        }
        if fs.exists(&new_path) {
            return Err(format!(
                "target already exists: {}",
                new_path.to_slash_string()
            ));
        }
        planned_moves.push(PlannedMove { old_path, new_path });
    }

    Ok(planned_moves)
}

pub(super) fn apply_planned_moves(
    fs: &WorkspaceFs,
    planned_moves: &[PlannedMove],
) -> Result<(), String> {
    let mut completed = Vec::new();
    for planned_move in planned_moves {
        if let Err(error) = fs.rename(&planned_move.old_path, &planned_move.new_path) {
            let rollback_error = rollback_completed_moves(fs, &completed);
            return Err(match rollback_error {
                Ok(()) => error.to_string(),
                Err(rollback_error) => format!("{}; rollback failed: {}", error, rollback_error),
            });
        }
        completed.push(planned_move.clone());
    }
    Ok(())
}

pub(super) fn project_path_moves_from_plan(
    planned_moves: &[PlannedMove],
) -> Vec<(Utf8PathBuf, Utf8PathBuf)> {
    planned_moves
        .iter()
        .map(|planned_move| (planned_move.old_path.clone(), planned_move.new_path.clone()))
        .collect()
}

pub(super) fn update_active_sequence_after_moves(
    active_sequence: &mut Option<Utf8PathBuf>,
    planned_moves: &[PlannedMove],
) {
    if let Some(sequence) = active_sequence.as_ref() {
        for planned_move in planned_moves {
            if let Some(new_sequence) =
                moved_path(sequence, &planned_move.old_path, &planned_move.new_path)
            {
                *active_sequence = Some(new_sequence);
                return;
            }
        }
    }
}

fn reject_nested_selected_paths(paths: &[Utf8PathBuf]) -> Result<(), String> {
    for (left_index, left) in paths.iter().enumerate() {
        for right in paths.iter().skip(left_index + 1) {
            if left.starts_with(right) || right.starts_with(left) {
                return Err(format!(
                    "cannot move nested selected paths together: {} and {}",
                    left.to_slash_string(),
                    right.to_slash_string()
                ));
            }
        }
    }
    Ok(())
}

fn rollback_completed_moves(fs: &WorkspaceFs, completed: &[PlannedMove]) -> Result<(), String> {
    let mut errors = Vec::new();
    for completed_move in completed.iter().rev() {
        if let Err(error) = fs.rename(&completed_move.new_path, &completed_move.old_path) {
            errors.push(format!(
                "{} -> {}: {}",
                completed_move.new_path.to_slash_string(),
                completed_move.old_path.to_slash_string(),
                error
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

fn moved_path(
    path: &Utf8PathBuf,
    old_path: &Utf8PathBuf,
    new_path: &Utf8PathBuf,
) -> Option<Utf8PathBuf> {
    if path == old_path {
        return Some(new_path.clone());
    }
    if !path.starts_with(old_path) {
        return None;
    }
    let relative = path.strip_prefix(old_path).ok()?;
    Some(new_path.join(relative))
}
