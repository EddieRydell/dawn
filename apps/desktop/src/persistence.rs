//! Desktop persistence facade for stored models, save policy, and window state.

mod model;
mod service;
mod window;

pub(crate) use model::*;
pub(crate) use service::*;
pub(crate) use window::*;
