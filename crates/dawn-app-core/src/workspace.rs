use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use dawn_project::{
    canonicalize_path, load_project, save_project, utf8_path, Curve, CurveDefinitionKey, DawnFile,
    DawnObject, DawnProject, EffectDefinitionKey, EffectParam, EffectTarget, Fixture,
    FixtureDefinitionKey, FixtureId, Geometry, GroupInstantiationId, Layout, LayoutDefinitionKey,
    LayoutTargetKind, ObjectKind, PathStringExt, ProjectDiagnostic, ProjectLoadResult,
    ProjectSaveResult, Resolved, ResolvedInlineOrRef, ResolvedSourceFile, ResolvedSourceObject,
    ResolvedSymbolRef, Sequence, SequenceDefinitionKey, SequenceEffect, Utf8PathBuf,
    WorkspaceEntry, WorkspaceEntryKind, WorkspaceFs,
};

use crate::document::{
    geometry_render_plan, geometry_summary, DocumentDescriptor, DocumentObjectDescriptor,
    DocumentViewId, EffectScriptReferenceDocument, FixtureDefinitionDocument, FixtureDocument,
    LayoutDocument, LayoutFixturePlacement, LayoutTargetDocument, ResolvedLayoutFixture,
    SequenceAudioDocument, SequenceCurveLibraryItemDocument, SequenceDocument,
    SequenceEffectDocument, SequenceEffectParamCurveSourceDocument, SequenceEffectParamDocument,
    SequenceEffectPixelDocument, SequenceEffectRenderDocument, SequenceEffectScriptDocument,
    SequenceEffectScriptParamDocument, SequenceLaneDocument, SequenceMarkCollectionDocument,
};
use crate::editor_session::FileDiskVersion;

#[derive(Debug, Default, Clone)]
pub struct WorkspaceService {
    root_path: Option<PathBuf>,
    root_display: Option<String>,
    fs: Option<WorkspaceFs>,
    project_file: Option<Utf8PathBuf>,
    active_sequence: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlannedMove {
    old_path: Utf8PathBuf,
    new_path: Utf8PathBuf,
}

impl WorkspaceService {
    pub fn open_project(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        let (root, project_file) = if path.is_dir() {
            (path.to_path_buf(), Utf8PathBuf::from("project.dawn"))
        } else {
            let file_name = path
                .file_name()
                .ok_or_else(|| "project file has no file name".to_string())?;
            let root = path
                .parent()
                .map(Path::to_path_buf)
                .ok_or_else(|| "project file has no parent".to_string())?;
            (root, utf8_path(PathBuf::from(file_name))?)
        };
        let fs = WorkspaceFs::open(&root).map_err(|error| error.to_string())?;
        self.root_path = Some(root.clone());
        self.root_display = Some(root.to_string_lossy().replace('\\', "/"));
        self.fs = Some(fs);
        self.project_file = Some(project_file);
        self.active_sequence = None;
        Ok(())
    }

    pub fn close_project(&mut self) {
        self.root_display = None;
        self.root_path = None;
        self.fs = None;
        self.project_file = None;
        self.active_sequence = None;
    }

    pub fn project_root_display(&self) -> Option<&str> {
        self.root_display.as_deref()
    }

    pub fn project_entries(&self) -> Result<Vec<WorkspaceEntry>, String> {
        list_project_entries(self.project_fs()?)
    }

    pub fn load_project(&self) -> ProjectLoadResult {
        let fs = match self.project_fs() {
            Ok(fs) => fs,
            Err(error) => {
                return ProjectLoadResult {
                    project: None,
                    diagnostics: vec![diagnostic(Utf8PathBuf::new(), error)],
                };
            }
        };
        let project_file = match self.current_project_file() {
            Ok(path) => path,
            Err(error) => {
                return ProjectLoadResult {
                    project: None,
                    diagnostics: vec![diagnostic(Utf8PathBuf::new(), error)],
                };
            }
        };
        let project_key = match self.root_project_key(&project_file) {
            Ok(key) => key,
            Err(error) => {
                return ProjectLoadResult {
                    project: None,
                    diagnostics: vec![diagnostic(project_file, error)],
                };
            }
        };
        load_project(fs, project_file, &project_key)
    }

