pub use dawn_runtime::native_effect::*;

use crate::dsl::{BoundParams, DslBindCache, Identifier, RuntimeError, Value};
use crate::effect::{BuiltinEffect, builtin_effect_definition};
use indexmap::IndexMap;

pub fn bind(
    builtin: BuiltinEffect,
    overrides: &IndexMap<Identifier, Value>,
) -> Result<BoundNativeEffect, RuntimeError> {
    bind_cached(builtin, overrides, &mut DslBindCache::default())
}

pub fn bind_cached(
    builtin: BuiltinEffect,
    overrides: &IndexMap<Identifier, Value>,
    cache: &mut DslBindCache,
) -> Result<BoundNativeEffect, RuntimeError> {
    let params =
        BoundParams::bind_cached(&builtin_effect_definition(builtin).params, overrides, cache)?;
    bind_prepared(builtin, params)
}
