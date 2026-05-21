// Lint justification for DSL modules:
// - indexing_slicing: 90+ array index ops in lexer/parser/compiler/VM, all guarded
//   by loop bounds or length checks. Replacing with `.get()` harms readability
//   without meaningful safety benefit since bounds are structurally guaranteed.
// - cast_possible_truncation: f64↔i32/u32/u16 conversions inherent to numeric VM.
// - needless_pass_by_value / module_name_repetitions: style lints that conflict
//   with compiler-code idioms.
macro_rules! dsl_mod {
    ($vis:vis $name:ident) => {
        #[allow(
            clippy::indexing_slicing,
            clippy::cast_possible_truncation,
            clippy::needless_pass_by_value,
            clippy::module_name_repetitions,
        )]
        $vis mod $name;
    };
}

dsl_mod!(pub ast);
dsl_mod!(pub error);
dsl_mod!(pub lexer);
dsl_mod!(pub parser);
dsl_mod!(pub builtins);
dsl_mod!(pub ops);
dsl_mod!(pub typeck);
dsl_mod!(pub compiler);
dsl_mod!(pub optimize);
dsl_mod!(pub peephole);
dsl_mod!(pub vm);

use compiler::CompiledScript;
use error::CompileError;

/// Intern a constant into the pool, reusing an existing index if possible (exact bit equality).
/// Returns `None` if the pool would exceed u16 capacity.
pub(crate) fn intern_constant(constants: &mut Vec<f64>, value: f64) -> Option<u16> {
    for (i, &c) in constants.iter().enumerate() {
        if c.to_bits() == value.to_bits() {
            return u16::try_from(i).ok();
        }
    }
    let idx = u16::try_from(constants.len()).ok()?;
    constants.push(value);
    Some(idx)
}

/// Compile a DSL source string into a `CompiledScript` ready for VM execution.
///
/// This is the primary public entry point for the DSL pipeline:
/// source → lex → parse → type check → constant fold → compile → peephole → `CompiledScript`
pub fn compile_source(source: &str) -> Result<CompiledScript, Vec<CompileError>> {
    let tokens = lexer::lex(source)?;
    let ast = parser::parse(tokens)?;
    let typed = typeck::type_check(&ast)?;
    let folded = optimize::fold_constants(typed);
    let mut compiled = compiler::compile(&folded).map_err(|e| vec![e])?;
    compiled.ops = optimize::peephole(compiled.ops, &mut compiled.constants);
    Ok(compiled)
}