    pub fn save_project(&self, project: &DawnProject) -> Result<ProjectSaveResult, String> {
        Ok(save_project(self.project_fs()?, project))
    }

    pub fn inspect_document(
        &self,
        project: &DawnProject,
        path: Utf8PathBuf,
    ) -> Result<DocumentDescriptor, String> {
        let source_path = self.canonical_project_path(&path)?;
        let source = project
            .stores
            .source_files
            .get(&source_path)
            .ok_or_else(|| format!("document `{}` is not part of the loaded project", path))?;
        let mut objects = Vec::new();
        let mut default_object_keys = HashMap::new();
        if let ResolvedSourceFile::Dawn { objects: slots, .. } = source {
            for (key, slot) in slots {
                let kind = source_object_kind(slot);
                objects.push(DocumentObjectDescriptor {
                    key: key.clone(),
                    kind,
                });
                match kind {
                    ObjectKind::Layout => {
                        default_object_keys
                            .entry(DocumentViewId::Layout)
                            .or_insert_with(|| key.clone());
                    }
                    ObjectKind::Fixture => {
                        default_object_keys
                            .entry(DocumentViewId::Fixture)
                            .or_insert_with(|| key.clone());
                    }
                    ObjectKind::Sequence => {
                        default_object_keys
                            .entry(DocumentViewId::Sequence)
                            .or_insert_with(|| key.clone());
                    }
                    _ => {}
                }
            }
        }
        let mut available_views = vec![DocumentViewId::Text];
        for view in [
            DocumentViewId::Layout,
            DocumentViewId::Fixture,
            DocumentViewId::Sequence,
        ] {
            if default_object_keys.contains_key(&view) {
                available_views.push(view);
            }
        }
        Ok(DocumentDescriptor {
            path: path.to_slash_string(),
            objects,
            available_views,
            default_object_keys,
        })
    }

    pub fn sequence_document(
        &self,
        project: &DawnProject,
        path: Utf8PathBuf,
        object_key: &str,
    ) -> Result<SequenceDocument, String> {
        let source_path = self.canonical_project_path(&path)?;
        let key = SequenceDefinitionKey::new(source_path.clone(), object_key.to_string());
        let sequence = project
            .stores
            .sequences
            .get(&key)
            .ok_or_else(|| format!("sequence `{object_key}` was not found"))?;
        Ok(sequence_document(
            project,
            &path,
            object_key,
            &sequence.value,
        ))
    }

    pub fn layout_document(
        &self,
        project: &DawnProject,
        path: Utf8PathBuf,
        object_key: &str,
    ) -> Result<LayoutDocument, String> {
        let source_path = self.canonical_project_path(&path)?;
        let key = LayoutDefinitionKey::new(source_path.clone(), object_key.to_string());
        let layout = project
            .stores
            .layouts
            .get(&key)
            .ok_or_else(|| format!("layout `{object_key}` was not found"))?;
        Ok(layout_document(
            project,
            &source_path,
            object_key,
            &layout.value,
        ))
    }

    pub fn fixture_document(
        &self,
        project: &DawnProject,
        path: Utf8PathBuf,
        selected_object_key: Option<&str>,
    ) -> Result<FixtureDocument, String> {
        let source_path = self.canonical_project_path(&path)?;
        let mut fixtures = Vec::new();
        for (key, fixture) in &project.stores.fixture_definitions {
            if key.path != source_path {
                continue;
            }
            if selected_object_key.is_some_and(|selected| selected != key.name) {
                continue;
            }
            fixtures.push(fixture_definition_document(&key.name, &fixture.value));
        }
        Ok(FixtureDocument {
            path: source_path.to_slash_string(),
            selected_object_key: selected_object_key.map(ToString::to_string),
            fixtures,
        })
    }

