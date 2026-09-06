use super::*;

#[derive(Debug)]
pub enum GuiMutationError {
    Blocked(String),
    Invalid(String),
}

impl GuiMutationError {
    pub fn message(&self) -> &str {
        match self {
            Self::Blocked(message) | Self::Invalid(message) => message,
        }
    }
}

pub fn blocked(reason: impl Into<String>, diagnostics: Vec<ProjectDiagnostic>) -> GuiDocument {
    GuiDocument::Blocked {
        reason: reason.into(),
        diagnostics,
    }
}

pub fn project_gui_document(
    session: Option<&ProjectSession>,
    request: &GuiDocumentRequest,
) -> GuiDocument {
    let Some(session) = session else {
        return blocked("No project is loaded.", Vec::new());
    };
    let resolved = match resolve_request(session, request) {
        Ok(resolved) => resolved,
        Err(message) => {
            return blocked(
                message.clone(),
                vec![gui_diagnostic(&request.path, "gui.resolve", &message)],
            );
        }
    };
    if !session
        .source
        .is_project_owned(resolved.identity.document_id())
    {
        return blocked(
            "Imported dependency documents are read-only.",
            vec![gui_diagnostic(
                &request.path,
                "gui.read_only_dependency",
                "Imported dependency documents are read-only.",
            )],
        );
    }
    match request.view {
        DocumentViewId::Sequence => project_sequence(session, &resolved),
        DocumentViewId::Setup => project_setup(session, &resolved),
        DocumentViewId::Preview => project_layout(session, &resolved),
        DocumentViewId::Prop => project_fixture(session, &resolved),
        DocumentViewId::Text => blocked(
            "Text documents do not have a GUI projection.",
            vec![gui_diagnostic(
                &request.path,
                "gui.view",
                "Text documents do not have a GUI projection.",
            )],
        ),
    }
}

pub fn affected_paths(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<BTreeSet<String>, GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(session, &resolved)?;
    if matches!(request.view, DocumentViewId::Setup) {
        return Ok(session
            .source
            .documents
            .keys()
            .filter(|document| session.source.is_project_owned(document))
            .map(|document| document.path().to_string())
            .collect());
    }
    Ok(BTreeSet::from([resolved.identity.document().to_string()]))
}

pub(crate) struct ResolvedGuiObject {
    pub(crate) identity: SourceIdentity,
    kind: SourceObjectKind,
}

impl ResolvedGuiObject {
    pub(crate) fn source_ref(&self) -> GuiObjectRef {
        GuiObjectRef {
            module_id: self.identity.module_id().to_string(),
            path: self.identity.document().to_string(),
            object_key: self.identity.object().to_string(),
            kind: ObjectKind::from(&self.kind),
            id: self.identity.object().to_string(),
        }
    }
}

pub(crate) fn ensure_owned_gui_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> Result<(), GuiMutationError> {
    if session
        .source
        .is_project_owned(resolved.identity.document_id())
    {
        Ok(())
    } else {
        Err(GuiMutationError::Blocked(
            "Imported dependency documents are read-only.".to_string(),
        ))
    }
}

pub(crate) fn resolve_request(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<ResolvedGuiObject, String> {
    let path = Utf8Path::new(&request.path);
    let kind = source_kind_for_view(&request.view)?;
    let requested_key = request.object_key.as_deref();
    let mut documents = session
        .source
        .documents
        .iter()
        .filter(|(document_id, _)| document_id.path() == path);
    let Some((document_id, document)) = documents.next() else {
        return Err("No matching GUI document was found for this request.".to_string());
    };
    if documents.next().is_some() {
        return Err("The document path is ambiguous across package modules.".to_string());
    }
    let mut matches = document
        .objects()
        .iter()
        .filter(|object| object.kind() == &kind)
        .filter(|object| requested_key.is_none_or(|key| object.id() == key));
    let Some(source_id) = matches.next() else {
        return Err("No matching GUI object was found for this request.".to_string());
    };
    if matches.next().is_some() && requested_key.is_none() {
        return Err("GUI request must include an object key for this document.".to_string());
    }
    Ok(ResolvedGuiObject {
        identity: SourceIdentity::from_document(document_id.clone(), source_id.id().to_string()),
        kind: source_id.kind().clone(),
    })
}

pub(super) fn source_kind_for_view(view: &DocumentViewId) -> Result<SourceObjectKind, String> {
    match view {
        DocumentViewId::Sequence => Ok(SourceObjectKind::Sequence),
        DocumentViewId::Setup => Ok(SourceObjectKind::Setup),
        DocumentViewId::Preview => Ok(SourceObjectKind::PreviewLayout),
        DocumentViewId::Prop => Ok(SourceObjectKind::PropDefinition),
        DocumentViewId::Text => Err("Text view has no source GUI object kind.".to_string()),
    }
}

pub(crate) fn gui_diagnostic(path: &str, code: &str, message: &str) -> ProjectDiagnostic {
    ProjectDiagnostic {
        path: path.to_string(),
        code: code.to_string(),
        severity: DiagnosticSeverity::Error,
        message: message.to_string(),
        range: None,
        detail: None,
        related: Vec::new(),
    }
}
