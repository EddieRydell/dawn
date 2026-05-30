use std::collections::{BTreeMap, HashMap, HashSet};

use indexmap::IndexMap;
use serde::Serialize;

use crate::effect_script::{
    compile as compile_effect_script, CompiledEffect, ParamDefault, RuntimeValue, ScriptDiagnostic,
};
use crate::fs::{WorkspaceEntryKind, WorkspaceFs};
use crate::lower::{
    lower_project, select_referenced_object, LowerError, ResolvedEffectImport, ResolvedImport,
    SymbolResolver,
};
use crate::model::*;
use crate::parse::{parse_dawn_file_with_source_map, YamlPath, YamlSourceMap};
use crate::path::{canonicalize_path, resolve_import_path, PathStringExt, Utf8PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectAnalysis {
    pub root_path: Utf8PathBuf,
    pub project_key: String,
    pub files: IndexMap<Utf8PathBuf, AnalyzedFile>,
    pub scripts: IndexMap<String, EffectScriptAnalysis>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub resolved: Option<ResolvedProject>,
}

impl ProjectAnalysis {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }

    pub fn is_resolved(&self) -> bool {
        self.resolved.is_some()
    }

    pub fn reachable_file_count(&self) -> usize {
        self.files.len()
    }

    pub fn object_count(&self) -> usize {
        self.files
            .values()
            .filter_map(|file| file.file.as_ref())
            .map(|file| file.len())
            .sum()
    }

    pub fn compiled_script_for_path(&self, path: &Utf8PathBuf) -> Option<&CompiledEffect> {
        self.scripts
            .get(&path.to_slash_string())?
            .result
            .as_ref()
            .ok()
    }

    pub fn sample_effect_script(
        &self,
        script_path: &Utf8PathBuf,
        progress: f64,
        seconds: f64,
        fixture: crate::effect_script::FixtureContext,
        pixel: crate::effect_script::PixelContext,
        params: BTreeMap<String, RuntimeValue>,
    ) -> Result<Color, String> {
        self.sample_effect_script_key(
            &script_path.to_slash_string(),
            progress,
            seconds,
            fixture,
            pixel,
            params,
        )
    }

    pub fn sample_effect_script_key(
        &self,
        script_key: &str,
        progress: f64,
        seconds: f64,
        fixture: crate::effect_script::FixtureContext,
        pixel: crate::effect_script::PixelContext,
        params: BTreeMap<String, RuntimeValue>,
    ) -> Result<Color, String> {
        let script = self
            .scripts
            .get(script_key)
            .and_then(|script| script.result.as_ref().ok())
            .ok_or_else(|| format!("compiled script `{script_key}` was not found"))?;
        script
            .sample(progress, seconds, fixture, pixel, &params)
            .map_err(|error| error.to_string())
    }

    pub fn default_runtime_params_for_script(
        &self,
        script_path: &Utf8PathBuf,
    ) -> BTreeMap<String, RuntimeValue> {
        let Some(script_analysis) = self.scripts.get(&script_path.to_slash_string()) else {
            return BTreeMap::new();
        };
        let Some(script) = script_analysis.result.as_ref().ok() else {
            return BTreeMap::new();
        };
        script
            .params
            .iter()
            .filter_map(|param| {
                param
                    .default
                    .as_ref()
                    .map(|ParamDefault::Value(value)| (param.name.clone(), value.clone()))
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct AnalyzedFile {
    pub path: Utf8PathBuf,
    pub text: Option<String>,
    pub file: Option<DawnFile>,
    pub source_map: Option<YamlSourceMap>,
    pub script: Option<EffectScriptAnalysis>,
}

#[derive(Debug, Clone)]
pub struct EffectScriptAnalysis {
    pub source: ScriptSource,
    pub result: Result<CompiledEffect, Vec<ScriptDiagnostic>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiagnostic {
    pub path: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub severity: DiagnosticSeverity,
    pub code: DiagnosticCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    Io,
    Yaml,
    Import,
    Lower,
    ProjectKey,
    Sequence,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectOverlay {
    pub path: Utf8PathBuf,
    pub content: String,
}
pub fn analyze_project(
    fs: &WorkspaceFs,
    project_path: Utf8PathBuf,
    project_key: &str,
) -> ProjectAnalysis {
    analyze_project_with_overlays(fs, project_path, Some(project_key), Vec::new())
}

pub fn analyze_project_with_overlays(
    fs: &WorkspaceFs,
    project_path: Utf8PathBuf,
    project_key: Option<&str>,
    overlays: Vec<ProjectOverlay>,
) -> ProjectAnalysis {
    let root_path = canonicalize_path(&fs.resolve(&project_path));
    let overlays = overlays
        .into_iter()
        .map(|overlay| ProjectOverlay {
            path: canonicalize_path(&fs.resolve(&overlay.path)),
            content: overlay.content,
        })
        .collect();
    let mut session = AnalysisSession::new(fs.clone(), overlays);
    session.visit_file(root_path.clone());

    let inferred_project_key = if let Some(project_key) = project_key {
        Some(project_key.to_string())
    } else {
        infer_project_key(&root_path, &mut session)
    };

    session.scan_workspace_files();

    let mut resolved = None;
    if !session.has_errors() {
        if let Some(root_file) = session
            .files
            .get(&root_path)
            .and_then(|analyzed| analyzed.file.as_ref())
        {
            if let Some(project_key) = inferred_project_key.as_deref() {
                let mut loader = AnalysisImportResolver {
                    files: &session.files,
                    scripts: &session.scripts,
                };
                match lower_project(root_file, project_key, &root_path, &mut loader) {
                    Ok(project) => {
                        session.validate_resolved_effects(&root_path, &project);
                        if !session.has_errors() {
                            resolved = Some(project);
                        }
                    }
                    Err(error) => {
                        let (path, range) = session.locate_lower_error(&root_path, &error);
                        session.diagnostics.push(ProjectDiagnostic {
                            path,
                            range,
                            severity: DiagnosticSeverity::Error,
                            code: DiagnosticCode::Lower,
                            message: error.to_string(),
                        });
                    }
                }
            }
        }
    }

    ProjectAnalysis {
        root_path,
        project_key: inferred_project_key.unwrap_or_default(),
        files: session.files,
        scripts: session.scripts,
        diagnostics: session.diagnostics,
        resolved,
    }
}

fn infer_project_key(root_path: &Utf8PathBuf, session: &mut AnalysisSession) -> Option<String> {
    let root_file = session
        .files
        .get(root_path)
        .and_then(|analyzed| analyzed.file.as_ref())?;

    let project_keys = root_file
        .iter()
        .filter_map(|(key, object)| match object {
            DawnObject::Project(_) => Some(key.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    match project_keys.as_slice() {
        [project_key] => Some(project_key.clone()),
        [] => {
            session.diagnostics.push(ProjectDiagnostic {
                path: root_path.clone(),
                range: None,
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::ProjectKey,
                message: "root file must contain one project object, but found none".to_string(),
            });
            None
        }
        _ => {
            session.diagnostics.push(ProjectDiagnostic {
                path: root_path.clone(),
                range: None,
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::ProjectKey,
                message: format!(
                    "root file must contain one project object, but found {}",
                    project_keys.len()
                ),
            });
            None
        }
    }
}
struct AnalysisSession {
    fs: WorkspaceFs,
    files: IndexMap<Utf8PathBuf, AnalyzedFile>,
    diagnostics: Vec<ProjectDiagnostic>,
    scripts: IndexMap<String, EffectScriptAnalysis>,
    visiting: HashSet<Utf8PathBuf>,
    overlays: HashMap<Utf8PathBuf, String>,
}

impl AnalysisSession {
    fn new(fs: WorkspaceFs, overlays: Vec<ProjectOverlay>) -> Self {
        Self {
            fs,
            files: IndexMap::new(),
            diagnostics: Vec::new(),
            scripts: IndexMap::new(),
            visiting: HashSet::new(),
            overlays: overlays
                .into_iter()
                .map(|overlay| (overlay.path, overlay.content))
                .collect(),
        }
    }

    fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }

    fn visit_file(&mut self, path: Utf8PathBuf) {
        if self.files.contains_key(&path) || !self.visiting.insert(path.clone()) {
            return;
        }

        let text = match self
            .overlays
            .get(&path)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| self.fs.read_to_string(&path))
        {
            Ok(text) => text,
            Err(source) => {
                self.diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: None,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Io,
                    message: format!("failed to read `{}`: {source}", path),
                });
                self.files.insert(
                    path.clone(),
                    AnalyzedFile {
                        path: path.clone(),
                        text: None,
                        file: None,
                        source_map: None,
                        script: None,
                    },
                );
                self.visiting.remove(&path);
                return;
            }
        };

        if is_effect_script_path(&path) {
            let result = compile_effect_script(&text);
            for diagnostic in script_diagnostics(&path, &result) {
                self.diagnostics.push(diagnostic);
            }
            let script = EffectScriptAnalysis {
                source: ScriptSource::External(path.clone()),
                result,
            };
            self.scripts.insert(path.to_slash_string(), script.clone());
            self.files.insert(
                path.clone(),
                AnalyzedFile {
                    path: path.clone(),
                    text: Some(text.clone()),
                    file: None,
                    source_map: None,
                    script: Some(script),
                },
            );
            self.visiting.remove(&path);
            return;
        }

        let parsed = match parse_dawn_file_with_source_map(&text) {
            Ok(parsed) => Some(parsed),
            Err(source) => {
                self.diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: source.range,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Yaml,
                    message: source.to_string(),
                });
                None
            }
        };

        let file = parsed.as_ref().map(|parsed| parsed.file.clone());
        let source_map = parsed.as_ref().map(|parsed| parsed.source_map.clone());
        let imports = file
            .as_ref()
            .map(|file| file.imports.iter().cloned().enumerate().collect::<Vec<_>>())
            .unwrap_or_else(Vec::new);
        if let (Some(file), Some(source_map)) = (file.as_ref(), source_map.as_ref()) {
            validate_module_declarations(&path, source_map, file, &mut self.diagnostics);
            validate_sequence_marks(&path, source_map, file, &mut self.diagnostics);
        }
        self.files.insert(
            path.clone(),
            AnalyzedFile {
                path: path.clone(),
                text: Some(text.clone()),
                file,
                source_map: source_map.clone(),
                script: None,
            },
        );

        for (import_index, import) in imports {
            self.visit_import(&path, import_index, import);
        }

        self.visiting.remove(&path);
    }

    fn can_load_file(&self, path: &Utf8PathBuf) -> bool {
        self.overlays.contains_key(path) || self.fs.is_file(path)
    }

    fn import_field_range(
        &self,
        source_path: &Utf8PathBuf,
        import_index: usize,
        field: &str,
    ) -> Option<TextRange> {
        self.files
            .get(source_path)
            .and_then(|file| file.source_map.as_ref())
            .and_then(|source_map| {
                source_map.value_range(
                    YamlPath::root()
                        .field("imports")
                        .index(import_index)
                        .field(field),
                )
            })
    }

    fn visit_import(&mut self, source_path: &Utf8PathBuf, import_index: usize, import: DawnImport) {
        let range = self.import_field_range(source_path, import_index, "from");
        let import_path = resolve_import_path(source_path, &import.from);
        if self.fs.is_dir(&import_path) {
            let Ok(entries) = std::fs::read_dir(import_path.as_std_path()) else {
                self.diagnostics.push(ProjectDiagnostic {
                    path: source_path.clone(),
                    range,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Import,
                    message: format!("failed to read import `{}`", import.from),
                });
                return;
            };
            for entry in entries.flatten() {
                let Ok(path) = Utf8PathBuf::from_path_buf(entry.path()) else {
                    continue;
                };
                if path.is_file() && is_dawn_path(&path) {
                    if self.visiting.contains(&canonicalize_path(&path)) {
                        self.diagnostics.push(ProjectDiagnostic {
                            path: source_path.clone(),
                            range,
                            severity: DiagnosticSeverity::Error,
                            code: DiagnosticCode::Import,
                            message: format!("import cycle includes `{}`", path.to_slash_string()),
                        });
                        continue;
                    }
                    self.visit_file(canonicalize_path(&path));
                }
            }
            return;
        }
        if !self.can_load_file(&import_path) {
            self.diagnostics.push(ProjectDiagnostic {
                path: source_path.clone(),
                range,
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::Import,
                message: format!(
                    "failed to read import `{}`: file `{}` was not found",
                    import.from, import_path
                ),
            });
            return;
        }

        if self.visiting.contains(&import_path) {
            self.diagnostics.push(ProjectDiagnostic {
                path: source_path.clone(),
                range,
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::Import,
                message: format!("import cycle includes `{}`", import_path.to_slash_string()),
            });
            return;
        }

        self.visit_file(import_path);
    }

    fn scan_workspace_files(&mut self) {
        let mut paths = Vec::new();
        if let Ok(entries) = self.fs.list_entries() {
            for entry in entries {
                if entry.kind != WorkspaceEntryKind::File || !is_dawn_path(&entry.path) {
                    continue;
                }
                paths.push(canonicalize_path(&self.fs.resolve(&entry.path)));
            }
        }

        let workspace_root = canonicalize_path(self.fs.root());
        for path in self.overlays.keys() {
            if is_dawn_path(path) && path.starts_with(&workspace_root) {
                paths.push(path.clone());
            }
        }

        paths.sort();
        paths.dedup();

        for path in paths {
            if self.files.contains_key(&path) {
                continue;
            }
            self.scan_workspace_file(path);
        }
    }

    fn scan_workspace_file(&mut self, path: Utf8PathBuf) {
        let text = match self
            .overlays
            .get(&path)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| self.fs.read_to_string(&path))
        {
            Ok(text) => text,
            Err(source) => {
                self.diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: None,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Io,
                    message: format!("failed to read `{}`: {source}", path),
                });
                return;
            }
        };

        if is_effect_script_path(&path) {
            let result = compile_effect_script(&text);
            for diagnostic in script_diagnostics(&path, &result) {
                self.diagnostics.push(diagnostic);
            }
            return;
        }

        if let Err(source) = parse_dawn_file_with_source_map(&text) {
            self.diagnostics.push(ProjectDiagnostic {
                path,
                range: source.range,
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::Yaml,
                message: source.to_string(),
            });
        }
    }

    fn locate_lower_error(
        &self,
        root_path: &Utf8PathBuf,
        error: &LowerError,
    ) -> (Utf8PathBuf, Option<TextRange>) {
        self.locate_lower_error_range(root_path, error)
            .map(|(path, range)| (path, Some(range)))
            .unwrap_or_else(|| (root_path.clone(), None))
    }

    fn locate_lower_error_range(
        &self,
        root_path: &Utf8PathBuf,
        error: &LowerError,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        match error {
            LowerError::MissingProject { key } | LowerError::WrongObjectKind { key, .. } => self
                .source_value_range(root_path, YamlPath::root().field(key.clone()))
                .or_else(|| self.source_key_range(root_path, YamlPath::root().field(key.clone()))),
            LowerError::Import { reference, .. }
            | LowerError::WrongImportedObjectKind { reference, .. } => {
                self.locate_symbol_ref(root_path, reference)
            }
            LowerError::DuplicateFixtureId { id } => self.locate_duplicate_fixture_id(*id),
            LowerError::EmptyFixtureName => self.locate_empty_fixture_name(),
            LowerError::DuplicateFixtureName { name } => self.locate_duplicate_fixture_name(name),
            LowerError::UnknownFixture { id } => self.locate_fixture_ref(*id),
            LowerError::DuplicateControllerName { name } => {
                self.locate_duplicate_controller_name(name)
            }
            LowerError::UnknownController { name } => self.locate_controller_ref(name),
            LowerError::DuplicateGroupName { name } => self.locate_duplicate_group_name(name),
            LowerError::UnknownGroup { name } => self.locate_group_ref(name),
            LowerError::DuplicateLayoutTargetOrderEntry { kind, name }
            | LowerError::UnknownLayoutTargetOrderEntry { kind, name }
            | LowerError::MissingLayoutTargetOrderEntry { kind, name } => {
                self.locate_layout_target_order(*kind, name)
            }
            LowerError::DuplicateSequenceEffectId { id } => {
                self.locate_duplicate_sequence_effect_id(*id)
            }
            LowerError::UnknownSequenceEffect { id } => self.locate_sequence_effect_ref(*id),
            LowerError::AutomationCurveType { id, .. } => self.locate_automation_clip_curve(*id),
        }
    }

    fn source_value_range(
        &self,
        path: &Utf8PathBuf,
        yaml_path: YamlPath,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        self.files
            .get(path)
            .and_then(|file| file.source_map.as_ref())
            .and_then(|source_map| source_map.value_range(yaml_path))
            .map(|range| (path.clone(), range))
    }

    fn source_key_range(
        &self,
        path: &Utf8PathBuf,
        yaml_path: YamlPath,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        self.files
            .get(path)
            .and_then(|file| file.source_map.as_ref())
            .and_then(|source_map| source_map.key_range(yaml_path))
            .map(|range| (path.clone(), range))
    }

    fn locate_symbol_ref(
        &self,
        preferred_path: &Utf8PathBuf,
        reference: &str,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_files_ordered(preferred_path, |file| {
            let dawn_file = file.file.as_ref()?;
            for (object_key, object) in dawn_file {
                let object_path = YamlPath::root().field(object_key.clone());
                if let Some(range) = object_symbol_ref_range(file, &object_path, object, reference)
                {
                    return Some(range);
                }
            }
            None
        })
    }

    fn locate_duplicate_fixture_id(&self, id: FixtureId) -> Option<(Utf8PathBuf, TextRange)> {
        let mut seen = false;
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            for (object_key, object) in dawn_file {
                if let Some(range) = duplicate_fixture_id_in_object(
                    file,
                    YamlPath::root().field(object_key.clone()),
                    object,
                    id,
                    &mut seen,
                ) {
                    return Some(range);
                }
            }
            None
        })
    }

    fn locate_empty_fixture_name(&self) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            for (object_key, object) in dawn_file {
                let DawnObject::Layout(layout) = object else {
                    continue;
                };
                for (index, fixture) in layout.fixtures.iter().enumerate() {
                    if fixture.name.trim().is_empty() {
                        return file.source_map.as_ref()?.value_range(
                            YamlPath::root()
                                .field(object_key.clone())
                                .field("fixtures")
                                .index(index)
                                .field("name"),
                        );
                    }
                }
            }
            None
        })
    }

    fn locate_duplicate_fixture_name(&self, name: &str) -> Option<(Utf8PathBuf, TextRange)> {
        let mut seen = false;
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            for (object_key, object) in dawn_file {
                let DawnObject::Layout(layout) = object else {
                    continue;
                };
                for (index, fixture) in layout.fixtures.iter().enumerate() {
                    if fixture.name.trim() == name {
                        if seen {
                            return file.source_map.as_ref()?.value_range(
                                YamlPath::root()
                                    .field(object_key.clone())
                                    .field("fixtures")
                                    .index(index)
                                    .field("name"),
                            );
                        }
                        seen = true;
                    }
                }
            }
            None
        })
    }

    fn locate_fixture_ref(&self, id: FixtureId) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            let source_map = file.source_map.as_ref()?;
            for (object_key, object) in dawn_file {
                match object {
                    DawnObject::Layout(layout) => {
                        for (group_index, group) in layout.groups.iter().enumerate() {
                            for (member_index, member) in group.members.iter().enumerate() {
                                if *member == id {
                                    return source_map.value_range(
                                        YamlPath::root()
                                            .field(object_key.clone())
                                            .field("groups")
                                            .index(group_index)
                                            .field("members")
                                            .index(member_index),
                                    );
                                }
                            }
                        }
                    }
                    DawnObject::Patch(patch) => {
                        for (route_index, route) in patch.routes.iter().enumerate() {
                            if route.fixture == id {
                                return source_map.value_range(
                                    YamlPath::root()
                                        .field(object_key.clone())
                                        .field("routes")
                                        .index(route_index)
                                        .field("fixture"),
                                );
                            }
                        }
                    }
                    DawnObject::Sequence(sequence) => {
                        for (effect_index, effect) in sequence.effects.iter().enumerate() {
                            if matches!(effect.target, EffectTarget::Fixture { id: fixture } if fixture == id)
                            {
                                return source_map.value_range(
                                    YamlPath::root()
                                        .field(object_key.clone())
                                        .field("effects")
                                        .index(effect_index)
                                        .field("target")
                                        .field("id"),
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
            None
        })
    }

    fn locate_duplicate_controller_name(&self, name: &str) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_duplicate_display_controller_name(name)
    }

    fn locate_controller_ref(&self, name: &str) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            let source_map = file.source_map.as_ref()?;
            for (object_key, object) in dawn_file {
                let DawnObject::Patch(patch) = object else {
                    continue;
                };
                for (route_index, route) in patch.routes.iter().enumerate() {
                    if route.controller.as_str() == name {
                        return source_map.value_range(
                            YamlPath::root()
                                .field(object_key.clone())
                                .field("routes")
                                .index(route_index)
                                .field("controller"),
                        );
                    }
                }
            }
            None
        })
    }

    fn locate_duplicate_group_name(&self, name: &str) -> Option<(Utf8PathBuf, TextRange)> {
        let mut seen = false;
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            for (object_key, object) in dawn_file {
                let DawnObject::Layout(layout) = object else {
                    continue;
                };
                for (index, group) in layout.groups.iter().enumerate() {
                    if group.name == name {
                        if seen {
                            return file.source_map.as_ref()?.value_range(
                                YamlPath::root()
                                    .field(object_key.clone())
                                    .field("groups")
                                    .index(index)
                                    .field("name"),
                            );
                        }
                        seen = true;
                    }
                }
            }
            None
        })
    }

    fn locate_group_ref(&self, name: &str) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            for (object_key, object) in dawn_file {
                if let Some(range) = group_ref_in_object(
                    file,
                    YamlPath::root().field(object_key.clone()),
                    object,
                    name,
                ) {
                    return Some(range);
                }
            }
            None
        })
    }

    fn locate_layout_target_order(
        &self,
        kind: LayoutTargetKind,
        name: &str,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            let source_map = file.source_map.as_ref()?;
            for (object_key, object) in dawn_file {
                let DawnObject::Layout(layout) = object else {
                    continue;
                };
                for (index, target) in layout.target_order.iter().enumerate() {
                    if target.kind == kind && target.name == name {
                        return source_map.value_range(
                            YamlPath::root()
                                .field(object_key.clone())
                                .field("target_order")
                                .index(index)
                                .field("name"),
                        );
                    }
                }
            }
            None
        })
    }

    fn locate_duplicate_sequence_effect_id(&self, id: u32) -> Option<(Utf8PathBuf, TextRange)> {
        let mut seen = false;
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            for (object_key, object) in dawn_file {
                let DawnObject::Sequence(sequence) = object else {
                    continue;
                };
                for (index, effect) in sequence.effects.iter().enumerate() {
                    if effect.id == id {
                        if seen {
                            return file.source_map.as_ref()?.value_range(
                                YamlPath::root()
                                    .field(object_key.clone())
                                    .field("effects")
                                    .index(index)
                                    .field("id"),
                            );
                        }
                        seen = true;
                    }
                }
            }
            None
        })
    }

    fn locate_sequence_effect_ref(&self, id: u32) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            let source_map = file.source_map.as_ref()?;
            for (object_key, object) in dawn_file {
                let DawnObject::Sequence(sequence) = object else {
                    continue;
                };
                for (clip_index, clip) in sequence.automation_clips.iter().enumerate() {
                    for (target_index, target) in clip.targets.iter().enumerate() {
                        if *target == id {
                            return source_map.value_range(
                                YamlPath::root()
                                    .field(object_key.clone())
                                    .field("automation_clips")
                                    .index(clip_index)
                                    .field("targets")
                                    .index(target_index),
                            );
                        }
                    }
                }
            }
            None
        })
    }

    fn locate_automation_clip_curve(&self, id: u32) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            let source_map = file.source_map.as_ref()?;
            for (object_key, object) in dawn_file {
                let DawnObject::Sequence(sequence) = object else {
                    continue;
                };
                for (clip_index, clip) in sequence.automation_clips.iter().enumerate() {
                    if clip.id == id {
                        return source_map.value_range(
                            YamlPath::root()
                                .field(object_key.clone())
                                .field("automation_clips")
                                .index(clip_index)
                                .field("curve"),
                        );
                    }
                }
            }
            None
        })
    }

    fn locate_duplicate_display_controller_name(
        &self,
        name: &str,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        let mut seen = false;
        self.locate_in_files(|file| {
            let dawn_file = file.file.as_ref()?;
            for (object_key, object) in dawn_file {
                let DawnObject::Display(display) = object else {
                    continue;
                };
                for (index, controller) in display.controllers.iter().enumerate() {
                    let InlineOrRef::Inline(controller) = controller else {
                        continue;
                    };
                    if controller.name == name {
                        if seen {
                            return file.source_map.as_ref()?.value_range(
                                YamlPath::root()
                                    .field(object_key.clone())
                                    .field("controllers")
                                    .index(index)
                                    .field("name"),
                            );
                        }
                        seen = true;
                    }
                }
            }
            None
        })
    }

    fn locate_in_files(
        &self,
        mut locate: impl FnMut(&AnalyzedFile) -> Option<TextRange>,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        for (path, file) in &self.files {
            if let Some(range) = locate(file) {
                return Some((path.clone(), range));
            }
        }
        None
    }

    fn locate_in_files_ordered(
        &self,
        preferred_path: &Utf8PathBuf,
        mut locate: impl FnMut(&AnalyzedFile) -> Option<TextRange>,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        if let Some(file) = self.files.get(preferred_path) {
            if let Some(range) = locate(file) {
                return Some((preferred_path.clone(), range));
            }
        }
        for (path, file) in &self.files {
            if path == preferred_path {
                continue;
            }
            if let Some(range) = locate(file) {
                return Some((path.clone(), range));
            }
        }
        None
    }

    fn validate_resolved_effects(&mut self, root_path: &Utf8PathBuf, project: &ResolvedProject) {
        for sequence in &project.sequences {
            for effect in &sequence.effects {
                let script = match &effect.script {
                    ScriptSource::External(path) => self
                        .scripts
                        .get(&path.to_slash_string())
                        .and_then(|script| script.result.as_ref().ok())
                        .cloned(),
                    ScriptSource::Inline(text) => {
                        let key = format!("inline:{}:{}", root_path.to_slash_string(), effect.id);
                        let result = compile_effect_script(text);
                        for diagnostic in
                            self.inline_script_diagnostics(root_path, effect.id, &result)
                        {
                            self.diagnostics.push(diagnostic);
                        }
                        let script = result.as_ref().ok().cloned();
                        self.scripts.insert(
                            key,
                            EffectScriptAnalysis {
                                source: ScriptSource::Inline(text.clone()),
                                result,
                            },
                        );
                        script
                    }
                };
                if let Some(script) = script {
                    self.validate_effect_params(
                        root_path,
                        &effect.id,
                        &script,
                        &effect.params,
                        &sequence.mark_collections,
                    );
                }
            }
        }
    }

    fn validate_effect_params(
        &mut self,
        root_path: &Utf8PathBuf,
        effect_id: &u32,
        script: &CompiledEffect,
        params: &IndexMap<String, EffectParam<Resolved>>,
        mark_collections: &[SequenceMarkCollection],
    ) {
        for name in params.keys() {
            if script.param(name).is_none() {
                let (path, range) = self
                    .locate_effect_param(*effect_id, name)
                    .map(|(path, range)| (path, Some(range)))
                    .unwrap_or_else(|| (root_path.clone(), None));
                self.diagnostics.push(ProjectDiagnostic {
                    path,
                    range,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Script,
                    message: format!(
                        "effect `{effect_id}` passes unknown parameter `{name}` to script `{}`",
                        script.name
                    ),
                });
            }
        }
        for schema in &script.params {
            match params.get(&schema.name) {
                Some(param) if schema.value_type.matches_param(param) => {
                    self.validate_effect_param_options(
                        root_path,
                        effect_id,
                        script,
                        schema,
                        param,
                        mark_collections,
                    );
                }
                Some(_) => {
                    let (path, range) = self
                        .locate_effect_param_type(*effect_id, &schema.name)
                        .map(|(path, range)| (path, Some(range)))
                        .unwrap_or_else(|| (root_path.clone(), None));
                    self.diagnostics.push(ProjectDiagnostic {
                        path,
                        range,
                        severity: DiagnosticSeverity::Error,
                        code: DiagnosticCode::Script,
                        message: format!(
                            "effect `{effect_id}` parameter `{}` must be {}",
                            schema.name, schema.value_type
                        ),
                    });
                }
                None if schema.default.is_some() => {}
                None => {
                    let (path, range) = self
                        .locate_effect_params_block(*effect_id)
                        .map(|(path, range)| (path, Some(range)))
                        .unwrap_or_else(|| (root_path.clone(), None));
                    self.diagnostics.push(ProjectDiagnostic {
                        path,
                        range,
                        severity: DiagnosticSeverity::Error,
                        code: DiagnosticCode::Script,
                        message: format!(
                            "effect `{effect_id}` is missing required parameter `{}`",
                            schema.name
                        ),
                    });
                }
            }
        }
    }

    fn validate_effect_param_options(
        &mut self,
        root_path: &Utf8PathBuf,
        effect_id: &u32,
        script: &CompiledEffect,
        schema: &crate::effect_script::EffectParamSchema,
        param: &EffectParam<Resolved>,
        mark_collections: &[SequenceMarkCollection],
    ) {
        match param {
            EffectParam::Enum { value } if !schema.options.contains(value) => {
                let (path, range) = self
                    .locate_effect_param_value(*effect_id, &schema.name, value)
                    .map(|(path, range)| (path, Some(range)))
                    .unwrap_or_else(|| (root_path.clone(), None));
                self.diagnostics.push(ProjectDiagnostic {
                    path,
                    range,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Script,
                    message: format!(
                        "effect `{effect_id}` parameter `{}` value `{value}` is not declared by script `{}`",
                        schema.name, script.name
                    ),
                });
            }
            EffectParam::Flags { value } => {
                for flag in &value.values {
                    if !schema.options.contains(flag) {
                        let (path, range) = self
                            .locate_effect_param_value(*effect_id, &schema.name, flag)
                            .map(|(path, range)| (path, Some(range)))
                            .unwrap_or_else(|| (root_path.clone(), None));
                        self.diagnostics.push(ProjectDiagnostic {
                            path,
                            range,
                            severity: DiagnosticSeverity::Error,
                            code: DiagnosticCode::Script,
                            message: format!(
                                "effect `{effect_id}` parameter `{}` flag `{flag}` is not declared by script `{}`",
                                schema.name, script.name
                            ),
                        });
                    }
                }
            }
            EffectParam::Marks { key }
                if !mark_collections
                    .iter()
                    .any(|collection| collection.key == *key) =>
            {
                let (path, range) = self
                    .locate_effect_param_value(*effect_id, &schema.name, key)
                    .map(|(path, range)| (path, Some(range)))
                    .unwrap_or_else(|| (root_path.clone(), None));
                self.diagnostics.push(ProjectDiagnostic {
                    path,
                    range,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Script,
                    message: format!(
                        "effect `{effect_id}` parameter `{}` references unknown mark collection `{key}` for script `{}`",
                        schema.name, script.name
                    ),
                });
            }
            _ => {}
        }
    }

    fn inline_script_diagnostics(
        &self,
        fallback_path: &Utf8PathBuf,
        effect_id: u32,
        result: &Result<CompiledEffect, Vec<ScriptDiagnostic>>,
    ) -> Vec<ProjectDiagnostic> {
        result
            .as_ref()
            .err()
            .into_iter()
            .flatten()
            .map(|diagnostic| {
                let located = diagnostic
                    .range
                    .and_then(|range| self.locate_inline_script_range(effect_id, range));
                ProjectDiagnostic {
                    path: located
                        .as_ref()
                        .map(|(path, _)| path.clone())
                        .unwrap_or_else(|| fallback_path.clone()),
                    range: located.map(|(_, range)| range),
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Script,
                    message: diagnostic.message.clone(),
                }
            })
            .collect()
    }

    fn locate_effect_param(
        &self,
        effect_id: u32,
        param_name: &str,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_yaml_files(|text| locate_effect_param(text, effect_id, param_name))
    }

    fn locate_effect_param_type(
        &self,
        effect_id: u32,
        param_name: &str,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_yaml_files(|text| {
            locate_effect_param_field(text, effect_id, param_name, "type", None)
        })
        .or_else(|| self.locate_effect_param(effect_id, param_name))
    }

    fn locate_effect_param_value(
        &self,
        effect_id: u32,
        param_name: &str,
        value: &str,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_yaml_files(|text| {
            locate_effect_param_value(text, effect_id, param_name, value)
        })
        .or_else(|| self.locate_effect_param(effect_id, param_name))
    }

    fn locate_effect_params_block(&self, effect_id: u32) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_yaml_files(|text| locate_effect_params_block(text, effect_id))
            .or_else(|| self.locate_in_yaml_files(|text| locate_effect_id(text, effect_id)))
    }

    fn locate_inline_script_range(
        &self,
        effect_id: u32,
        range: crate::effect_script::SourceRange,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        self.locate_in_yaml_files(|text| locate_inline_script_range(text, effect_id, range))
    }

    fn locate_in_yaml_files(
        &self,
        locate: impl Fn(&str) -> Option<TextRange>,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        for (path, file) in &self.files {
            if is_effect_script_path(path) {
                continue;
            }
            let Some(text) = file.text.as_deref() else {
                continue;
            };
            if let Some(range) = locate(text) {
                return Some((path.clone(), range));
            }
        }
        None
    }
}

