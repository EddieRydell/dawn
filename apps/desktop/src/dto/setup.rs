use super::*;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiDocument {
    Setup {
        document: SetupGuiDocument,
    },
    Sequence {
        document: SequenceGuiDocument,
    },
    Preview {
        document: PreviewGuiDocument,
    },
    Prop {
        document: PropGuiDocument,
    },
    Blocked {
        reason: String,
        diagnostics: Vec<ProjectDiagnostic>,
    },
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiDocumentRequest {
    pub path: String,
    pub view: DocumentViewId,
    pub object_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiObjectRef {
    pub module_id: String,
    pub path: String,
    pub object_key: String,
    pub kind: ObjectKind,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiEditCommand {
    Setup { edit: SetupGuiEdit },
    Sequence { edit: SequenceGuiEdit },
    Preview { edit: PreviewGuiEdit },
    Prop { edit: PropGuiEdit },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiEditResult {
    pub snapshot: AppSnapshot,
    pub document: GuiDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BufferExternalState {
    Current,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SequenceGradientStop {
    pub time: f32,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DocumentViewId {
    Text,
    Setup,
    Preview,
    Prop,
    Sequence,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupGuiDocument {
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub elements: Vec<SetupElementNode>,
    pub fixture_profiles: Vec<SetupFixtureProfile>,
    pub preview_links: Vec<SetupPreviewLink>,
    pub patch_nodes: Vec<SetupPatchNode>,
    pub patch_edges: Vec<SetupPatchEdge>,
    pub controllers: Vec<SetupController>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupElementNode {
    pub id: u32,
    pub name: String,
    pub kind: SetupElementKind,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub cell_count: Option<u32>,
    pub capability: Option<String>,
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SetupElementKind {
    Group,
    Color,
    Scalar,
    Indexed,
    Fixture,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupFixtureProfile {
    pub id: String,
    pub name: String,
    pub function_count: u32,
    pub channel_count: u32,
    pub behavior_rule_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupPreviewLink {
    pub prop_id: u32,
    pub name: String,
    pub point_count: u32,
    pub bindings: Vec<SetupElementCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupElementCell {
    pub node: u32,
    pub cell: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupPatchNode {
    pub id: u32,
    pub kind: SetupPatchNodeKind,
    pub label: String,
    pub width: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SetupPatchNodeKind {
    Source,
    Filter,
    Sink,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupPatchEdge {
    pub from_node: u32,
    pub from_port: u16,
    pub to_node: u32,
    pub to_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupController {
    pub label: String,
    pub source_ref: GuiObjectRef,
    pub read_only: bool,
    pub protocol: String,
    pub bind_address: String,
    pub destination: Option<String>,
    pub mode: String,
    pub priority: Option<u8>,
    pub source_name: Option<String>,
    pub ports: Vec<SetupControllerPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupControllerPort {
    pub id: u32,
    pub address: u16,
    pub slot_count: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SetupGuiEdit {
    RenameElement {
        id: u32,
        name: String,
    },
    SetElementCellCount {
        id: u32,
        cells: u32,
    },
    ReorderElements {
        parent: Option<u32>,
        ordered_ids: Vec<u32>,
    },
    SetPreviewBindings {
        prop_id: u32,
        bindings: Vec<SetupElementCell>,
    },
    AutoLinkPreview {
        prop_id: u32,
        node: u32,
        start_cell: u32,
    },
    ConnectPatch {
        from_node: u32,
        from_port: u16,
        to_node: u32,
        to_port: u16,
    },
    DisconnectPatch {
        from_node: u32,
        from_port: u16,
        to_node: u32,
        to_port: u16,
    },
    SetControllerPort {
        controller: GuiObjectRef,
        port: u32,
        address: u16,
        slot_count: u16,
    },
}
