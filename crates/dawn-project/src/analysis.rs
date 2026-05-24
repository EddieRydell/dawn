use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use serde::Serialize;

use crate::fs::ProjectFs;
use crate::lower::{lower_project, select_imported_object, LowerError, ResolvedImport};
use crate::model::*;
use crate::path::{resolve_import_file_path, ProjectPath};

#[derive(Debug, Clone)]
pub struct ProjectAnalysis {
    pub root_path: ProjectPath,
    pub project_key: String,
    pub files: IndexMap<ProjectPath, AnalyzedFile>,
    pub diagnostics: Vec<ProjectDiagnostic>,
    pub resolved: Option<ResolvedProject>,
}

impl ProjectAnalysis {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
    }
}

#[derive(Debug, Clone)]
pub struct AnalyzedFile {
    pub path: ProjectPath,
    pub text: Option<String>,
    pub file: Option<DawnFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDiagnostic {
    pub path: ProjectPath,
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
    pub path: ProjectPath,
    pub content: String,
}
pub fn analyze_project(
    fs: &ProjectFs,
    project_path: ProjectPath,
    project_key: &str,
) -> ProjectAnalysis {
    analyze_project_with_overlays(fs, project_path, Some(project_key), Vec::new())
}

pub fn analyze_project_with_overlays(
    fs: &ProjectFs,
    project_path: ProjectPath,
    project_key: Option<&str>,
    overlays: Vec<ProjectOverlay>,
) -> ProjectAnalysis {
    let root_path = project_path;
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
                    Ok(project) => resolved = Some(project),
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
        diagnostics: session.diagnostics,
        resolved,
    }
}

fn infer_project_key(root_path: &ProjectPath, session: &mut AnalysisSession) -> Option<String> {
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
    fs: ProjectFs,
    files: IndexMap<ProjectPath, AnalyzedFile>,
    diagnostics: Vec<ProjectDiagnostic>,
    visiting: HashSet<ProjectPath>,
    overlays: HashMap<ProjectPath, String>,
}

impl AnalysisSession {
    fn new(fs: ProjectFs, overlays: Vec<ProjectOverlay>) -> Self {
        Self {
            fs,
            files: IndexMap::new(),
            diagnostics: Vec::new(),
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

    fn visit_file(&mut self, path: ProjectPath) {
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
                    message: format!("failed to read `{}`: {source}", path.display()),
                });
                self.files.insert(
                    path.clone(),
                    AnalyzedFile {
                        path: path.clone(),
                        text: None,
                        file: None,
                    },
                );
                self.visiting.remove(&path);
                return;
            }
        };

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
            },
        );

        for import in imports {
            let import_path = match resolve_import_file_path(&path, import.path()) {
                Ok(import_path) => import_path,
                Err(message) => {
                    self.diagnostics.push(ProjectDiagnostic {
                        path: path.clone(),
                        range: import_range(&text, &import),
                        severity: DiagnosticSeverity::Error,
                        code: DiagnosticCode::Import,
                        message: format!("invalid import `{}`: {message}", import.raw()),
                    });
                    continue;
                }
            };
            if !self.can_load_file(&import_path) {
                self.diagnostics.push(ProjectDiagnostic {
                    path: path.clone(),
                    range: import_range(&text, &import),
                    severity: DiagnosticSeverity::Error,
                    code: DiagnosticCode::Import,
                    message: format!(
                        "failed to read import `{}`: file `{}` was not found",
                        import.raw(),
                        import_path.display()
                    ),
                });
                continue;
            }

            self.visit_file(import_path);
        }

        self.visiting.remove(&path);
    }

    fn can_load_file(&self, path: &ProjectPath) -> bool {
        self.overlays.contains_key(path) || self.fs.is_file(path)
    }

    fn locate_lower_error(
        &self,
        root_path: &ProjectPath,
        error: &LowerError,
    ) -> (ProjectPath, Option<TextRange>) {
        let token = match error {
            LowerError::MissingProject { key } => Some(key.as_str()),
            LowerError::WrongObjectKind { key, .. } => Some(key.as_str()),
            LowerError::WrongImportedObjectKind { import, .. } => Some(import.as_str()),
            LowerError::Import { import, .. } => Some(import.as_str()),
            LowerError::DuplicateFixtureId { id } => Some(id.as_str()),
            LowerError::UnknownFixture { id } => Some(id.as_str()),
            LowerError::DuplicateControllerName { name } => Some(name.as_str()),
            LowerError::UnknownController { name } => Some(name.as_str()),
            LowerError::DuplicateGroupName { name } => Some(name.as_str()),
            LowerError::UnknownGroup { name } => Some(name.as_str()),
            LowerError::DuplicateSequenceEffectId { id } => Some(id.as_str()),
            LowerError::UnknownSequenceEffect { id } => Some(id.as_str()),
        };

        if let Some(token) = token {
            if let Some((path, range)) = self.find_token(root_path, token) {
                return (path, Some(range));
            }
        }

        (root_path.clone(), None)
    }

    fn find_token(
        &self,
        preferred_path: &ProjectPath,
        token: &str,
    ) -> Option<(ProjectPath, TextRange)> {
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
}

pub(crate) struct AnalysisImportResolver<'a> {
    pub(crate) files: &'a IndexMap<ProjectPath, AnalyzedFile>,
}

impl AnalysisImportResolver<'_> {
    pub(crate) fn resolve(
        &mut self,
        source_path: &ProjectPath,
        import: &ImportRef,
        _expected: ObjectKind,
    ) -> Result<ResolvedImport, LowerError> {
        let import_path =
            resolve_import_file_path(source_path, import.path()).map_err(|message| {
                LowerError::Import {
                    import: import.raw().to_string(),
                    message,
                }
            })?;
        let analyzed = self
            .files
            .get(&import_path)
            .ok_or_else(|| LowerError::Import {
                import: import.raw().to_string(),
                message: format!("file `{}` was not loaded", import_path.display()),
            })?;
        let file = analyzed.file.as_ref().ok_or_else(|| LowerError::Import {
            import: import.raw().to_string(),
            message: format!("file `{}` did not parse", import_path.display()),
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