pub(crate) struct AnalysisImportResolver<'a> {
    pub(crate) files: &'a IndexMap<Utf8PathBuf, AnalyzedFile>,
    pub(crate) scripts: &'a IndexMap<String, EffectScriptAnalysis>,
}

impl AnalysisImportResolver<'_> {
    fn import_paths_for_alias(
        &mut self,
        source_path: &Utf8PathBuf,
        alias: &str,
        reference: &SymbolRef,
    ) -> Result<Vec<Utf8PathBuf>, LowerError> {
        let analyzed = self
            .files
            .get(source_path)
            .ok_or_else(|| LowerError::Import {
                reference: reference.raw().to_string(),
                message: format!("file `{}` was not loaded", source_path),
            })?;
        let file = analyzed.file.as_ref().ok_or_else(|| LowerError::Import {
            reference: reference.raw().to_string(),
            message: format!("file `{}` did not parse", source_path),
        })?;
        let imports = file
            .imports
            .iter()
            .filter(|import| import.alias == alias)
            .map(|import| import.from.clone())
            .collect::<Vec<_>>();
        if imports.is_empty() {
            return Err(LowerError::Import {
                reference: reference.raw().to_string(),
                message: format!("alias `{alias}` was not imported"),
            });
        }
        if imports.len() > 1 {
            return Err(LowerError::Import {
                reference: reference.raw().to_string(),
                message: format!("alias `{alias}` is imported more than once"),
            });
        }
        let import_path = resolve_import_path(source_path, &imports[0]);
        if self.files.contains_key(&import_path) {
            return Ok(vec![import_path]);
        }
        let mut paths = self
            .files
            .keys()
            .filter(|path| path.parent() == Some(import_path.as_path()) && is_dawn_path(path))
            .cloned()
            .collect::<Vec<_>>();
        paths.sort();
        if paths.is_empty() {
            return Err(LowerError::Import {
                reference: reference.raw().to_string(),
                message: format!("import path `{}` was not loaded", import_path),
            });
        }
        Ok(paths)
    }
}