    pub fn inspect_fixture_file(
        &self,
        project: &DawnProject,
        selected_file: &Path,
    ) -> Result<(Utf8PathBuf, FixtureDocument), String> {
        let path = self.project_path_for_selected_file(selected_file)?;
        let document = self.fixture_document(project, path.clone(), None)?;
        Ok((path, document))
    }

    pub fn fixture_import_string(
        &self,
        importing_path: &Utf8PathBuf,
        selected_file: &Path,
        object_key: &str,
    ) -> Result<(String, bool), String> {
        let path = self.project_path_for_selected_file(selected_file)?;
        let is_absolute = path.is_absolute();
        let import_path = if is_absolute {
            path.to_slash_string()
        } else {
            serialized_import_path(importing_path, &path)
        };
        Ok((format!("{import_path}::{object_key}"), is_absolute))
    }

    pub fn read_file(&self, path: Utf8PathBuf) -> Result<String, String> {
        self.project_fs()?
            .read_to_string(&path)
            .map_err(|error| error.to_string())
    }

    pub fn read_file_with_version(
        &self,
        path: Utf8PathBuf,
    ) -> Result<(String, FileDiskVersion), String> {
        let text = self.read_file(path.clone())?;
        let version = self
            .file_version(&path, &text)?
            .ok_or_else(|| "file does not exist".to_string())?;
        Ok((text, version))
    }

    pub fn write_file(&self, path: Utf8PathBuf, content: impl AsRef<[u8]>) -> Result<(), String> {
        self.project_fs()?
            .write(&path, content)
            .map_err(|error| error.to_string())
    }

    pub fn write_text_file_with_version(
        &self,
        path: Utf8PathBuf,
        content: &str,
    ) -> Result<FileDiskVersion, String> {
        self.write_file(path.clone(), content.as_bytes())?;
        self.file_version(&path, content)?
            .ok_or_else(|| "written file does not exist".to_string())
    }

    pub fn file_version(
        &self,
        path: &Utf8PathBuf,
        content: &str,
    ) -> Result<Option<FileDiskVersion>, String> {
        let resolved = self.project_fs()?.resolve(path);
        let metadata = match std::fs::metadata(resolved.as_std_path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        if !metadata.is_file() {
            return Ok(None);
        }
        let modified_millis = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_millis());
        Ok(Some(FileDiskVersion {
            len: metadata.len(),
            modified_millis,
            content_hash: content_hash(content),
        }))
    }

    pub fn create_file(&mut self, parent: Utf8PathBuf, name: &str) -> Result<Utf8PathBuf, String> {
        let name = file_name_with_default_extension(name)?;
        validate_file_name(&name)?;
        let fs = self.project_fs()?.clone();
        if !parent.as_str().is_empty() && !fs.is_dir(&parent) {
            return Err("parent path is not a directory".to_string());
        }
        let path = parent.join(&name);
        if fs.exists(&path) {
            return Err("target path already exists".to_string());
        }
        fs.create_file(&path, [])
            .map_err(|error| error.to_string())?;
        Ok(path)
    }

    pub fn create_directory(
        &mut self,
        parent: Utf8PathBuf,
        name: &str,
    ) -> Result<Utf8PathBuf, String> {
        validate_file_name(name)?;
        let fs = self.project_fs()?.clone();
        if !parent.as_str().is_empty() && !fs.is_dir(&parent) {
            return Err("parent path is not a directory".to_string());
        }
        let path = parent.join(name);
        if fs.exists(&path) {
            return Err("target path already exists".to_string());
        }
        fs.create_dir(&path).map_err(|error| error.to_string())?;
        Ok(path)
    }

