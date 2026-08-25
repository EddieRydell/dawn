use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::mpsc;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::thread;

use dawn_language::dsl::{EffectKind, hash_compiled_effect};
use dawn_language::effect::{
    CurveDefinition, CurveId, CurveSource, EffectDefinition, EffectInst, EffectInstId,
    EffectParamValue, EffectScope, GradientDefinition, GradientId, GradientSource,
};
use dawn_language::model::DawnProject;
use dawn_language::sequence::{
    AutomationBinding, AutomationMapping, MarkCollectionKey, Sequence, SequenceId,
};
use dawn_language::setup::SetupId;
use dawn_language::values::{Curve, DawnTime, Gradient};
use dawn_project_io::ProjectSession;
use dawn_runtime::{
    EffectRasterPrepareBatch, PreparedEffectRasterRenderer, RenderedTargetPixelAddress,
    resolve_effect_target_pixel_addresses,
};

use crate::dto::{
    EffectRasterSettings, GuiDocumentRequest, SequenceClipRaster, SequenceClipRasterError,
    SequenceClipRasterRequest, SequenceClipRasterResponse, SequenceClipRasterResultBatch,
    SequenceClipRasterUnavailable,
};

const RASTER_CACHE_BYTE_BUDGET: usize = 128 * 1024 * 1024;

mod cache;
mod render;
mod service;
mod signature;
mod worker;

use cache::*;
use render::*;
use signature::*;
use worker::*;

pub(crate) use service::SequenceClipRasterService;