impl SymbolResolver for AnalysisImportResolver<'_> {
    fn resolve_object(
        &mut self,
        source_path: &Utf8PathBuf,
        reference: &SymbolRef,
        _expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError> {
        if reference.alias().is_none() {
            let analyzed = self
                .files
                .get(source_path)
                .ok_or_else(|| LowerError::Import {
                    reference: reference.raw().to_string(),
                    message: format!("file `{}` was not loaded", source_path),
                })?;
            let file = analyzed.file.as_ref().ok_or_else(|| LowerError::Import {
                reference: reference.raw().to_string(),
                message: format!("file `{}` did not parse", source_path),
            })?;
            let object = select_referenced_object(file, reference)?;
            return Ok(ResolvedImport {
                source_path: source_path.clone(),
                object,
            });
        }

        let mut matches = Vec::new();
        for import_path in
            self.import_paths_for_alias(source_path, reference.alias().unwrap(), reference)?
        {
            let analyzed = self
                .files
                .get(&import_path)
                .ok_or_else(|| LowerError::Import {
                    reference: reference.raw().to_string(),
                    message: format!("file `{}` was not loaded", import_path),
                })?;
            let file = analyzed.file.as_ref().ok_or_else(|| LowerError::Import {
                reference: reference.raw().to_string(),
                message: format!("file `{}` did not parse", import_path),
            })?;
            if let Some(object) = file.get(reference.name().as_str()) {
                matches.push(ResolvedImport {
                    source_path: import_path,
                    object: object.clone(),
                });
            }
        }
        single_match(matches, reference)
    }

    fn resolve_effect(
        &mut self,
        source_path: &Utf8PathBuf,
        reference: &SymbolRef,
    ) -> Result<ResolvedEffectImport, LowerError> {
        let Some(alias) = reference.alias() else {
            return Err(LowerError::Import {
                reference: reference.raw().to_string(),
                message: "effect references must use an imported alias".to_string(),
            });
        };
        let mut matches = Vec::new();
        for import_path in self.import_paths_for_alias(source_path, alias, reference)? {
            let Some(script) = self.scripts.get(&import_path.to_slash_string()) else {
                continue;
            };
            let Ok(compiled) = &script.result else {
                continue;
            };
            if compiled.name == reference.name().as_str() {
                matches.push(ResolvedEffectImport {
                    source_path: import_path,
                });
            }
        }
        single_match(matches, reference)
    }
}