    pub fn delete_path(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        let fs = self.project_fs()?.clone();
        if path.as_str().is_empty() {
            return Err("cannot delete project root".to_string());
        }
        if !fs.exists(&path) {
            return Err("path does not exist".to_string());
        }
        fs.delete_path(&path).map_err(|error| error.to_string())?;
        if self
            .active_sequence
            .as_ref()
            .is_some_and(|sequence| sequence == &path || sequence.starts_with(&path))
        {
            self.active_sequence = None;
        }
        Ok(())
    }

    pub fn rename_path(
        &mut self,
        path: Utf8PathBuf,
        new_name: &str,
    ) -> Result<Vec<(Utf8PathBuf, Utf8PathBuf)>, String> {
        validate_file_name(new_name)?;
        let fs = self.project_fs()?.clone();
        let new_path = path
            .parent()
            .ok_or_else(|| "path has no parent".to_string())?
            .join(new_name);
        if fs.exists(&new_path) {
            return Err("target path already exists".to_string());
        }
        fs.rename(&path, &new_path)
            .map_err(|error| error.to_string())?;
        update_active_sequence_after_moves(
            &mut self.active_sequence,
            &[PlannedMove {
                old_path: path.clone(),
                new_path: new_path.clone(),
            }],
        );
        Ok(vec![(path, new_path)])
    }

    pub fn move_paths(
        &mut self,
        paths: Vec<Utf8PathBuf>,
        new_parent: Utf8PathBuf,
    ) -> Result<Vec<(Utf8PathBuf, Utf8PathBuf)>, String> {
        let fs = self.project_fs()?.clone();
        let planned_moves = plan_moves(&fs, paths, new_parent)?;
        apply_planned_moves(&fs, &planned_moves)?;
        update_active_sequence_after_moves(&mut self.active_sequence, &planned_moves);

        Ok(project_path_moves_from_plan(&planned_moves))
    }

    pub fn open_sequence(&mut self, path: Utf8PathBuf) -> Result<(), String> {
        if !self.project_fs()?.is_file(&path) {
            return Err(format!(
                "sequence file not found: {}",
                path.to_slash_string()
            ));
        }
        self.active_sequence = Some(path);
        Ok(())
    }

    pub fn active_sequence(&self) -> Option<&Utf8PathBuf> {
        self.active_sequence.as_ref()
    }

