use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackageStatus {
    pub readiness: PackageReadiness,
    pub root: Option<String>,
    pub manifest_valid: bool,
    pub lock_present: bool,
    pub lock_current: bool,
    pub registry: Option<String>,
    pub update_checked: bool,
    pub dependencies: Vec<PackageDependencyStatus>,
    pub modules: Vec<PackageModuleStatus>,
    pub warnings: Vec<PackageCompatibilityWarning>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PackageReadiness {
    NoProject,
    Invalid,
    NeedsSync,
    Ready,
    Warning,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackageDependencyStatus {
    pub alias: String,
    pub source: PackageDependencySource,
    pub requirement: String,
    pub package: Option<String>,
    pub locked_version: Option<String>,
    pub module_id: Option<String>,
    pub cache: PackageCacheState,
    pub update_available: Option<bool>,
    pub website_url: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PackageDependencySource {
    Registry,
    Path,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum PackageCacheState {
    Ready,
    Missing,
    Local,
    Error,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackageModuleStatus {
    pub identity: String,
    pub module_id: String,
    pub version: Option<String>,
    pub documents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PackageCompatibilityWarning {
    pub package: String,
    pub message: String,
    pub breaking: bool,
}