fn single_match<T>(mut matches: Vec<T>, reference: &SymbolRef) -> Result<T, LowerError> {
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(LowerError::Import {
            reference: reference.raw().to_string(),
            message: format!("symbol `{}` was not found", reference.name().as_str()),
        }),
        _ => Err(LowerError::Import {
            reference: reference.raw().to_string(),
            message: format!(
                "symbol `{}` is exported more than once",
                reference.name().as_str()
            ),
        }),
    }
}

fn script_range(range: crate::effect_script::SourceRange) -> TextRange {
    TextRange {
        start: TextPosition {
            line: range.start.line,
            character: range.start.character,
        },
        end: TextPosition {
            line: range.end.line,
            character: range.end.character,
        },
    }
}

fn script_diagnostics(
    path: &Utf8PathBuf,
    result: &Result<CompiledEffect, Vec<ScriptDiagnostic>>,
) -> Vec<ProjectDiagnostic> {
    result
        .as_ref()
        .err()
        .into_iter()
        .flatten()
        .map(|diagnostic| ProjectDiagnostic {
            path: path.clone(),
            range: diagnostic.range.map(script_range),
            severity: DiagnosticSeverity::Error,
            code: DiagnosticCode::Script,
            message: diagnostic.message.clone(),
        })
        .collect()
}