    fn root_project_key(&self, project_file: &Utf8PathBuf) -> Result<String, String> {
        let text = self.read_file(project_file.clone())?;
        let file: DawnFile = serde_yaml::from_str(&text).map_err(|error| error.to_string())?;
        let keys = file
            .iter()
            .filter_map(|(key, object)| match object {
                DawnObject::Project(_) => Some(key.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        match keys.as_slice() {
            [key] => Ok(key.clone()),
            [] => Err("project file has no project object".to_string()),
            _ => Err("project file has more than one project object".to_string()),
        }
    }

    fn canonical_project_path(&self, path: &Utf8PathBuf) -> Result<Utf8PathBuf, String> {
        Ok(canonicalize_path(&self.project_fs()?.resolve(path)))
    }

    fn project_fs(&self) -> Result<&WorkspaceFs, String> {
        self.fs.as_ref().ok_or_else(no_project)
    }

    fn current_project_file(&self) -> Result<Utf8PathBuf, String> {
        self.project_file.clone().ok_or_else(no_project)
    }

    fn project_path_for_selected_file(&self, selected_file: &Path) -> Result<Utf8PathBuf, String> {
        let root = self.root_path.as_ref().ok_or_else(no_project)?;
        let selected_file = selected_file
            .canonicalize()
            .map_err(|error| format!("failed to inspect selected file: {error}"))?;
        let root = root
            .canonicalize()
            .map_err(|error| format!("failed to inspect project root: {error}"))?;
        if let Ok(relative) = selected_file.strip_prefix(&root) {
            utf8_path(relative)
        } else {
            utf8_path(selected_file)
        }
    }
}

fn sequence_document(
    project: &DawnProject,
    path: &Utf8PathBuf,
    object_key: &str,
    sequence: &Sequence<Resolved>,
) -> SequenceDocument {
    let lanes = sequence_lanes(project);
    SequenceDocument {
        path: path.to_slash_string(),
        object_key: object_key.to_string(),
        duration_seconds: sequence.duration.as_seconds_f64(),
        frame_rate: sequence.frame_rate,
        audio: sequence.audio.as_ref().map(|audio| SequenceAudioDocument {
            import: audio.source.raw().to_string(),
            resolved_path: audio.path.to_slash_string(),
            file_name: audio
                .path
                .file_name()
                .map(ToString::to_string)
                .unwrap_or_else(|| audio.path.to_slash_string()),
            exists: audio.path.exists(),
        }),
        mark_collections: sequence
            .mark_collections
            .iter()
            .map(|collection| SequenceMarkCollectionDocument {
                key: collection.key.clone(),
                name: collection.name.clone(),
                color: collection.color.clone(),
                marks_seconds: collection
                    .marks
                    .iter()
                    .map(|mark| mark.as_seconds_f64())
                    .collect(),
            })
            .collect(),
        lanes,
        effect_scripts: project
            .stores
            .effect_definitions
            .iter()
            .map(|(key, effect)| SequenceEffectScriptDocument {
                name: key.name.clone(),
                kind: effect.value.kind,
                script: EffectScriptReferenceDocument {
                    path: key.path.to_slash_string(),
                    effect_name: key.name.clone(),
                },
                import: key.path.to_slash_string(),
                params: effect
                    .value
                    .schema
                    .iter()
                    .map(|schema| SequenceEffectScriptParamDocument {
                        name: schema.name.clone(),
                        value_type: schema.value_type,
                    })
                    .collect(),
            })
            .collect(),
        curve_library: project
            .stores
            .curves
            .iter()
            .map(|(key, curve)| SequenceCurveLibraryItemDocument {
                path: key.path.to_slash_string(),
                object_key: key.name.clone(),
                display_name: key.name.clone(),
                value_type: curve.value.value_type,
                curve: curve.value.clone(),
            })
            .collect(),
        effects: sequence
            .effects
            .iter()
            .enumerate()
            .map(|(index, effect)| sequence_effect_document(project, index, effect))
            .collect(),
        degraded: false,
    }
}

fn sequence_lanes(project: &DawnProject) -> Vec<SequenceLaneDocument> {
    let Some(layout) = active_layout(project) else {
        return Vec::new();
    };
    layout
        .target_order
        .iter()
        .map(|target| SequenceLaneDocument {
            target: LayoutTargetDocument {
                kind: target.kind,
                name: target.id.to_string(),
            },
            label: layout_target_label(layout, target),
        })
        .collect()
}

fn layout_target_label(
    layout: &Layout<Resolved>,
    target: &dawn_project::LayoutTargetRef,
) -> String {
    match target.kind {
        LayoutTargetKind::Group => layout
            .groups
            .iter()
            .find(|group| group.id == GroupInstantiationId(target.id))
            .and_then(|group| group.name.clone())
            .unwrap_or_else(|| target.id.to_string()),
        LayoutTargetKind::Fixture => layout
            .fixtures
            .iter()
            .find(|fixture| fixture.id == FixtureId(target.id))
            .and_then(|fixture| fixture.name.clone())
            .unwrap_or_else(|| target.id.to_string()),
    }
}

fn sequence_effect_document(
    project: &DawnProject,
    index: usize,
    effect: &SequenceEffect<Resolved>,
) -> SequenceEffectDocument {
    let script_source = effect_script_reference(&effect.script);
    let params = effect
        .params
        .iter()
        .map(|(name, value)| SequenceEffectParamDocument {
            name: name.clone(),
            value: value.clone(),
            curve_source: effect_param_curve_source(value),
        })
        .collect::<Vec<_>>();
    let render = effect_script_text(project, &effect.script).map(|script_source| {
        SequenceEffectRenderDocument {
            script: effect.script.key.clone(),
            script_source,
            params: params.clone(),
            target_pixels: sequence_effect_target_pixels(project, &effect.target),
        }
    });
    SequenceEffectDocument {
        index,
        id: effect.id.0,
        start_seconds: effect.start.as_seconds_f64(),
        duration_seconds: effect.duration.as_seconds_f64(),
        target: target_document(&effect.target),
        target_label: target_document(&effect.target).name,
        scope: effect.scope,
        script: effect.script.reference.raw().to_string(),
        script_source,
        params,
        render,
    }
}

fn sequence_effect_target_pixels(
    project: &DawnProject,
    target: &EffectTarget<Resolved>,
) -> Vec<SequenceEffectPixelDocument> {
    let Some(layout) = active_layout(project) else {
        return Vec::new();
    };
    match target {
        EffectTarget::Fixture { id } => layout
            .fixtures
            .iter()
            .position(|fixture| fixture.id == *id)
            .and_then(|fixture_index| {
                layout
                    .fixtures
                    .get(fixture_index)
                    .map(|fixture| target_pixels_for_fixture(project, fixture_index, fixture))
            })
            .unwrap_or_default(),
        EffectTarget::Group { id } => layout
            .groups
            .iter()
            .find(|group| group.id == *id)
            .into_iter()
            .flat_map(|group| {
                group.members.iter().flat_map(|member_id| {
                    layout
                        .fixtures
                        .iter()
                        .position(|fixture| fixture.id == *member_id)
                        .and_then(|fixture_index| {
                            layout.fixtures.get(fixture_index).map(|fixture| {
                                target_pixels_for_fixture(project, fixture_index, fixture)
                            })
                        })
                        .unwrap_or_default()
                })
            })
            .collect(),
    }
}

fn target_pixels_for_fixture(
    project: &DawnProject,
    fixture_index: usize,
    fixture: &dawn_project::FixturePlacement<Resolved>,
) -> Vec<SequenceEffectPixelDocument> {
    let (_, _, fixture_definition) = resolved_fixture(project, &fixture.fixture);
    let pixel_count = geometry_render_plan(&fixture_definition).emitters.len();
    (0..pixel_count)
        .map(|pixel_index| SequenceEffectPixelDocument {
            fixture_index,
            pixel_index,
            pixel_count,
        })
        .collect()
}

fn target_document(target: &EffectTarget<Resolved>) -> LayoutTargetDocument {
    match target {
        EffectTarget::Group { id } => LayoutTargetDocument {
            kind: LayoutTargetKind::Group,
            name: id.to_string(),
        },
        EffectTarget::Fixture { id } => LayoutTargetDocument {
            kind: LayoutTargetKind::Fixture,
            name: id.to_string(),
        },
    }
}

fn effect_script_reference(
    script: &ResolvedSymbolRef<EffectDefinitionKey>,
) -> Option<EffectScriptReferenceDocument> {
    Some(EffectScriptReferenceDocument {
        path: script.key.path.to_slash_string(),
        effect_name: script.key.name.clone(),
    })
}

fn effect_script_text(
    project: &DawnProject,
    script: &ResolvedSymbolRef<EffectDefinitionKey>,
) -> Option<String> {
    match project.stores.source_files.get(&script.key.path)? {
        ResolvedSourceFile::Effect { text } => Some(text.clone()),
        ResolvedSourceFile::Dawn { .. } => None,
    }
}

fn effect_param_curve_source(
    value: &EffectParam<Resolved>,
) -> Option<SequenceEffectParamCurveSourceDocument> {
    let EffectParam::Curve { curve } = value else {
        return None;
    };
    curve_source(&curve.curve)
}

fn curve_source(
    curve: &ResolvedInlineOrRef<Curve, CurveDefinitionKey>,
) -> Option<SequenceEffectParamCurveSourceDocument> {
    match curve {
        ResolvedInlineOrRef::Inline(_) => Some(SequenceEffectParamCurveSourceDocument::Inline),
        ResolvedInlineOrRef::Ref(reference) => {
            Some(SequenceEffectParamCurveSourceDocument::Library {
                reference: reference.reference.raw().to_string(),
                path: Some(reference.key.path.to_slash_string()),
                object_key: Some(reference.key.name.clone()),
                display_name: Some(reference.key.name.clone()),
            })
        }
    }
}

fn layout_document(
    project: &DawnProject,
    source_path: &Utf8PathBuf,
    object_key: &str,
    layout: &Layout<Resolved>,
) -> LayoutDocument {
    let fixtures = layout
        .fixtures
        .iter()
        .map(|placement| {
            let (source_path, object_key, fixture) = resolved_fixture(project, &placement.fixture);
            let name = placement
                .name
                .clone()
                .or_else(|| object_key.clone())
                .unwrap_or_else(|| placement.id.to_string());
            LayoutFixturePlacement {
                id: placement.id,
                name,
                transform: placement.transform,
                resolved_fixture: ResolvedLayoutFixture {
                    name: object_key
                        .clone()
                        .unwrap_or_else(|| "inline fixture".to_string()),
                    color_model: fixture.color_model,
                    bulb_diameter: fixture.bulb_diameter,
                    geometry_summary: geometry_summary(&fixture.geometry),
                    render_plan: geometry_render_plan(&fixture),
                    source_path: source_path
                        .map(|path| path.to_slash_string())
                        .unwrap_or_default(),
                    object_key,
                },
            }
        })
        .collect::<Vec<_>>();
    let render_bounds = fixtures
        .first()
        .map(|fixture| fixture.resolved_fixture.render_plan.bounds)
        .unwrap_or(crate::document::GeometryRenderBounds {
            min_x: dawn_project::Distance::ZERO,
            min_y: dawn_project::Distance::ZERO,
            max_x: dawn_project::Distance::ZERO,
            max_y: dawn_project::Distance::ZERO,
        });
    LayoutDocument {
        path: source_path.to_slash_string(),
        object_key: object_key.to_string(),
        name: object_key.to_string(),
        render_bounds,
        fixtures,
    }
}

fn resolved_fixture(
    project: &DawnProject,
    fixture: &ResolvedInlineOrRef<Fixture, FixtureDefinitionKey>,
) -> (Option<Utf8PathBuf>, Option<String>, Fixture) {
    match fixture {
        ResolvedInlineOrRef::Inline(fixture) => (None, None, fixture.clone()),
        ResolvedInlineOrRef::Ref(reference) => {
            let fixture = project
                .stores
                .fixture_definitions
                .get(&reference.key)
                .map(|fixture| fixture.value.clone())
                .unwrap_or_else(default_fixture);
            (
                Some(reference.key.path.clone()),
                Some(reference.key.name.clone()),
                fixture,
            )
        }
    }
}

fn fixture_definition_document(object_key: &str, fixture: &Fixture) -> FixtureDefinitionDocument {
    FixtureDefinitionDocument {
        object_key: object_key.to_string(),
        name: object_key.to_string(),
        color_model: fixture.color_model,
        bulb_diameter: fixture.bulb_diameter,
        geometry: fixture.geometry.clone(),
        geometry_summary: geometry_summary(&fixture.geometry),
        render_plan: geometry_render_plan(fixture),
    }
}

fn active_layout(project: &DawnProject) -> Option<&Layout<Resolved>> {
    let display = match &project.display {
        ResolvedInlineOrRef::Inline(display) => display,
        ResolvedInlineOrRef::Ref(reference) => &project.stores.displays.get(&reference.key)?.value,
    };
    match &display.layout {
        ResolvedInlineOrRef::Inline(layout) => Some(layout),
        ResolvedInlineOrRef::Ref(reference) => project
            .stores
            .layouts
            .get(&reference.key)
            .map(|layout| &layout.value),
    }
}

fn source_object_kind(object: &ResolvedSourceObject) -> ObjectKind {
    match object {
        ResolvedSourceObject::Project(_) => ObjectKind::Project,
        ResolvedSourceObject::Display(_) => ObjectKind::Display,
        ResolvedSourceObject::Controller(_) => ObjectKind::Controller,
        ResolvedSourceObject::Layout(_) => ObjectKind::Layout,
        ResolvedSourceObject::Fixture(_) => ObjectKind::Fixture,
        ResolvedSourceObject::Patch(_) => ObjectKind::Patch,
        ResolvedSourceObject::Sequence(_) => ObjectKind::Sequence,
        ResolvedSourceObject::Curve(_) => ObjectKind::Curve,
        ResolvedSourceObject::Unused(object) => object.kind(),
    }
}

fn default_fixture() -> Fixture {
    Fixture {
        color_model: dawn_project::ColorModel::Rgb,
        bulb_diameter: dawn_project::DistanceSpan::try_from_meters_f64_truncated(0.01)
            .unwrap_or(dawn_project::DistanceSpan::ZERO),
        geometry: Geometry::Points { points: Vec::new() },
    }
}

fn diagnostic(file: Utf8PathBuf, message: String) -> ProjectDiagnostic {
    ProjectDiagnostic {
        severity: dawn_project::DiagnosticSeverity::Error,
        file,
        range: None,
        message,
        kind: dawn_project::ProjectDiagnosticKind::Io,
    }
}

fn content_hash(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn no_project() -> String {
    "no project open".to_string()
}

fn list_project_entries(fs: &WorkspaceFs) -> Result<Vec<WorkspaceEntry>, String> {
    let mut entries = fs.list_entries().map_err(|error| error.to_string())?;
    entries.sort_by(|left, right| {
        (left.kind != WorkspaceEntryKind::Directory, &left.path)
            .cmp(&(right.kind != WorkspaceEntryKind::Directory, &right.path))
    });
    Ok(entries)
}

fn validate_file_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("name cannot be empty".to_string());
    }
    if name == "." || name == ".." {
        return Err("name cannot be . or ..".to_string());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("name cannot contain path separators".to_string());
    }
    Ok(())
}

fn file_name_with_default_extension(name: &str) -> Result<String, String> {
    validate_file_name(name)?;
    let path = Path::new(name);
    if path.extension().is_none() {
        Ok(format!("{name}.dawn"))
    } else {
        Ok(name.to_string())
    }
}

fn plan_moves(
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

fn apply_planned_moves(fs: &WorkspaceFs, planned_moves: &[PlannedMove]) -> Result<(), String> {
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

fn project_path_moves_from_plan(planned_moves: &[PlannedMove]) -> Vec<(Utf8PathBuf, Utf8PathBuf)> {
    planned_moves
        .iter()
        .map(|planned_move| (planned_move.old_path.clone(), planned_move.new_path.clone()))
        .collect()
}

fn update_active_sequence_after_moves(
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

pub fn serialized_import_path(importing_path: &Utf8PathBuf, imported_path: &Utf8PathBuf) -> String {
    if imported_path.is_absolute() {
        return imported_path.to_slash_string();
    }
    let importing_dir = importing_path
        .parent()
        .unwrap_or_else(|| camino::Utf8Path::new(""));
    pathdiff::diff_paths(imported_path.as_std_path(), importing_dir.as_std_path())
        .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        .unwrap_or_else(|| imported_path.clone())
        .to_slash_string()
}
