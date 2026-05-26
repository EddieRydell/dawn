use std::collections::{BTreeMap, HashMap, HashSet};

use indexmap::IndexMap;
use serde::Serialize;

use crate::effect_script::{
    compile as compile_effect_script, CompiledEffect, ParamDefault, RuntimeValue, ScriptDiagnostic,
};
use crate::fs::WorkspaceFs;
use crate::lower::{lower_project, select_imported_object, LowerError, ResolvedImport};
use crate::model::*;
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
            .filter_map(|param| match &param.default {
                Some(ParamDefault::Value(value)) => Some((param.name.clone(), value.clone())),
                None => None,
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct AnalyzedFile {
    pub path: Utf8PathBuf,
    pub text: Option<String>,
    pub file: Option<DawnFile>,
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
                };
                match lower_project(
                    root_file,
                    project_key,
                    &root_path,
                    |source_path, import, expected| loader.resolve(source_path, import, expected),
                ) {
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
    let Some(root_file) = session
        .files
        .get(root_path)
        .and_then(|analyzed| analyzed.file.as_ref())
    else {
        return None;
    };

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
                    script: Some(script),
                },
            );
            self.visiting.remove(&path);
            return;
        }

        let file = match serde_yaml::from_str::<DawnFile>(&text) {
            Ok(file) => Some(file),
            Err(source) => {
                self.diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: yaml_error_range(&source),
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Yaml,
                    message: source.to_string(),
                });
                None
            }
        };

        let imports = file
            .as_ref()
            .map(collect_file_imports)
            .unwrap_or_else(Vec::new);
        self.files.insert(
            path.clone(),
            AnalyzedFile {
                path: path.clone(),
                text: Some(text.clone()),
                file,
                script: None,
            },
        );

        for import in imports {
            self.visit_import(&path, &text, import);
        }

        self.visiting.remove(&path);
    }

    fn can_load_file(&self, path: &Utf8PathBuf) -> bool {
        self.overlays.contains_key(path) || self.fs.is_file(path)
    }

    fn visit_import(&mut self, source_path: &Utf8PathBuf, text: &str, import: ImportRef) {
        let import_path = resolve_import_path(source_path, import.path());
        if !self.can_load_file(&import_path) {
            self.diagnostics.push(ProjectDiagnostic {
                path: source_path.clone(),
                range: import_range(text, &import),
                severity: DiagnosticSeverity::Error,
                code: DiagnosticCode::Import,
                message: format!(
                    "failed to read import `{}`: file `{}` was not found",
                    import.raw(),
                    import_path
                ),
            });
            return;
        }

        self.visit_file(import_path);
    }

    fn locate_lower_error(
        &self,
        root_path: &Utf8PathBuf,
        error: &LowerError,
    ) -> (Utf8PathBuf, Option<TextRange>) {
        let token = match error {
            LowerError::MissingProject { key } => Some(key.clone()),
            LowerError::WrongObjectKind { key, .. } => Some(key.clone()),
            LowerError::WrongImportedObjectKind { import, .. } => Some(import.clone()),
            LowerError::Import { import, .. } => Some(import.clone()),
            LowerError::DuplicateFixtureId { id } => Some(id.to_string()),
            LowerError::EmptyFixtureName => None,
            LowerError::DuplicateFixtureName { name } => Some(name.clone()),
            LowerError::UnknownFixture { id } => Some(id.to_string()),
            LowerError::DuplicateControllerName { name } => Some(name.clone()),
            LowerError::UnknownController { name } => Some(name.clone()),
            LowerError::DuplicateGroupName { name } => Some(name.clone()),
            LowerError::UnknownGroup { name } => Some(name.clone()),
            LowerError::DuplicateLayoutTargetOrderEntry { name, .. } => Some(name.clone()),
            LowerError::MissingLayoutTargetOrderEntry { name, .. } => Some(name.clone()),
            LowerError::UnknownLayoutTargetOrderEntry { name, .. } => Some(name.clone()),
            LowerError::DuplicateSequenceEffectId { id } => Some(id.to_string()),
            LowerError::UnknownSequenceEffect { id } => Some(id.to_string()),
            LowerError::AutomationCurveType { id, .. } => Some(id.to_string()),
        };

        if let Some(token) = token {
            if let Some((path, range)) = self.find_token(root_path, &token) {
                return (path, Some(range));
            }
        }

        (root_path.clone(), None)
    }

    fn find_token(
        &self,
        preferred_path: &Utf8PathBuf,
        token: &str,
    ) -> Option<(Utf8PathBuf, TextRange)> {
        if let Some(file) = self.files.get(preferred_path) {
            if let Some(text) = file.text.as_deref() {
                if let Some(range) = find_text_range(text, token) {
                    return Some((preferred_path.clone(), range));
                }
            }
        }

        for (path, file) in &self.files {
            if path == preferred_path {
                continue;
            }
            if let Some(text) = file.text.as_deref() {
                if let Some(range) = find_text_range(text, token) {
                    return Some((path.clone(), range));
                }
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
                        for diagnostic in script_diagnostics(root_path, &result) {
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
                    self.validate_effect_params(root_path, &effect.id, &script, &effect.params);
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
    ) {
        for name in params.keys() {
            if script.param(name).is_none() {
                self.diagnostics.push(ProjectDiagnostic {
                    path: root_path.clone(),
                    range: None,
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
                    self.validate_effect_param_options(root_path, effect_id, script, schema, param);
                }
                Some(_) => self.diagnostics.push(ProjectDiagnostic {
                    path: root_path.clone(),
                    range: None,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Script,
                    message: format!(
                        "effect `{effect_id}` parameter `{}` must be {}",
                        schema.name, schema.value_type
                    ),
                }),
                None if schema.default.is_some() => {}
                None => self.diagnostics.push(ProjectDiagnostic {
                    path: root_path.clone(),
                    range: None,
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Script,
                    message: format!(
                        "effect `{effect_id}` is missing required parameter `{}`",
                        schema.name
                    ),
                }),
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
    ) {
        match param {
            EffectParam::Enum { value } if !schema.options.contains(value) => {
                self.diagnostics.push(ProjectDiagnostic {
                    path: root_path.clone(),
                    range: None,
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
                        self.diagnostics.push(ProjectDiagnostic {
                            path: root_path.clone(),
                            range: None,
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
            _ => {}
        }
    }
}

pub(crate) struct AnalysisImportResolver<'a> {
    pub(crate) files: &'a IndexMap<Utf8PathBuf, AnalyzedFile>,
}

impl AnalysisImportResolver<'_> {
    pub(crate) fn resolve(
        &mut self,
        source_path: &Utf8PathBuf,
        import: &ImportRef,
        _expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError> {
        let import_path = resolve_import_path(source_path, import.path());
        let analyzed = self
            .files
            .get(&import_path)
            .ok_or_else(|| LowerError::Import {
                import: import.raw().to_string(),
                message: format!("file `{}` was not loaded", import_path),
            })?;
        let file = analyzed.file.as_ref().ok_or_else(|| LowerError::Import {
            import: import.raw().to_string(),
            message: format!("file `{}` did not parse", import_path),
        })?;
        let object = select_imported_object(file, import)?;

        Ok(ResolvedImport {
            source_path: import_path,
            object,
        })
    }
}

fn yaml_error_range(error: &serde_yaml::Error) -> Option<TextRange> {
    error.location().map(|location| {
        let line = location.line().saturating_sub(1) as u32;
        let character = location.column().saturating_sub(1) as u32;
        TextRange {
            start: TextPosition { line, character },
            end: TextPosition {
                line,
                character: character.saturating_add(1),
            },
        }
    })
}

fn find_text_range(text: &str, needle: &str) -> Option<TextRange> {
    for (line_index, line) in text.lines().enumerate() {
        if let Some(column) = line.find(needle) {
            return Some(TextRange {
                start: TextPosition {
                    line: line_index as u32,
                    character: column as u32,
                },
                end: TextPosition {
                    line: line_index as u32,
                    character: column.saturating_add(needle.len()) as u32,
                },
            });
        }
    }
    None
}

fn import_range(text: &str, import: &ImportRef) -> Option<TextRange> {
    find_text_range(text, import.raw())
        .or_else(|| find_text_range(text, &import.path().to_slash_string()))
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

fn is_effect_script_path(path: &Utf8PathBuf) -> bool {
    path.file_name()
        .is_some_and(|name| name.ends_with(".effect.dawn"))
}

fn collect_file_imports(file: &DawnFile) -> Vec<ImportRef> {
    let mut imports = Vec::new();
    for object in file.values() {
        collect_object_imports(object, &mut imports);
    }
    imports
}

fn collect_object_imports(object: &DawnObject<Authored>, imports: &mut Vec<ImportRef>) {
    match object {
        DawnObject::Project(project) => collect_project_imports(project, imports),
        DawnObject::Display(display) => collect_display_imports(display, imports),
        DawnObject::Controller(_) => {}
        DawnObject::Layout(layout) => collect_layout_imports(layout, imports),
        DawnObject::Fixture(_) => {}
        DawnObject::Patch(_) => {}
        DawnObject::Sequence(sequence) => collect_sequence_imports(sequence, imports),
        DawnObject::Curve(_) => {}
    }
}

fn collect_project_imports(project: &Project<Authored>, imports: &mut Vec<ImportRef>) {
    match &project.display {
        InlineOrImport::Inline(display) => collect_display_imports(display, imports),
        InlineOrImport::Import { import } => imports.push(import.clone()),
    }
    for sequence in &project.sequences {
        match sequence {
            InlineOrImport::Inline(sequence) => collect_sequence_imports(sequence, imports),
            InlineOrImport::Import { import } => imports.push(import.clone()),
        }
    }
}

fn collect_display_imports(display: &Display<Authored>, imports: &mut Vec<ImportRef>) {
    for controller in &display.controllers {
        if let InlineOrImport::Import { import } = controller {
            imports.push(import.clone());
        }
    }
    match &display.patch {
        InlineOrImport::Inline(_) => {}
        InlineOrImport::Import { import } => imports.push(import.clone()),
    }
    match &display.layout {
        InlineOrImport::Inline(layout) => collect_layout_imports(layout, imports),
        InlineOrImport::Import { import } => imports.push(import.clone()),
    }
}

fn collect_sequence_imports(sequence: &Sequence<Authored>, imports: &mut Vec<ImportRef>) {
    for effect in &sequence.effects {
        if let InlineOrImport::Import { import } = &effect.script {
            imports.push(import.clone());
        }
        for param in effect.params.values() {
            if let EffectParam::Curve {
                curve: InlineOrImport::Import { import },
            } = param
            {
                imports.push(import.clone());
            }
        }
    }
    for clip in &sequence.automation_clips {
        if let InlineOrImport::Import { import } = &clip.curve {
            imports.push(import.clone());
        }
    }
}

fn collect_layout_imports(layout: &Layout<Authored>, imports: &mut Vec<ImportRef>) {
    for fixture in &layout.fixtures {
        if let InlineOrImport::Import { import } = &fixture.fixture {
            imports.push(import.clone());
        }
    }
}