fn object_symbol_ref_range(
    file: &AnalyzedFile,
    object_path: &YamlPath,
    object: &DawnObject<Authored>,
    reference: &str,
) -> Option<TextRange> {
    match object {
        DawnObject::Project(project) => inline_ref_range(
            file,
            object_path.clone().field("display"),
            &project.display,
            reference,
        )
        .or_else(|| {
            project
                .sequences
                .iter()
                .enumerate()
                .find_map(|(index, sequence)| {
                    inline_ref_range(
                        file,
                        object_path.clone().field("sequences").index(index),
                        sequence,
                        reference,
                    )
                })
        }),
        DawnObject::Display(display) => display
            .controllers
            .iter()
            .enumerate()
            .find_map(|(index, controller)| {
                inline_ref_range(
                    file,
                    object_path.clone().field("controllers").index(index),
                    controller,
                    reference,
                )
            })
            .or_else(|| {
                inline_ref_range(
                    file,
                    object_path.clone().field("patch"),
                    &display.patch,
                    reference,
                )
            })
            .or_else(|| {
                inline_ref_range(
                    file,
                    object_path.clone().field("layout"),
                    &display.layout,
                    reference,
                )
            }),
        DawnObject::Layout(layout) => {
            layout
                .fixtures
                .iter()
                .enumerate()
                .find_map(|(index, fixture)| {
                    inline_ref_range(
                        file,
                        object_path
                            .clone()
                            .field("fixtures")
                            .index(index)
                            .field("fixture"),
                        &fixture.fixture,
                        reference,
                    )
                })
        }
        DawnObject::Sequence(sequence) => sequence
            .effects
            .iter()
            .enumerate()
            .find_map(|(index, effect)| {
                inline_script_ref_range(
                    file,
                    object_path
                        .clone()
                        .field("effects")
                        .index(index)
                        .field("script"),
                    &effect.script,
                    reference,
                )
                .or_else(|| {
                    effect
                        .params
                        .iter()
                        .enumerate()
                        .find_map(|(param_index, (name, param))| {
                            effect_param_symbol_ref_range(
                                file,
                                object_path
                                    .clone()
                                    .field("effects")
                                    .index(index)
                                    .field("params")
                                    .field(name.clone()),
                                param_index,
                                param,
                                reference,
                            )
                        })
                })
            })
            .or_else(|| {
                sequence
                    .automation_clips
                    .iter()
                    .enumerate()
                    .find_map(|(index, clip)| {
                        inline_ref_range(
                            file,
                            object_path
                                .clone()
                                .field("automation_clips")
                                .index(index)
                                .field("curve"),
                            &clip.curve,
                            reference,
                        )
                    })
            }),
        _ => None,
    }
}

