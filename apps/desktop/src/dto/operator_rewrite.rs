use serde::{Deserialize, Serialize};
use specta::Type;

use super::SequenceEffectParamValue;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PendingOperatorRewrite {
    pub token: u32,
    pub path: String,
    pub definitions: Vec<OperatorDefinitionRewriteDescription>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorDefinitionRewriteDescription {
    pub definition: OperatorDefinitionKey,
    pub old_name: String,
    pub exact_replacement: Option<String>,
    pub candidates: Vec<OperatorDefinitionCandidate>,
    pub usage_count: u32,
    pub usages: Vec<OperatorRewriteUsageDescription>,
    pub removed_or_changed_params: Vec<String>,
    pub new_required_params: Vec<OperatorRequiredParamDescription>,
    pub removed_ports: Vec<String>,
    pub new_ports: Vec<String>,
}

#[derive(Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorDefinitionKey {
    pub module_id: String,
    pub document: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRewriteUsageDescription {
    pub sequence_path: String,
    pub sequence_name: String,
    pub node_id: String,
    pub upstream_sources: Vec<OperatorUpstreamSourceDescription>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorUpstreamSourceDescription {
    pub node_id: String,
    pub port: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorDefinitionCandidate {
    pub name: String,
    pub params: Vec<OperatorSchemaParam>,
    pub input_ports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorSchemaParam {
    pub name: String,
    pub value_type: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRequiredParamDescription {
    pub name: String,
    pub value_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRewriteResolution {
    pub definitions: Vec<OperatorDefinitionResolution>,
    pub usage_definitions: Vec<OperatorUsageDefinitionResolution>,
    pub parameters: Vec<OperatorParameterResolution>,
    pub usage_parameters: Vec<OperatorUsageParameterResolution>,
    pub ports: Vec<OperatorPortResolution>,
    pub usage_ports: Vec<OperatorUsagePortResolution>,
    pub required_values: Vec<OperatorRequiredValueResolution>,
    pub required_connections: Vec<OperatorRequiredConnectionResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorUsageDefinitionResolution {
    pub sequence_path: String,
    pub sequence_name: String,
    pub node_id: String,
    pub replacement_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorUsageParameterResolution {
    pub sequence_path: String,
    pub sequence_name: String,
    pub node_id: String,
    pub old_name: String,
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorUsagePortResolution {
    pub sequence_path: String,
    pub sequence_name: String,
    pub node_id: String,
    pub old_name: String,
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorDefinitionResolution {
    pub definition: OperatorDefinitionKey,
    pub replacement_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorParameterResolution {
    pub definition: OperatorDefinitionKey,
    pub old_name: String,
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorPortResolution {
    pub definition: OperatorDefinitionKey,
    pub old_name: String,
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRequiredValueResolution {
    pub sequence_path: String,
    pub sequence_name: String,
    pub node_id: String,
    pub name: String,
    pub value: SequenceEffectParamValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRequiredConnectionResolution {
    pub sequence_path: String,
    pub sequence_name: String,
    pub node_id: String,
    pub input_port: String,
    pub from_node: String,
    pub from_port: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OperatorRewriteValidation {
    pub valid: bool,
    pub errors: Vec<String>,
}
