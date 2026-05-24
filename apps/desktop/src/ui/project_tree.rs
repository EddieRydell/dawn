use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

use dawn_project::path::ProjectPath;
use floem::event::EventListener;
use floem::menu::{Menu, MenuItem};
use floem::prelude::*;
use floem::views::Button;
use lucide_floem::Icon;

use crate::actions::AppAction;
use crate::app_model::AppSnapshot;
use crate::workspace::{ProjectEntry, ProjectEntryKind};

#[derive(Clone)]
pub struct ExplorerUiState {
    pub expanded: RwSignal<HashSet<String>>,
    pub selected: RwSignal<Option<ProjectPath>>,
    pub pending: RwSignal<Option<PendingExplorerEdit>>,
    pub pending_name: RwSignal<String>,
    pub drag_source: RwSignal<Option<ProjectPath>>,
    pub revealed_active_file: RwSignal<Option<ProjectPath>>,
    pub root: RwSignal<Option<String>>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum PendingExplorerEdit {
    CreateFile(ProjectPath),
    CreateDirectory(ProjectPath),
    Rename(ProjectPath),
}

impl ExplorerUiState {
    pub fn new() -> Self {
        let mut expanded = HashSet::new();
        expanded.insert(String::new());
        Self {
            expanded: RwSignal::new(expanded),
            selected: RwSignal::new(None),
            pending: RwSignal::new(None),
            pending_name: RwSignal::new(String::new()),
            drag_source: RwSignal::new(None),
            revealed_active_file: RwSignal::new(None),
            root: RwSignal::new(None),
        }
    }

    pub fn reset_for_root(&self, root: Option<String>) {
        if self.root.get() == root {
            return;
        }
        let mut expanded = HashSet::new();
        expanded.insert(String::new());
        self.root.set(root);
        self.expanded.set(expanded);
        self.selected.set(None);
        self.pending.set(None);
        self.pending_name.set(String::new());
        self.drag_source.set(None);
        self.revealed_active_file.set(None);
    }

    pub fn reveal(&self, path: &ProjectPath) {
        let mut ancestors = ancestor_paths(path);
        ancestors.insert(String::new());
        self.expanded.update(|expanded| {
            expanded.extend(ancestors);
        });
        self.selected.set(Some(path.clone()));
    }