fn duplicate_fixture_id_in_object(
    file: &AnalyzedFile,
    object_path: YamlPath,
    object: &DawnObject<Authored>,
    id: FixtureId,
    seen: &mut bool,
) -> Option<TextRange> {
    match object {
        DawnObject::Project(project) => {
            let InlineOrRef::Inline(display) = &project.display else {
                return None;
            };
            duplicate_fixture_id_in_display(file, object_path.field("display"), display, id, seen)
        }
        DawnObject::Display(display) => {
            duplicate_fixture_id_in_display(file, object_path, display, id, seen)
        }
        DawnObject::Layout(layout) => {
            duplicate_fixture_id_in_layout(file, object_path, layout, id, seen)
        }
        _ => None,
    }
}

fn duplicate_fixture_id_in_display(
    file: &AnalyzedFile,
    display_path: YamlPath,
    display: &Display<Authored>,
    id: FixtureId,
    seen: &mut bool,
) -> Option<TextRange> {
    let InlineOrRef::Inline(layout) = &display.layout else {
        return None;
    };
    duplicate_fixture_id_in_layout(file, display_path.field("layout"), layout, id, seen)
}

fn duplicate_fixture_id_in_layout(
    file: &AnalyzedFile,
    layout_path: YamlPath,
    layout: &Layout<Authored>,
    id: FixtureId,
    seen: &mut bool,
) -> Option<TextRange> {
    for (index, fixture) in layout.fixtures.iter().enumerate() {
        if fixture.id == id {
            if *seen {
                return file.source_map.as_ref()?.value_range(
                    layout_path
                        .clone()
                        .field("fixtures")
                        .index(index)
                        .field("id"),
                );
            }
            *seen = true;
        }
    }
    None
}

