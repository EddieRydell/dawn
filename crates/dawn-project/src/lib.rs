#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

mod effect_script;
mod fs;
mod load;
mod lower;
mod model;
mod parse;
mod path;

pub use effect_script::{
    compile, compile_ast, compile_module, compile_module_with_imports, compile_with_imports,
    BytecodeStats, CompiledEffect, EffectParamSchema, EffectScriptKind, EffectVisibility,
    FixtureContext, GeneratorTarget, GeneratorTargetItem, GeneratorTargetPixel, ImportedEffect,
    ParamDefault, PixelContext, PreparedEffectParams, RuntimeArrayValue, RuntimeError,
    RuntimeMarks, RuntimeValue, ScriptDiagnostic, ScriptType, SourcePosition, SourceRange,
};
pub use fs::{WorkspaceEntry, WorkspaceEntryKind, WorkspaceFs};
pub use load::{load_dawn_file, load_project, LoadProjectError};
pub use model::*;
pub use parse::{DawnParseDiagnostic, TextPosition, TextRange};
pub use path::{Utf8Path, Utf8PathBuf};