    pub fn reveal_active_file(&self, path: &ProjectPath) {
        if self.revealed_active_file.get().as_ref() == Some(path) {
            return;
        }

        self.reveal(path);
        self.revealed_active_file.set(Some(path.clone()));
    }
}

pub fn project_tree_view(
    state: AppSnapshot,
    explorer: ExplorerUiState,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    let Some(project) = state.project.clone() else {
        return v_stack((header("Project"), static_label("No project open")))
            .style(panel_style)
            .into_any();
    };

    if let Some(active_file) = state.active_file.as_ref() {
        explorer.reveal_active_file(active_file);
    }

    let root = project.root.clone();
    let entries = project.entries.clone();
    let rows_explorer = explorer.clone();
    let rows_dispatch = Rc::clone(&dispatch);

    v_stack((
        header("Project"),
        scroll(
            dyn_container(
                {
                    let explorer = explorer.clone();
                    move || {
                        let expanded = explorer.expanded.get();
                        ExplorerRenderState {
                            rows: visible_rows(&root, &entries, &expanded),
                            expanded,
                            selected: explorer.selected.get(),
                            pending: explorer.pending.get(),
                        }
                    }
                },
                move |render| {
                    let expanded = render.expanded;
                    let selected = render.selected;
                    let pending = render.pending;
                    v_stack_from_iter(render.rows.into_iter().map({
                        let explorer = rows_explorer.clone();
                        let dispatch = Rc::clone(&rows_dispatch);
                        move |row| {
                            explorer_row(
                                row,
                                explorer.clone(),
                                Rc::clone(&dispatch),
                                &expanded,
                                selected.as_ref(),
                                pending.as_ref(),
                            )
                            .into_any()
                        }
                    }))
                    .style(|s| s.width_full())
                },
            )
            .style(|s| s.width_full()),
        )
        .style(|s| s.flex_grow(1.0).min_height(0.0)),
    ))
    .style(panel_style)
    .into_any()
}

fn explorer_row(
    row: ExplorerRow,
    explorer: ExplorerUiState,
    dispatch: crate::ui::UiDispatch,
    expanded: &HashSet<String>,
    selected_path: Option<&ProjectPath>,
    pending: Option<&PendingExplorerEdit>,
) -> impl IntoView {
    let path = row.path.clone();
    let row_key = path.to_slash_string();
    let is_dir = row.kind == ProjectEntryKind::Directory;
    let is_expanded = expanded.contains(&row_key);
    let selected = selected_path.is_some_and(|selected| selected == &path);

    if let Some(PendingExplorerEdit::Rename(rename_path)) = pending {
        if rename_path == &path {
            return edit_name_row(row, explorer, dispatch).into_any();
        }
    }

    let open_state = explorer.clone();
    let row_dispatch = Rc::clone(&dispatch);
    let drag_state = explorer.clone();
    let drop_state = explorer.clone();
    let context_state = explorer.clone();
    let path_for_click = path.clone();
    let path_for_drag = path.clone();
    let path_for_drop = path.clone();
    let path_for_menu = path.clone();
    let drop_dispatch = Rc::clone(&dispatch);
    let menu_dispatch = Rc::clone(&dispatch);

    let row_view = h_stack((
        caret_view(is_dir, is_expanded),
        file_icon(row.kind, is_expanded).style(|s| s.size(15.0, 15.0)),
        label(move || row.name.clone()).style(|s| s.flex_grow(1.0).min_width(0.0)),
    ))
    .on_click_stop(move |_| {
        open_state.selected.set(Some(path_for_click.clone()));
        if is_dir {
            toggle_expanded(&open_state.expanded, &path_for_click.to_slash_string());
        } else {
            row_dispatch(AppAction::OpenFile(path_for_click.clone()));
        }
    })
    .on_event_stop(EventListener::DragStart, move |_| {
        drag_state.drag_source.set(Some(path_for_drag.clone()));
    })
    .on_event_stop(EventListener::Drop, move |_| {
        if is_dir {
            if let Some(source) = drop_state.drag_source.get() {
                if source != path_for_drop && !path_for_drop.starts_with(&source) {
                    drop_dispatch(AppAction::MovePaths {
                        paths: vec![source],
                        new_parent: path_for_drop.clone(),
                    });
                    drop_state.drag_source.set(None);
                }
            }
        }
    })
    .draggable()
    .context_menu(move || {
        row_menu(
            path_for_menu.clone(),
            is_dir,
            context_state.clone(),
            Rc::clone(&menu_dispatch),
        )
    })
    .style(move |s| {
        let bg = if selected {
            crate::ui::theme::color(crate::ui::theme::SELECTED)
        } else {
            crate::ui::theme::color(crate::ui::theme::SURFACE)
        };
        s.width_full()
            .height(26.0)
            .items_center()
            .gap(5.0)
            .padding_left(6.0 + row.depth as f32 * 14.0)
            .padding_right(4.0)
            .background(bg)
    });

    if let Some(PendingExplorerEdit::CreateFile(parent)) = pending {
        if parent == &path {
            return v_stack((
                row_view,
                create_row(parent.clone(), true, explorer, dispatch),
            ))
            .style(|s| s.width_full())
            .into_any();
        }
    }
    if let Some(PendingExplorerEdit::CreateDirectory(parent)) = pending {
        if parent == &path {
            return v_stack((
                row_view,
                create_row(parent.clone(), false, explorer, dispatch),
            ))
            .style(|s| s.width_full())
            .into_any();
        }
    }

    row_view.into_any()
}

fn create_row(
    parent: ProjectPath,
    is_file: bool,
    explorer: ExplorerUiState,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    edit_row(
        "Name",
        explorer.pending_name,
        {
            let explorer = explorer.clone();
            move || {
                let name = explorer.pending_name.get();
                if is_file {
                    dispatch(AppAction::CreateFile {
                        parent: parent.clone(),
                        name,
                    });
                } else {
                    dispatch(AppAction::CreateDirectory {
                        parent: parent.clone(),
                        name,
                    });
                }
                explorer.pending.set(None);
            }
        },
        {
            let explorer = explorer.clone();
            move || explorer.pending.set(None)
        },
    )
    .style(|s| s.padding_left(24.0))
}

fn edit_name_row(
    row: ExplorerRow,
    explorer: ExplorerUiState,
    dispatch: crate::ui::UiDispatch,
) -> impl IntoView {
    edit_row(
        "Rename",
        explorer.pending_name,
        {
            let explorer = explorer.clone();
            let path = row.path.clone();
            move || {
                dispatch(AppAction::RenamePath {
                    path: path.clone(),
                    new_name: explorer.pending_name.get(),
                });
                explorer.pending.set(None);
            }
        },
        {
            let explorer = explorer.clone();
            move || explorer.pending.set(None)
        },
    )
    .style(move |s| s.padding_left(6.0 + row.depth as f32 * 14.0))
}

fn edit_row(
    placeholder: &'static str,
    name: RwSignal<String>,
    apply: impl Fn() + 'static,
    cancel: impl Fn() + 'static,
) -> impl IntoView {
    h_stack((
        text_input(name)
            .placeholder(placeholder)
            .style(|s| s.flex_grow(1.0).height(24.0)),
        icon_button(Icon::Check, "Apply").action(apply),
        icon_button(Icon::X, "Cancel").action(cancel),
    ))
    .style(|s| s.width_full().height(28.0).items_center().gap(4.0))
}

fn row_menu(
    path: ProjectPath,
    is_dir: bool,
    explorer: ExplorerUiState,
    dispatch: crate::ui::UiDispatch,
) -> Menu {
    let create_parent = if is_dir {
        path.clone()
    } else {
        path.parent().unwrap_or_else(ProjectPath::root)
    };
    let file_state = explorer.clone();
    let file_parent = create_parent.clone();
    let folder_state = explorer.clone();
    let folder_parent = create_parent;
    let can_modify = !path.is_root();
    let rename_state = explorer.clone();
    let rename_path = path.clone();
    let delete_path = path.clone();
    let delete_dispatch = Rc::clone(&dispatch);

    Menu::new("Project")
        .entry(MenuItem::new("Add File").action(move || {
            file_state
                .pending
                .set(Some(PendingExplorerEdit::CreateFile(file_parent.clone())));
            file_state.pending_name.set("untitled".to_string());
        }))
        .entry(MenuItem::new("Add Directory").action(move || {
            folder_state
                .pending
                .set(Some(PendingExplorerEdit::CreateDirectory(
                    folder_parent.clone(),
                )));
            folder_state.pending_name.set("folder".to_string());
        }))
        .separator()
        .entry(
            MenuItem::new("Rename")
                .enabled(can_modify)
                .action(move || begin_rename(&rename_state, rename_path.clone())),
        )
        .entry(
            MenuItem::new("Remove")
                .enabled(can_modify)
                .action(move || delete_dispatch(AppAction::DeletePath(delete_path.clone()))),
        )
}

fn begin_rename(explorer: &ExplorerUiState, path: ProjectPath) {
    explorer.pending_name.set(
        path.file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default(),
    );
    explorer
        .pending
        .set(Some(PendingExplorerEdit::Rename(path)));
}

fn visible_rows(
    root: &str,
    entries: &[ProjectEntry],
    expanded: &HashSet<String>,
) -> Vec<ExplorerRow> {
    let mut rows = vec![ExplorerRow {
        path: ProjectPath::root(),
        name: root.rsplit('/').next().unwrap_or(root).to_string(),
        kind: ProjectEntryKind::Directory,
        depth: 0,
    }];

    append_visible_children(&ProjectPath::root(), 1, entries, expanded, &mut rows);
    rows
}

fn append_visible_children(
    parent: &ProjectPath,
    depth: usize,
    entries: &[ProjectEntry],
    expanded: &HashSet<String>,
    rows: &mut Vec<ExplorerRow>,
) {
    if !expanded.contains(&parent.to_slash_string()) {
        return;
    }

    let mut children = entries
        .iter()
        .filter_map(|entry| {
            let path = ProjectPath::parse(&entry.path).ok()?;
            if path.parent().as_ref() == Some(parent) {
                Some((path, entry))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    children.sort_by(|(left_path, left), (right_path, right)| {
        let left_dir = left.kind == ProjectEntryKind::Directory;
        let right_dir = right.kind == ProjectEntryKind::Directory;
        (!left_dir, left_path.file_name()).cmp(&(!right_dir, right_path.file_name()))
    });

    for (path, entry) in children {
        rows.push(ExplorerRow {
            name: path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.path.clone()),
            depth,
            path: path.clone(),
            kind: entry.kind,
        });
        if entry.kind == ProjectEntryKind::Directory {
            append_visible_children(&path, depth + 1, entries, expanded, rows);
        }
    }
}

fn ancestor_paths(path: &ProjectPath) -> BTreeSet<String> {
    let mut ancestors = BTreeSet::new();
    let mut current = path.parent();
    while let Some(parent) = current {
        ancestors.insert(parent.to_slash_string());
        current = parent.parent();
    }
    ancestors
}

fn toggle_expanded(expanded: &RwSignal<HashSet<String>>, path: &str) {
    expanded.update(|expanded| {
        if !expanded.insert(path.to_string()) {
            expanded.remove(path);
        }
    });
}

struct ExplorerRenderState {
    rows: Vec<ExplorerRow>,
    expanded: HashSet<String>,
    selected: Option<ProjectPath>,
    pending: Option<PendingExplorerEdit>,
}

fn caret_view(is_dir: bool, expanded: bool) -> floem::AnyView {
    if !is_dir {
        return empty().style(|s| s.size(18.0, 18.0)).into_any();
    }

    let icon = if expanded {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    icon.style(|s| s.size(15.0, 15.0)).into_any()
}

fn file_icon(kind: ProjectEntryKind, expanded: bool) -> impl IntoView {
    match (kind, expanded) {
        (ProjectEntryKind::Directory, true) => Icon::FolderOpen.into_any(),
        (ProjectEntryKind::Directory, false) => Icon::Folder.into_any(),
        (ProjectEntryKind::File, _) => Icon::File.into_any(),
    }
}

fn icon_button(icon: Icon, _title: &'static str) -> Button {
    button(icon.style(|s| s.size(14.0, 14.0))).style(|s| s.size(24.0, 22.0).padding(3.0))
}

fn header(text: &'static str) -> impl IntoView {
    static_label(text).style(|s| {
        s.height(24.0)
            .font_size(12.0)
            .font_bold()
            .color(crate::ui::theme::color(crate::ui::theme::MUTED))
    })
}

fn panel_style(s: floem::style::Style) -> floem::style::Style {
    s.height_full()
        .padding(8.0)
        .gap(6.0)
        .background(crate::ui::theme::color(crate::ui::theme::SURFACE))
}

#[derive(Clone)]
struct ExplorerRow {
    path: ProjectPath,
    name: String,
    kind: ProjectEntryKind,
    depth: usize,
}