fn group_ref_in_object(
    file: &AnalyzedFile,
    object_path: YamlPath,
    object: &DawnObject<Authored>,
    name: &str,
) -> Option<TextRange> {
    match object {
        DawnObject::Project(project) => {
            project
                .sequences
                .iter()
                .enumerate()
                .find_map(|(index, sequence)| {
                    let InlineOrRef::Inline(sequence) = sequence else {
                        return None;
                    };
                    inline_sequence_group_ref_range(
                        file,
                        object_path.clone().field("sequences").index(index),
                        sequence,
                        name,
                    )
                })
        }
        DawnObject::Sequence(sequence) => {
            inline_sequence_group_ref_range(file, object_path, sequence, name)
        }
        _ => None,
    }
}

fn inline_sequence_group_ref_range(
    file: &AnalyzedFile,
    sequence_path: YamlPath,
    sequence: &Sequence<Authored>,
    name: &str,
) -> Option<TextRange> {
    let source_map = file.source_map.as_ref()?;
    for (effect_index, effect) in sequence.effects.iter().enumerate() {
        if matches!(&effect.target, EffectTarget::Group { name: group } if group.as_str() == name) {
            return source_map.value_range(
                sequence_path
                    .field("effects")
                    .index(effect_index)
                    .field("target")
                    .field("name"),
            );
        }
    }
    None
}

fn inline_ref_range<T>(
    file: &AnalyzedFile,
    path: YamlPath,
    value: &InlineOrRef<T>,
    reference: &str,
) -> Option<TextRange> {
    match value {
        InlineOrRef::Ref(symbol) if symbol.raw() == reference => {
            file.source_map.as_ref()?.value_range(path)
        }
        _ => None,
    }
}

fn inline_script_ref_range(
    file: &AnalyzedFile,
    path: YamlPath,
    value: &InlineScriptOrRef,
    reference: &str,
) -> Option<TextRange> {
    match value {
        InlineScriptOrRef::Ref(symbol) if symbol.raw() == reference => {
            file.source_map.as_ref()?.value_range(path)
        }
        _ => None,
    }
}

fn effect_param_symbol_ref_range(
    file: &AnalyzedFile,
    path: YamlPath,
    _param_index: usize,
    param: &EffectParam<Authored>,
    reference: &str,
) -> Option<TextRange> {
    match param {
        EffectParam::Curve { curve } => {
            inline_ref_range(file, path.field("curve"), curve, reference)
        }
        _ => None,
    }
}

fn locate_effect_id(text: &str, effect_id: u32) -> Option<TextRange> {
    let (start, _) = find_effect_block(text, effect_id)?;
    line_value_range(text, start, "id", Some(&effect_id.to_string()))
}

fn locate_effect_params_block(text: &str, effect_id: u32) -> Option<TextRange> {
    let (start, end) = find_effect_block(text, effect_id)?;
    let lines = text.lines().collect::<Vec<_>>();
    for line_index in start..end {
        if field_name(lines[line_index]) == Some("params") {
            return line_value_range(text, line_index, "params", None);
        }
    }
    locate_effect_id(text, effect_id)
}

fn locate_effect_param(text: &str, effect_id: u32, param_name: &str) -> Option<TextRange> {
    let (line_index, _) = find_effect_param_block(text, effect_id, param_name)?;
    line_value_range(text, line_index, param_name, None)
}

fn locate_effect_param_field(
    text: &str,
    effect_id: u32,
    param_name: &str,
    field: &str,
    value: Option<&str>,
) -> Option<TextRange> {
    let (start, end) = find_effect_param_block(text, effect_id, param_name)?;
    let lines = text.lines().collect::<Vec<_>>();
    for line_index in start..end {
        if field_name(lines[line_index]) == Some(field) {
            return line_value_range(text, line_index, field, value);
        }
    }
    locate_effect_param(text, effect_id, param_name)
}

fn locate_effect_param_value(
    text: &str,
    effect_id: u32,
    param_name: &str,
    value: &str,
) -> Option<TextRange> {
    let (start, end) = find_effect_param_block(text, effect_id, param_name)?;
    let lines = text.lines().collect::<Vec<_>>();
    for (line_index, line) in lines.iter().enumerate().take(end).skip(start) {
        if let Some(column) = line.find(value) {
            return Some(TextRange {
                start: TextPosition {
                    line: line_index as u32,
                    character: column as u32,
                },
                end: TextPosition {
                    line: line_index as u32,
                    character: column.saturating_add(value.len()) as u32,
                },
            });
        }
    }
    locate_effect_param(text, effect_id, param_name)
}

fn locate_inline_script_range(
    text: &str,
    effect_id: u32,
    source_range: crate::effect_script::SourceRange,
) -> Option<TextRange> {
    let (start, end) = find_effect_block(text, effect_id)?;
    let lines = text.lines().collect::<Vec<_>>();
    for line_index in start..end {
        let line = lines[line_index];
        if field_name(line) != Some("script") {
            continue;
        }
        let script_indent = line_indent(line);
        let after_colon = line.split_once(':')?.1.trim_start();
        if after_colon.starts_with('|') || after_colon.starts_with('>') {
            let mut content_start = None;
            let mut content_end = line_index + 1;
            let mut content_indent = usize::MAX;
            for (candidate_index, candidate) in
                lines.iter().enumerate().take(end).skip(line_index + 1)
            {
                let trimmed = candidate.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let indent = line_indent(candidate);
                if indent <= script_indent {
                    break;
                }
                content_start.get_or_insert(candidate_index);
                content_end = candidate_index;
                content_indent = content_indent.min(indent);
            }
            let content_start = content_start?;
            let content_indent = content_indent.min(usize::MAX - 1);
            return Some(TextRange {
                start: yaml_position_for_script_position(
                    source_range.start,
                    content_start,
                    content_end,
                    content_indent,
                ),
                end: yaml_position_for_script_position(
                    source_range.end,
                    content_start,
                    content_end,
                    content_indent,
                ),
            });
        }

        if !after_colon.is_empty() {
            return line_value_range(text, line_index, "script", None);
        }
    }
    None
}

