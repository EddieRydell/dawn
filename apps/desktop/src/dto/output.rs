use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputSnapshot {
    pub state: LiveOutputState,
    pub generation: u32,
    pub active_controller_count: u32,
    pub active_universe_count: u32,
    pub controllers: Vec<LiveOutputControllerSnapshot>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum LiveOutputState {
    Disabled,
    Preparing,
    Holding,
    Streaming,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveOutputControllerSnapshot {
    pub id: String,
    pub state: LiveOutputControllerState,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum LiveOutputControllerState {
    Opening,
    Active,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Point3Meters {
    pub x_meters: f32,
    pub y_meters: f32,
    pub z_meters: f32,
}