fn yaml_position_for_script_position(
    position: crate::effect_script::SourcePosition,
    content_start: usize,
    content_end: usize,
    content_indent: usize,
) -> TextPosition {
    let line = content_start
        .saturating_add(position.line as usize)
        .min(content_end);
    TextPosition {
        line: line as u32,
        character: content_indent.saturating_add(position.character as usize) as u32,
    }
}

fn find_effect_param_block(text: &str, effect_id: u32, param_name: &str) -> Option<(usize, usize)> {
    let (start, end) = find_effect_block(text, effect_id)?;
    let lines = text.lines().collect::<Vec<_>>();
    let mut params_start = None;
    let mut params_indent = 0usize;
    for (line_index, line) in lines.iter().enumerate().take(end).skip(start) {
        if field_name(line) == Some("params") {
            params_start = Some(line_index + 1);
            params_indent = line_indent(line);
            break;
        }
    }
    for line_index in params_start?..end {
        let trimmed = lines[line_index].trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if line_indent(lines[line_index]) <= params_indent {
            break;
        }
        if field_name(lines[line_index]) != Some(param_name) {
            continue;
        }
        let indent = line_indent(lines[line_index]);
        let mut block_end = end;
        for (candidate_index, candidate) in lines.iter().enumerate().take(end).skip(line_index + 1)
        {
            let trimmed = candidate.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            if line_indent(candidate) <= indent {
                block_end = candidate_index;
                break;
            }
        }
        return Some((line_index, block_end));
    }
    None
}

fn find_effect_block(text: &str, effect_id: u32) -> Option<(usize, usize)> {
    let lines = text.lines().collect::<Vec<_>>();
    for (line_index, line) in lines.iter().enumerate() {
        if !is_effect_id_line(line, effect_id) {
            continue;
        }
        let indent = line_indent(line);
        let mut end = lines.len();
        for (candidate_index, candidate) in lines.iter().enumerate().skip(line_index + 1) {
            let trimmed = candidate.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            if line_indent(candidate) <= indent {
                end = candidate_index;
                break;
            }
        }
        if lines[line_index..end]
            .iter()
            .any(|candidate| field_name(candidate) == Some("script"))
        {
            return Some((line_index, end));
        }
    }
    None
}

fn is_effect_id_line(line: &str, effect_id: u32) -> bool {
    let trimmed = line.trim_start();
    let field = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    field
        .strip_prefix("id:")
        .is_some_and(|value| value.trim() == effect_id.to_string())
}

fn field_name(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let field = trimmed.strip_prefix("- ").unwrap_or(trimmed);
    let (name, _) = field.split_once(':')?;
    Some(name.trim())
}

fn line_value_range(
    text: &str,
    line_index: usize,
    field: &str,
    value: Option<&str>,
) -> Option<TextRange> {
    let line = text.lines().nth(line_index)?;
    let field_column = line.find(field)?;
    let value_column = value
        .and_then(|value| line.find(value).map(|column| (column, value.len())))
        .or_else(|| {
            line.find(':').map(|colon| {
                let start = colon.saturating_add(1).saturating_add(
                    line[colon.saturating_add(1)..].len()
                        - line[colon.saturating_add(1)..].trim_start().len(),
                );
                if start < line.len() {
                    (start, line[start..].trim_end().len())
                } else {
                    (field_column, field.len())
                }
            })
        })
        .unwrap_or((field_column, field.len()));
    let (start_column, len) = if value_column.1 == 0 {
        (field_column, field.len())
    } else {
        value_column
    };
    Some(TextRange {
        start: TextPosition {
            line: line_index as u32,
            character: start_column as u32,
        },
        end: TextPosition {
            line: line_index as u32,
            character: start_column.saturating_add(len) as u32,
        },
    })
}

fn line_indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn validate_sequence_marks(
    path: &Utf8PathBuf,
    source_map: &YamlSourceMap,
    file: &DawnFile,
    diagnostics: &mut Vec<ProjectDiagnostic>,
) {
    for (object_key, object) in file {
        let DawnObject::Sequence(sequence) = object else {
            continue;
        };
        let mut keys = HashSet::new();
        for (collection_index, collection) in sequence.mark_collections.iter().enumerate() {
            if !is_mark_collection_key(&collection.key) {
                diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: source_map.value_range(
                        YamlPath::root()
                            .field(object_key.clone())
                            .field("mark_collections")
                            .index(collection_index)
                            .field("key"),
                    ),
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Sequence,
                    message: format!(
                        "sequence `{object_key}` mark collection key `{}` must match [a-z][a-z0-9_]*",
                        collection.key
                    ),
                });
            }
            if !keys.insert(collection.key.as_str()) {
                diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: source_map.value_range(
                        YamlPath::root()
                            .field(object_key.clone())
                            .field("mark_collections")
                            .index(collection_index)
                            .field("key"),
                    ),
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Sequence,
                    message: format!(
                        "sequence `{object_key}` has duplicate mark collection key `{}`",
                        collection.key
                    ),
                });
            }
            if let Err(error) = Color::parse(&collection.color) {
                diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: source_map.value_range(
                        YamlPath::root()
                            .field(object_key.clone())
                            .field("mark_collections")
                            .index(collection_index)
                            .field("color"),
                    ),
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Sequence,
                    message: format!(
                        "sequence `{object_key}` mark collection `{}` color is invalid: {error}",
                        collection.key
                    ),
                });
            }
        }
    }
}

fn validate_module_declarations(
    path: &Utf8PathBuf,
    source_map: &YamlSourceMap,
    file: &DawnFile,
    diagnostics: &mut Vec<ProjectDiagnostic>,
) {
    let mut aliases = HashSet::new();
    for (import_index, import) in file.imports.iter().enumerate() {
        if let Err(error) = validate_identifier(&import.alias, "import alias") {
            diagnostics.push(ProjectDiagnostic {
                path: path.clone(),
                range: source_map.value_range(
                    YamlPath::root()
                        .field("imports")
                        .index(import_index)
                        .field("as"),
                ),
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::Import,
                message: error,
            });
        }
        if !aliases.insert(import.alias.as_str()) {
            diagnostics.push(ProjectDiagnostic {
                path: path.clone(),
                range: source_map.value_range(
                    YamlPath::root()
                        .field("imports")
                        .index(import_index)
                        .field("as"),
                ),
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::Import,
                message: format!("duplicate import alias `{}`", import.alias),
            });
        }
    }

    for key in file.objects.keys() {
        if let Err(error) = validate_identifier(key, "exported object name") {
            diagnostics.push(ProjectDiagnostic {
                path: path.clone(),
                range: source_map.key_range(YamlPath::root().field(key.clone())),
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::Import,
                message: error,
            });
        }
    }
}

fn is_mark_collection_key(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_lowercase())
        && chars.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
}

fn is_effect_script_path(path: &Utf8PathBuf) -> bool {
    path.file_name()
        .is_some_and(|name| name.ends_with(".effect.dawn"))
}

fn is_dawn_path(path: &Utf8PathBuf) -> bool {
    path.file_name().is_some_and(|name| name.ends_with(".dawn"))
}
