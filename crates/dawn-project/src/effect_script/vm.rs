use std::collections::HashMap;

use crate::model::{Color, CurveValue};

use super::ast::{BinaryOp, EffectAst, Expr, Stmt, UnaryOp};
use super::{
    binary_result_type, is_float_compatible, EffectParamSchema, FixtureContext, ParamDefault,
    PixelContext, RuntimeError, RuntimeValue, ScriptDiagnostic, ScriptType,
};

const MAX_LOOP_ITERATIONS: usize = 4096;
const INITIAL_RNG: u64 = 0x9e37_79b9_7f4a_7c15;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BytecodeProgram {
    pub(super) instructions: Vec<Instruction>,
    pub(super) constants: Vec<RuntimeValue>,
    pub(super) registers: RegisterCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytecodeStats {
    pub instruction_count: usize,
    pub constant_count: usize,
    pub param_slots: usize,
    pub float_slots: usize,
    pub int_slots: usize,
    pub bool_slots: usize,
    pub color_slots: usize,
    pub ref_slots: usize,
    pub fixture_slots: usize,
    pub pixel_slots: usize,
    pub total_slots: usize,
}

impl BytecodeStats {
    pub fn instruction_count(&self) -> usize {
        self.instruction_count
    }

    pub fn constant_count(&self) -> usize {
        self.constant_count
    }

    pub fn param_slots(&self) -> usize {
        self.param_slots
    }

    pub fn total_slots(&self) -> usize {
        self.total_slots
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEffectParams {
    values: Vec<RuntimeValue>,
}

#[derive(Debug, Clone)]
pub struct EffectSampleScratch {
    floats: Vec<f64>,
    ints: Vec<i64>,
    bools: Vec<bool>,
    colors: Vec<Color>,
    refs: Vec<RefValue>,
    fixtures: Vec<FixtureContext>,
    pixels: Vec<PixelContext>,
}

impl EffectSampleScratch {
    pub fn new(stats: BytecodeStats) -> Self {
        Self {
            floats: vec![0.0; stats.float_slots],
            ints: vec![0; stats.int_slots],
            bools: vec![false; stats.bool_slots],
            colors: vec![Color::new(0, 0, 0); stats.color_slots],
            refs: vec![RefValue::Unset; stats.ref_slots],
            fixtures: vec![FixtureContext { index: 0 }; stats.fixture_slots],
            pixels: vec![PixelContext { index: 0, count: 0 }; stats.pixel_slots],
        }
    }

    fn resize_for(&mut self, counts: RegisterCounts) {
        self.floats.resize(counts.floats, 0.0);
        self.ints.resize(counts.ints, 0);
        self.bools.resize(counts.bools, false);
        self.colors.resize(counts.colors, Color::new(0, 0, 0));
        self.refs.resize(counts.refs, RefValue::Unset);
        self.fixtures
            .resize(counts.fixtures, FixtureContext { index: 0 });
        self.pixels
            .resize(counts.pixels, PixelContext { index: 0, count: 0 });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RefValue {
    Param(usize),
    Constant(usize),
    Unset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Instruction {
    LoadConst(ValueSlot, usize),
    LoadContext(ValueSlot, ContextSlot),
    LoadParam(ValueSlot, usize),
    Copy(ValueSlot, ValueSlot),
    IntToFloat(FloatSlot, IntSlot),
    FloatUnary(FloatSlot, UnaryFloatInstruction, FloatSlot),
    IntNegate(IntSlot, IntSlot),
    BoolNot(BoolSlot, BoolSlot),
    Binary(ValueSlot, BinaryInstruction, ValueSlot, ValueSlot),
    JumpIfFalse(BoolSlot, usize),
    JumpIfTrue(BoolSlot, usize),
    Jump(usize),
    LoopTick,
    Sin(FloatSlot, FloatSlot),
    Cos(FloatSlot, FloatSlot),
    Abs(FloatSlot, FloatSlot),
    Floor(FloatSlot, FloatSlot),
    Srand(FloatSlot, FloatSlot),
    Rand(FloatSlot),
    PixelIndex(IntSlot, PixelSlot),
    PixelCount(IntSlot, PixelSlot),
    MarkCount(IntSlot, RefSlot),
    MarkAt(FloatSlot, RefSlot, IntSlot, FloatSlot),
    MarkSearch(
        FloatSlot,
        MarkSearchInstruction,
        RefSlot,
        FloatSlot,
        FloatSlot,
    ),
    Min(FloatSlot, FloatSlot, FloatSlot),
    Max(FloatSlot, FloatSlot, FloatSlot),
    Clamp(FloatSlot, FloatSlot, FloatSlot, FloatSlot),
    Smoothstep(FloatSlot, FloatSlot, FloatSlot, FloatSlot),
    MixFloat(FloatSlot, FloatSlot, FloatSlot, FloatSlot),
    MixColor(ColorSlot, ColorSlot, ColorSlot, FloatSlot),
    Rgb(ColorSlot, FloatSlot, FloatSlot, FloatSlot),
    Hsv(ColorSlot, FloatSlot, FloatSlot, FloatSlot),
    CallCurveParam(ValueSlot, usize, FloatSlot),
    ReturnColor(ColorSlot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UnaryFloatInstruction {
    Negate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BinaryInstruction {
    FloatAdd,
    FloatSubtract,
    FloatMultiply,
    FloatDivide,
    IntAdd,
    IntSubtract,
    IntMultiply,
    IntDivide,
    FloatLess,
    FloatLessEqual,
    FloatGreater,
    FloatGreaterEqual,
    IntLess,
    IntLessEqual,
    IntGreater,
    IntGreaterEqual,
    FloatEqual,
    FloatNotEqual,
    IntEqual,
    IntNotEqual,
    BoolEqual,
    BoolNotEqual,
    ColorMultiplyFloat,
    FloatMultiplyColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MarkSearchInstruction {
    Prev,
    Next,
    Nearest,
    Phase,
    Elapsed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContextSlot {
    Progress,
    Seconds,
    Fixture,
    Pixel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueSlot {
    Float(FloatSlot),
    Int(IntSlot),
    Bool(BoolSlot),
    Color(ColorSlot),
    Ref(RefSlot, ScriptType),
    Fixture(FixtureSlot),
    Pixel(PixelSlot),
}

impl ValueSlot {
    fn value_type(self) -> ScriptType {
        match self {
            Self::Float(_) => ScriptType::Float,
            Self::Int(_) => ScriptType::Int,
            Self::Bool(_) => ScriptType::Bool,
            Self::Color(_) => ScriptType::Color,
            Self::Ref(_, value_type) => value_type,
            Self::Fixture(_) => ScriptType::Fixture,
            Self::Pixel(_) => ScriptType::Pixel,
        }
    }

    fn float(self) -> FloatSlot {
        match self {
            Self::Float(slot) => slot,
            _ => unreachable!("type checker validates float slot"),
        }
    }

    fn int(self) -> IntSlot {
        match self {
            Self::Int(slot) => slot,
            _ => unreachable!("type checker validates int slot"),
        }
    }

    fn bool(self) -> BoolSlot {
        match self {
            Self::Bool(slot) => slot,
            _ => unreachable!("type checker validates bool slot"),
        }
    }

    fn color(self) -> ColorSlot {
        match self {
            Self::Color(slot) => slot,
            _ => unreachable!("type checker validates color slot"),
        }
    }

    fn reference(self) -> RefSlot {
        match self {
            Self::Ref(slot, _) => slot,
            _ => unreachable!("type checker validates ref slot"),
        }
    }

    fn pixel(self) -> PixelSlot {
        match self {
            Self::Pixel(slot) => slot,
            _ => unreachable!("type checker validates pixel slot"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FloatSlot(usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct IntSlot(usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BoolSlot(usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ColorSlot(usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RefSlot(usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FixtureSlot(usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PixelSlot(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct RegisterCounts {
    pub(super) floats: usize,
    pub(super) ints: usize,
    pub(super) bools: usize,
    pub(super) colors: usize,
    pub(super) refs: usize,
    pub(super) fixtures: usize,
    pub(super) pixels: usize,
}

impl RegisterCounts {
    pub(super) fn total(self) -> usize {
        self.floats + self.ints + self.bools + self.colors + self.refs + self.fixtures + self.pixels
    }
}

#[derive(Debug, Clone, Copy)]
enum Binding {
    Context(ContextSlot),
    Constant(usize),
    Param {
        index: usize,
        value_type: ScriptType,
    },
    Local(ValueSlot),
}

#[derive(Debug)]
pub(super) struct Vm;

impl Vm {
    pub(super) fn compile(effect: &EffectAst) -> BytecodeProgram {
        Compiler::new(effect).compile()
    }

    pub(super) fn prepare_params(
        params: &[EffectParamSchema],
        values: &std::collections::BTreeMap<String, RuntimeValue>,
    ) -> Result<PreparedEffectParams, RuntimeError> {
        Self::prepare_params_with(params, |name| values.get(name).cloned())
    }

    pub(super) fn prepare_params_with(
        params: &[EffectParamSchema],
        mut value_for: impl FnMut(&str) -> Option<RuntimeValue>,
    ) -> Result<PreparedEffectParams, RuntimeError> {
        let mut prepared = Vec::with_capacity(params.len());
        for param in params {
            let value = value_for(&param.name)
                .or_else(|| {
                    param
                        .default
                        .as_ref()
                        .map(|ParamDefault::Value(value)| value.clone())
                })
                .ok_or_else(|| RuntimeError {
                    message: format!("missing parameter `{}`", param.name),
                })?;
            prepared.push(Self::coerce_value(value, param.value_type)?);
        }
        Ok(PreparedEffectParams { values: prepared })
    }

    pub(super) fn run(
        program: &BytecodeProgram,
        progress: f64,
        seconds: f64,
        fixture: FixtureContext,
        pixel: PixelContext,
        params: &PreparedEffectParams,
    ) -> Result<Color, RuntimeError> {
        let mut scratch = EffectSampleScratch::new(stats_for_program(program, params.values.len()));
        Self::run_with_scratch(
            program,
            progress,
            seconds,
            fixture,
            pixel,
            params,
            &mut scratch,
        )
    }

    pub(super) fn run_with_scratch<'a>(
        program: &'a BytecodeProgram,
        progress: f64,
        seconds: f64,
        fixture: FixtureContext,
        pixel: PixelContext,
        params: &'a PreparedEffectParams,
        scratch: &mut EffectSampleScratch,
    ) -> Result<Color, RuntimeError> {
        scratch.resize_for(program.registers);
        BytecodeVm {
            program,
            progress,
            seconds,
            fixture,
            pixel,
            params,
            scratch,
            ip: 0,
            rng: INITIAL_RNG,
            loop_iterations: 0,
        }
        .run()
    }

    pub(super) fn eval_constant(expr: &Expr) -> Result<RuntimeValue, ScriptDiagnostic> {
        match expr {
            Expr::Float(value) => Ok(RuntimeValue::Float(*value)),
            Expr::Int(value) => Ok(RuntimeValue::Int(*value)),
            Expr::Bool(value) => Ok(RuntimeValue::Bool(*value)),
            Expr::Color(value) => Ok(RuntimeValue::Color(*value)),
            _ => Err(ScriptDiagnostic {
                range: None,
                message: "parameter defaults must be literals in Dawn v1".to_string(),
            }),
        }
    }

    pub(super) fn coerce_value(
        value: RuntimeValue,
        expected: ScriptType,
    ) -> Result<RuntimeValue, RuntimeError> {
        match (expected, value) {
            (ScriptType::Float, RuntimeValue::Int(value)) => Ok(RuntimeValue::Float(value as f64)),
            (expected, value) if value.value_type() == expected => Ok(value),
            (expected, value) => Err(RuntimeError {
                message: format!(
                    "expected {expected} value, but found {}",
                    value.value_type()
                ),
            }),
        }
    }
}

fn stats_for_program(program: &BytecodeProgram, param_slots: usize) -> BytecodeStats {
    BytecodeStats {
        instruction_count: program.instructions.len(),
        constant_count: program.constants.len(),
        param_slots,
        float_slots: program.registers.floats,
        int_slots: program.registers.ints,
        bool_slots: program.registers.bools,
        color_slots: program.registers.colors,
        ref_slots: program.registers.refs,
        fixture_slots: program.registers.fixtures,
        pixel_slots: program.registers.pixels,
        total_slots: program.registers.total(),
    }
}

struct Compiler<'a> {
    effect: &'a EffectAst,
    instructions: Vec<Instruction>,
    constants: Vec<RuntimeValue>,
    scopes: Vec<HashMap<String, Binding>>,
    registers: RegisterCounts,
}

impl<'a> Compiler<'a> {
    fn new(effect: &'a EffectAst) -> Self {
        let mut compiler = Self {
            effect,
            instructions: Vec::new(),
            constants: Vec::new(),
            scopes: vec![HashMap::new()],
            registers: RegisterCounts::default(),
        };
        compiler.define_builtin_bindings();
        compiler
    }

    fn compile(mut self) -> BytecodeProgram {
        for statement in &self.effect.sample {
            self.compile_statement(statement);
        }
        BytecodeProgram {
            instructions: self.instructions,
            constants: self.constants,
            registers: self.registers,
        }
    }

    fn define_builtin_bindings(&mut self) {
        self.define("progress", Binding::Context(ContextSlot::Progress));
        self.define("seconds", Binding::Context(ContextSlot::Seconds));
        self.define("fixture", Binding::Context(ContextSlot::Fixture));
        self.define("pixel", Binding::Context(ContextSlot::Pixel));
        let pi = self.add_constant(RuntimeValue::Float(std::f64::consts::PI));
        let tau = self.add_constant(RuntimeValue::Float(std::f64::consts::TAU));
        self.define("PI", Binding::Constant(pi));
        self.define("TAU", Binding::Constant(tau));
        for (index, param) in self.effect.params.iter().enumerate() {
            self.define(
                &param.name,
                Binding::Param {
                    index,
                    value_type: param.value_type,
                },
            );
        }
    }

    fn compile_statement(&mut self, statement: &Stmt) {
        match statement {
            Stmt::Let {
                name,
                value_type,
                expr,
            } => {
                let local = self.allocate_slot(*value_type);
                let value = self.compile_expr(expr);
                self.emit_assign(local, value);
                self.define(name, Binding::Local(local));
            }
            Stmt::Assign { name, expr } => {
                let Binding::Local(local) = self.expect_binding(name) else {
                    unreachable!("type checker rejects assignment to non-local bindings");
                };
                let value = self.compile_expr(expr);
                self.emit_assign(local, value);
            }
            Stmt::Expr(expr) => {
                self.compile_expr(expr);
            }
            Stmt::For {
                name,
                value_type,
                initializer,
                condition,
                update,
                body,
            } => {
                self.push_scope();
                let local = self.allocate_slot(*value_type);
                let initializer = self.compile_expr(initializer);
                self.emit_assign(local, initializer);
                self.define(name, Binding::Local(local));
                let loop_start = self.instructions.len();
                let condition = self.compile_expr(condition).bool();
                let exit_jump = self.emit_jump_if_false(condition);
                self.emit(Instruction::LoopTick);
                self.push_scope();
                for statement in body {
                    self.compile_statement(statement);
                }
                self.pop_scope();
                self.compile_statement(update);
                self.emit(Instruction::Jump(loop_start));
                self.patch_jump(exit_jump);
                self.pop_scope();
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                let condition = self.compile_expr(condition).bool();
                let else_jump = self.emit_jump_if_false(condition);
                self.push_scope();
                for statement in then_body {
                    self.compile_statement(statement);
                }
                self.pop_scope();
                let end_jump = self.emit_jump();
                self.patch_jump(else_jump);
                self.push_scope();
                for statement in else_body {
                    self.compile_statement(statement);
                }
                self.pop_scope();
                self.patch_jump(end_jump);
            }
            Stmt::Return(expr) => {
                let value = self.compile_expr(expr).color();
                self.emit(Instruction::ReturnColor(value));
            }
        }
    }

    fn compile_expr(&mut self, expr: &Expr) -> ValueSlot {
        match expr {
            Expr::Float(value) => self.load_constant(RuntimeValue::Float(*value)),
            Expr::Int(value) => self.load_constant(RuntimeValue::Int(*value)),
            Expr::Bool(value) => self.load_constant(RuntimeValue::Bool(*value)),
            Expr::Color(value) => self.load_constant(RuntimeValue::Color(*value)),
            Expr::Ident(name) => match self.expect_binding(name) {
                Binding::Context(slot) => {
                    let dest = self.allocate_slot(match slot {
                        ContextSlot::Progress | ContextSlot::Seconds => ScriptType::Float,
                        ContextSlot::Fixture => ScriptType::Fixture,
                        ContextSlot::Pixel => ScriptType::Pixel,
                    });
                    self.emit(Instruction::LoadContext(dest, slot));
                    dest
                }
                Binding::Constant(index) => {
                    let value_type = self.constants[index].value_type();
                    let dest = self.allocate_slot(value_type);
                    self.emit(Instruction::LoadConst(dest, index));
                    dest
                }
                Binding::Param { index, value_type } => {
                    let dest = self.allocate_slot(value_type);
                    self.emit(Instruction::LoadParam(dest, index));
                    dest
                }
                Binding::Local(slot) => slot,
            },
            Expr::Unary { op, expr } => {
                let value = self.compile_expr(expr);
                match (*op, value) {
                    (UnaryOp::Negate, ValueSlot::Float(source)) => {
                        let dest = self.allocate_float();
                        self.emit(Instruction::FloatUnary(
                            dest,
                            UnaryFloatInstruction::Negate,
                            source,
                        ));
                        ValueSlot::Float(dest)
                    }
                    (UnaryOp::Negate, ValueSlot::Int(source)) => {
                        let dest = self.allocate_int();
                        self.emit(Instruction::IntNegate(dest, source));
                        ValueSlot::Int(dest)
                    }
                    (UnaryOp::Not, ValueSlot::Bool(source)) => {
                        let dest = self.allocate_bool();
                        self.emit(Instruction::BoolNot(dest, source));
                        ValueSlot::Bool(dest)
                    }
                    _ => unreachable!("type checker validates unary expression"),
                }
            }
            Expr::Binary { left, op, right } => self.compile_binary(left, *op, right),
            Expr::Call { name, args } => self.compile_call(name, args),
        }
    }

    fn compile_binary(&mut self, left: &Expr, op: BinaryOp, right: &Expr) -> ValueSlot {
        match op {
            BinaryOp::LogicalAnd => {
                let dest = self.allocate_bool();
                let left = self.compile_expr(left).bool();
                self.emit(Instruction::Copy(
                    ValueSlot::Bool(dest),
                    ValueSlot::Bool(left),
                ));
                let false_jump = self.emit_jump_if_false(dest);
                let right = self.compile_expr(right).bool();
                self.emit(Instruction::Copy(
                    ValueSlot::Bool(dest),
                    ValueSlot::Bool(right),
                ));
                self.patch_jump(false_jump);
                ValueSlot::Bool(dest)
            }
            BinaryOp::LogicalOr => {
                let dest = self.allocate_bool();
                let left = self.compile_expr(left).bool();
                self.emit(Instruction::Copy(
                    ValueSlot::Bool(dest),
                    ValueSlot::Bool(left),
                ));
                let true_jump = self.emit_jump_if_true(dest);
                let right = self.compile_expr(right).bool();
                self.emit(Instruction::Copy(
                    ValueSlot::Bool(dest),
                    ValueSlot::Bool(right),
                ));
                self.patch_jump(true_jump);
                ValueSlot::Bool(dest)
            }
            _ => {
                let left_type = self.expr_type(left);
                let right_type = self.expr_type(right);
                let left = self.compile_expr(left);
                let right = self.compile_expr(right);
                let instruction = self.binary_instruction(left_type, op, right_type);
                let dest = self.allocate_slot(
                    binary_result_type(left_type, op, right_type)
                        .expect("type checker validates binary expression"),
                );
                let left = self.float_promote_if_needed(left, instruction, true);
                let right = self.float_promote_if_needed(right, instruction, false);
                self.emit(Instruction::Binary(dest, instruction, left, right));
                dest
            }
        }
    }

    fn float_promote_if_needed(
        &mut self,
        slot: ValueSlot,
        instruction: BinaryInstruction,
        is_left: bool,
    ) -> ValueSlot {
        let needs_float = match (instruction, is_left) {
            (
                BinaryInstruction::FloatAdd
                | BinaryInstruction::FloatSubtract
                | BinaryInstruction::FloatMultiply
                | BinaryInstruction::FloatDivide
                | BinaryInstruction::FloatLess
                | BinaryInstruction::FloatLessEqual
                | BinaryInstruction::FloatGreater
                | BinaryInstruction::FloatGreaterEqual
                | BinaryInstruction::FloatEqual
                | BinaryInstruction::FloatNotEqual,
                _,
            ) => true,
            (BinaryInstruction::ColorMultiplyFloat, false) => true,
            (BinaryInstruction::FloatMultiplyColor, true) => true,
            _ => false,
        };
        if needs_float && matches!(slot, ValueSlot::Int(_)) {
            let dest = self.allocate_float();
            self.emit(Instruction::IntToFloat(dest, slot.int()));
            ValueSlot::Float(dest)
        } else {
            slot
        }
    }

    fn binary_instruction(
        &self,
        left: ScriptType,
        op: BinaryOp,
        right: ScriptType,
    ) -> BinaryInstruction {
        match (left, op, right) {
            (ScriptType::Float, BinaryOp::Add, ScriptType::Float)
            | (ScriptType::Float, BinaryOp::Add, ScriptType::Int)
            | (ScriptType::Int, BinaryOp::Add, ScriptType::Float) => BinaryInstruction::FloatAdd,
            (ScriptType::Float, BinaryOp::Subtract, ScriptType::Float)
            | (ScriptType::Float, BinaryOp::Subtract, ScriptType::Int)
            | (ScriptType::Int, BinaryOp::Subtract, ScriptType::Float) => {
                BinaryInstruction::FloatSubtract
            }
            (ScriptType::Float, BinaryOp::Multiply, ScriptType::Float)
            | (ScriptType::Float, BinaryOp::Multiply, ScriptType::Int)
            | (ScriptType::Int, BinaryOp::Multiply, ScriptType::Float) => {
                BinaryInstruction::FloatMultiply
            }
            (ScriptType::Float, BinaryOp::Divide, ScriptType::Float)
            | (ScriptType::Float, BinaryOp::Divide, ScriptType::Int)
            | (ScriptType::Int, BinaryOp::Divide, ScriptType::Float) => {
                BinaryInstruction::FloatDivide
            }
            (ScriptType::Int, BinaryOp::Add, ScriptType::Int) => BinaryInstruction::IntAdd,
            (ScriptType::Int, BinaryOp::Subtract, ScriptType::Int) => {
                BinaryInstruction::IntSubtract
            }
            (ScriptType::Int, BinaryOp::Multiply, ScriptType::Int) => {
                BinaryInstruction::IntMultiply
            }
            (ScriptType::Int, BinaryOp::Divide, ScriptType::Int) => BinaryInstruction::IntDivide,
            (left, BinaryOp::Less, right) if is_float_compare(left, right) => {
                BinaryInstruction::FloatLess
            }
            (left, BinaryOp::LessEqual, right) if is_float_compare(left, right) => {
                BinaryInstruction::FloatLessEqual
            }
            (left, BinaryOp::Greater, right) if is_float_compare(left, right) => {
                BinaryInstruction::FloatGreater
            }
            (left, BinaryOp::GreaterEqual, right) if is_float_compare(left, right) => {
                BinaryInstruction::FloatGreaterEqual
            }
            (ScriptType::Int, BinaryOp::Less, ScriptType::Int) => BinaryInstruction::IntLess,
            (ScriptType::Int, BinaryOp::LessEqual, ScriptType::Int) => {
                BinaryInstruction::IntLessEqual
            }
            (ScriptType::Int, BinaryOp::Greater, ScriptType::Int) => BinaryInstruction::IntGreater,
            (ScriptType::Int, BinaryOp::GreaterEqual, ScriptType::Int) => {
                BinaryInstruction::IntGreaterEqual
            }
            (left, BinaryOp::Equal, right) if is_float_compare(left, right) => {
                BinaryInstruction::FloatEqual
            }
            (left, BinaryOp::NotEqual, right) if is_float_compare(left, right) => {
                BinaryInstruction::FloatNotEqual
            }
            (ScriptType::Int, BinaryOp::Equal, ScriptType::Int) => BinaryInstruction::IntEqual,
            (ScriptType::Int, BinaryOp::NotEqual, ScriptType::Int) => {
                BinaryInstruction::IntNotEqual
            }
            (ScriptType::Bool, BinaryOp::Equal, ScriptType::Bool) => BinaryInstruction::BoolEqual,
            (ScriptType::Bool, BinaryOp::NotEqual, ScriptType::Bool) => {
                BinaryInstruction::BoolNotEqual
            }
            (ScriptType::Color, BinaryOp::Multiply, factor) if is_float_compatible(factor) => {
                BinaryInstruction::ColorMultiplyFloat
            }
            (factor, BinaryOp::Multiply, ScriptType::Color) if is_float_compatible(factor) => {
                BinaryInstruction::FloatMultiplyColor
            }
            _ => unreachable!("type checker validates binary expression"),
        }
    }

    fn expr_type(&self, expr: &Expr) -> ScriptType {
        match expr {
            Expr::Float(_) => ScriptType::Float,
            Expr::Int(_) => ScriptType::Int,
            Expr::Bool(_) => ScriptType::Bool,
            Expr::Color(_) => ScriptType::Color,
            Expr::Ident(name) => match self.expect_binding(name) {
                Binding::Context(ContextSlot::Progress | ContextSlot::Seconds)
                | Binding::Constant(_) => ScriptType::Float,
                Binding::Context(ContextSlot::Fixture) => ScriptType::Fixture,
                Binding::Context(ContextSlot::Pixel) => ScriptType::Pixel,
                Binding::Param { value_type, .. } => value_type,
                Binding::Local(slot) => slot.value_type(),
            },
            Expr::Unary { op, expr } => match op {
                UnaryOp::Negate => self.expr_type(expr),
                UnaryOp::Not => ScriptType::Bool,
            },
            Expr::Binary { left, op, right } => {
                let left = self.expr_type(left);
                let right = self.expr_type(right);
                binary_result_type(left, *op, right)
                    .expect("type checker validates binary expression")
            }
            Expr::Call { name, args } => self.call_type(name, args),
        }
    }

    fn call_type(&self, name: &str, args: &[Expr]) -> ScriptType {
        if let Some(Binding::Param { value_type, .. }) = self.binding(name) {
            return match value_type {
                ScriptType::CurveFloat => ScriptType::Float,
                ScriptType::CurveColor => ScriptType::Color,
                _ => unreachable!("type checker validates callable binding"),
            };
        }

        match name {
            "sin" | "cos" | "abs" | "floor" | "srand" | "rand" | "mark_at" | "mark_prev"
            | "mark_next" | "mark_nearest" | "mark_phase" | "mark_elapsed" | "min" | "max"
            | "clamp" | "smoothstep" => ScriptType::Float,
            "pixel_index" | "pixel_count" | "mark_count" => ScriptType::Int,
            "rgb" | "hsv" => ScriptType::Color,
            "mix" => {
                if matches!(
                    args.first().map(|arg| self.expr_type(arg)),
                    Some(ScriptType::Color)
                ) {
                    ScriptType::Color
                } else {
                    ScriptType::Float
                }
            }
            _ => unreachable!("type checker validates builtins"),
        }
    }

    fn compile_call(&mut self, name: &str, args: &[Expr]) -> ValueSlot {
        if let Some(Binding::Param { index, value_type }) = self.binding(name) {
            if matches!(value_type, ScriptType::CurveFloat | ScriptType::CurveColor) {
                let amount = self.compile_float_arg(&args[0]);
                let dest = self.allocate_slot(match value_type {
                    ScriptType::CurveFloat => ScriptType::Float,
                    ScriptType::CurveColor => ScriptType::Color,
                    _ => unreachable!("checked above"),
                });
                self.emit(Instruction::CallCurveParam(dest, index, amount));
                return dest;
            }
        }

        match name {
            "sin" => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Sin(dest, value));
                ValueSlot::Float(dest)
            }
            "cos" => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Cos(dest, value));
                ValueSlot::Float(dest)
            }
            "abs" => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Abs(dest, value));
                ValueSlot::Float(dest)
            }
            "floor" => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Floor(dest, value));
                ValueSlot::Float(dest)
            }
            "srand" => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Srand(dest, value));
                ValueSlot::Float(dest)
            }
            "rand" => {
                let dest = self.allocate_float();
                self.emit(Instruction::Rand(dest));
                ValueSlot::Float(dest)
            }
            "pixel_index" => {
                let pixel = self.compile_expr(&args[0]).pixel();
                let dest = self.allocate_int();
                self.emit(Instruction::PixelIndex(dest, pixel));
                ValueSlot::Int(dest)
            }
            "pixel_count" => {
                let pixel = self.compile_expr(&args[0]).pixel();
                let dest = self.allocate_int();
                self.emit(Instruction::PixelCount(dest, pixel));
                ValueSlot::Int(dest)
            }
            "mark_count" => {
                let marks = self.compile_expr(&args[0]).reference();
                let dest = self.allocate_int();
                self.emit(Instruction::MarkCount(dest, marks));
                ValueSlot::Int(dest)
            }
            "mark_at" => {
                let marks = self.compile_expr(&args[0]).reference();
                let index = self.compile_expr(&args[1]).int();
                let fallback = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                self.emit(Instruction::MarkAt(dest, marks, index, fallback));
                ValueSlot::Float(dest)
            }
            "mark_prev" | "mark_next" | "mark_nearest" | "mark_phase" | "mark_elapsed" => {
                let marks = self.compile_expr(&args[0]).reference();
                let time = self.compile_float_arg(&args[1]);
                let fallback = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                let search = match name {
                    "mark_prev" => MarkSearchInstruction::Prev,
                    "mark_next" => MarkSearchInstruction::Next,
                    "mark_nearest" => MarkSearchInstruction::Nearest,
                    "mark_phase" => MarkSearchInstruction::Phase,
                    "mark_elapsed" => MarkSearchInstruction::Elapsed,
                    _ => unreachable!("matched above"),
                };
                self.emit(Instruction::MarkSearch(dest, search, marks, time, fallback));
                ValueSlot::Float(dest)
            }
            "min" => {
                let left = self.compile_float_arg(&args[0]);
                let right = self.compile_float_arg(&args[1]);
                let dest = self.allocate_float();
                self.emit(Instruction::Min(dest, left, right));
                ValueSlot::Float(dest)
            }
            "max" => {
                let left = self.compile_float_arg(&args[0]);
                let right = self.compile_float_arg(&args[1]);
                let dest = self.allocate_float();
                self.emit(Instruction::Max(dest, left, right));
                ValueSlot::Float(dest)
            }
            "clamp" => {
                let value = self.compile_float_arg(&args[0]);
                let min = self.compile_float_arg(&args[1]);
                let max = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                self.emit(Instruction::Clamp(dest, value, min, max));
                ValueSlot::Float(dest)
            }
            "smoothstep" => {
                let edge0 = self.compile_float_arg(&args[0]);
                let edge1 = self.compile_float_arg(&args[1]);
                let value = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                self.emit(Instruction::Smoothstep(dest, edge0, edge1, value));
                ValueSlot::Float(dest)
            }
            "mix" if self.call_type(name, args) == ScriptType::Color => {
                let left = self.compile_expr(&args[0]).color();
                let right = self.compile_expr(&args[1]).color();
                let amount = self.compile_float_arg(&args[2]);
                let dest = self.allocate_color();
                self.emit(Instruction::MixColor(dest, left, right, amount));
                ValueSlot::Color(dest)
            }
            "mix" => {
                let left = self.compile_float_arg(&args[0]);
                let right = self.compile_float_arg(&args[1]);
                let amount = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                self.emit(Instruction::MixFloat(dest, left, right, amount));
                ValueSlot::Float(dest)
            }
            "rgb" => {
                let red = self.compile_float_arg(&args[0]);
                let green = self.compile_float_arg(&args[1]);
                let blue = self.compile_float_arg(&args[2]);
                let dest = self.allocate_color();
                self.emit(Instruction::Rgb(dest, red, green, blue));
                ValueSlot::Color(dest)
            }
            "hsv" => {
                let hue = self.compile_float_arg(&args[0]);
                let saturation = self.compile_float_arg(&args[1]);
                let value = self.compile_float_arg(&args[2]);
                let dest = self.allocate_color();
                self.emit(Instruction::Hsv(dest, hue, saturation, value));
                ValueSlot::Color(dest)
            }
            _ => unreachable!("type checker validates builtins"),
        }
    }

    fn compile_float_arg(&mut self, expr: &Expr) -> FloatSlot {
        let value = self.compile_expr(expr);
        match value {
            ValueSlot::Float(slot) => slot,
            ValueSlot::Int(slot) => {
                let dest = self.allocate_float();
                self.emit(Instruction::IntToFloat(dest, slot));
                dest
            }
            _ => unreachable!("type checker validates float-compatible argument"),
        }
    }

    fn load_constant(&mut self, value: RuntimeValue) -> ValueSlot {
        let value_type = value.value_type();
        let index = self.add_constant(value);
        let dest = self.allocate_slot(value_type);
        self.emit(Instruction::LoadConst(dest, index));
        dest
    }

    fn add_constant(&mut self, value: RuntimeValue) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    fn emit_assign(&mut self, dest: ValueSlot, source: ValueSlot) {
        match (dest, source) {
            (ValueSlot::Float(dest), ValueSlot::Int(source)) => {
                self.emit(Instruction::IntToFloat(dest, source));
            }
            (dest, source) if dest.value_type() == source.value_type() => {
                self.emit(Instruction::Copy(dest, source));
            }
            _ => unreachable!("type checker validates assignment"),
        }
    }

    fn emit_jump_if_false(&mut self, condition: BoolSlot) -> usize {
        let index = self.instructions.len();
        self.emit(Instruction::JumpIfFalse(condition, usize::MAX));
        index
    }

    fn emit_jump_if_true(&mut self, condition: BoolSlot) -> usize {
        let index = self.instructions.len();
        self.emit(Instruction::JumpIfTrue(condition, usize::MAX));
        index
    }

    fn emit_jump(&mut self) -> usize {
        let index = self.instructions.len();
        self.emit(Instruction::Jump(usize::MAX));
        index
    }

    fn patch_jump(&mut self, index: usize) {
        let target = self.instructions.len();
        match &mut self.instructions[index] {
            Instruction::JumpIfFalse(_, slot)
            | Instruction::JumpIfTrue(_, slot)
            | Instruction::Jump(slot) => {
                *slot = target;
            }
            _ => unreachable!("patch target must be a jump"),
        }
    }

    fn emit(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
    }

    fn allocate_slot(&mut self, value_type: ScriptType) -> ValueSlot {
        match value_type {
            ScriptType::Float => ValueSlot::Float(self.allocate_float()),
            ScriptType::Int => ValueSlot::Int(self.allocate_int()),
            ScriptType::Bool => ValueSlot::Bool(self.allocate_bool()),
            ScriptType::Color => ValueSlot::Color(self.allocate_color()),
            ScriptType::Marks
            | ScriptType::CurveFloat
            | ScriptType::CurveColor
            | ScriptType::Enum
            | ScriptType::Flags => {
                let slot = RefSlot(self.registers.refs);
                self.registers.refs += 1;
                ValueSlot::Ref(slot, value_type)
            }
            ScriptType::Fixture => {
                let slot = FixtureSlot(self.registers.fixtures);
                self.registers.fixtures += 1;
                ValueSlot::Fixture(slot)
            }
            ScriptType::Pixel => {
                let slot = PixelSlot(self.registers.pixels);
                self.registers.pixels += 1;
                ValueSlot::Pixel(slot)
            }
            ScriptType::Void => unreachable!("void values are not stored"),
        }
    }

    fn allocate_float(&mut self) -> FloatSlot {
        let slot = FloatSlot(self.registers.floats);
        self.registers.floats += 1;
        slot
    }

    fn allocate_int(&mut self) -> IntSlot {
        let slot = IntSlot(self.registers.ints);
        self.registers.ints += 1;
        slot
    }

    fn allocate_bool(&mut self) -> BoolSlot {
        let slot = BoolSlot(self.registers.bools);
        self.registers.bools += 1;
        slot
    }

    fn allocate_color(&mut self) -> ColorSlot {
        let slot = ColorSlot(self.registers.colors);
        self.registers.colors += 1;
        slot
    }

    fn binding(&self, name: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn expect_binding(&self, name: &str) -> Binding {
        self.binding(name)
            .unwrap_or_else(|| panic!("type checker validates binding `{name}`"))
    }

    fn define(&mut self, name: &str, binding: Binding) {
        self.scopes
            .last_mut()
            .expect("compiler always has a scope")
            .insert(name.to_string(), binding);
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
}

fn is_float_compare(left: ScriptType, right: ScriptType) -> bool {
    is_float_compatible(left)
        && is_float_compatible(right)
        && (left == ScriptType::Float || right == ScriptType::Float)
}

struct BytecodeVm<'a, 'scratch> {
    program: &'a BytecodeProgram,
    progress: f64,
    seconds: f64,
    fixture: FixtureContext,
    pixel: PixelContext,
    params: &'a PreparedEffectParams,
    scratch: &'scratch mut EffectSampleScratch,
    ip: usize,
    rng: u64,
    loop_iterations: usize,
}

impl<'a> BytecodeVm<'a, '_> {
    fn run(&mut self) -> Result<Color, RuntimeError> {
        while let Some(instruction) = self.program.instructions.get(self.ip) {
            self.ip += 1;
            match *instruction {
                Instruction::LoadConst(dest, index) => {
                    if matches!(dest, ValueSlot::Ref(_, _)) {
                        self.write_ref_source(dest.reference(), RefValue::Constant(index));
                    } else {
                        self.write_runtime(dest, &self.program.constants[index])?;
                    }
                }
                Instruction::LoadContext(dest, slot) => self.write_context(dest, slot),
                Instruction::LoadParam(dest, index) => {
                    if matches!(dest, ValueSlot::Ref(_, _)) {
                        self.write_ref_source(dest.reference(), RefValue::Param(index));
                    } else {
                        self.write_runtime(dest, &self.params.values[index])?;
                    }
                }
                Instruction::Copy(dest, source) => self.copy(dest, source),
                Instruction::IntToFloat(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.ints[source.0] as f64;
                }
                Instruction::FloatUnary(dest, UnaryFloatInstruction::Negate, source) => {
                    self.scratch.floats[dest.0] = -self.scratch.floats[source.0];
                }
                Instruction::IntNegate(dest, source) => {
                    self.scratch.ints[dest.0] = self.scratch.ints[source.0]
                        .checked_neg()
                        .ok_or_else(|| self.error("integer overflow"))?;
                }
                Instruction::BoolNot(dest, source) => {
                    self.scratch.bools[dest.0] = !self.scratch.bools[source.0];
                }
                Instruction::Binary(dest, op, left, right) => {
                    self.eval_binary(dest, op, left, right)?
                }
                Instruction::JumpIfFalse(condition, target) => {
                    if !self.scratch.bools[condition.0] {
                        self.ip = target;
                    }
                }
                Instruction::JumpIfTrue(condition, target) => {
                    if self.scratch.bools[condition.0] {
                        self.ip = target;
                    }
                }
                Instruction::Jump(target) => self.ip = target,
                Instruction::LoopTick => {
                    self.loop_iterations += 1;
                    if self.loop_iterations > MAX_LOOP_ITERATIONS {
                        return Err(self.error("effect exceeded the maximum loop iteration count"));
                    }
                }
                Instruction::Sin(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[source.0].sin();
                }
                Instruction::Cos(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[source.0].cos();
                }
                Instruction::Abs(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[source.0].abs();
                }
                Instruction::Floor(dest, source) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[source.0].floor();
                }
                Instruction::Srand(dest, source) => {
                    self.rng = seed_from_float(self.scratch.floats[source.0]);
                    self.scratch.floats[dest.0] = 0.0;
                }
                Instruction::Rand(dest) => {
                    self.scratch.floats[dest.0] = self.rand();
                }
                Instruction::PixelIndex(dest, source) => {
                    self.scratch.ints[dest.0] = self.scratch.pixels[source.0].index as i64;
                }
                Instruction::PixelCount(dest, source) => {
                    self.scratch.ints[dest.0] = self.scratch.pixels[source.0].count as i64;
                }
                Instruction::MarkCount(dest, source) => {
                    self.scratch.ints[dest.0] = self.marks(source)?.len() as i64;
                }
                Instruction::MarkAt(dest, marks, index, fallback) => {
                    let marks = self.marks(marks)?;
                    let fallback = self.scratch.floats[fallback.0];
                    let value = usize::try_from(self.scratch.ints[index.0])
                        .ok()
                        .and_then(|index| marks.get(index))
                        .copied()
                        .unwrap_or(fallback);
                    self.scratch.floats[dest.0] = value;
                }
                Instruction::MarkSearch(dest, search, marks, time, fallback) => {
                    let marks = self.marks(marks)?;
                    let time = self.scratch.floats[time.0];
                    let fallback = self.scratch.floats[fallback.0];
                    let value = match search {
                        MarkSearchInstruction::Prev => mark_prev(marks, time),
                        MarkSearchInstruction::Next => mark_next(marks, time),
                        MarkSearchInstruction::Nearest => mark_nearest(marks, time),
                        MarkSearchInstruction::Phase => mark_phase(marks, time),
                        MarkSearchInstruction::Elapsed => mark_elapsed(marks, time),
                    }
                    .unwrap_or(fallback);
                    self.scratch.floats[dest.0] = value;
                }
                Instruction::Min(dest, left, right) => {
                    self.scratch.floats[dest.0] =
                        self.scratch.floats[left.0].min(self.scratch.floats[right.0]);
                }
                Instruction::Max(dest, left, right) => {
                    self.scratch.floats[dest.0] =
                        self.scratch.floats[left.0].max(self.scratch.floats[right.0]);
                }
                Instruction::Clamp(dest, value, min, max) => {
                    self.scratch.floats[dest.0] = self.scratch.floats[value.0]
                        .clamp(self.scratch.floats[min.0], self.scratch.floats[max.0]);
                }
                Instruction::Smoothstep(dest, edge0, edge1, value) => {
                    let x = ((self.scratch.floats[value.0] - self.scratch.floats[edge0.0])
                        / (self.scratch.floats[edge1.0] - self.scratch.floats[edge0.0]))
                        .clamp(0.0, 1.0);
                    self.scratch.floats[dest.0] = x * x * (3.0 - 2.0 * x);
                }
                Instruction::MixFloat(dest, left, right, amount) => {
                    let left = self.scratch.floats[left.0];
                    let right = self.scratch.floats[right.0];
                    self.scratch.floats[dest.0] =
                        left + (right - left) * self.scratch.floats[amount.0];
                }
                Instruction::MixColor(dest, left, right, amount) => {
                    self.scratch.colors[dest.0] = self.scratch.colors[left.0]
                        .mix(self.scratch.colors[right.0], self.scratch.floats[amount.0]);
                }
                Instruction::Rgb(dest, red, green, blue) => {
                    self.scratch.colors[dest.0] = Color::new(
                        self.color_channel(red),
                        self.color_channel(green),
                        self.color_channel(blue),
                    );
                }
                Instruction::Hsv(dest, hue, saturation, value) => {
                    self.scratch.colors[dest.0] = hsv_to_rgb(
                        self.scratch.floats[hue.0],
                        self.scratch.floats[saturation.0],
                        self.scratch.floats[value.0],
                    );
                }
                Instruction::CallCurveParam(dest, index, amount) => {
                    let RuntimeValue::Curve(curve) = &self.params.values[index] else {
                        return Err(self.error("expected curve parameter"));
                    };
                    match curve.evaluate(self.scratch.floats[amount.0]) {
                        Some(CurveValue::Float(value)) => self.write_float_dest(dest, value),
                        Some(CurveValue::Color(value)) => self.write_color_dest(dest, value),
                        None => return Err(self.error("curve has no points")),
                    }
                }
                Instruction::ReturnColor(source) => return Ok(self.scratch.colors[source.0]),
            }
        }
        Err(self.error("sample did not return"))
    }

    fn write_runtime(
        &mut self,
        dest: ValueSlot,
        value: &'a RuntimeValue,
    ) -> Result<(), RuntimeError> {
        match (dest, value) {
            (ValueSlot::Float(slot), RuntimeValue::Float(value)) => {
                self.scratch.floats[slot.0] = *value
            }
            (ValueSlot::Float(slot), RuntimeValue::Int(value)) => {
                self.scratch.floats[slot.0] = *value as f64
            }
            (ValueSlot::Int(slot), RuntimeValue::Int(value)) => self.scratch.ints[slot.0] = *value,
            (ValueSlot::Bool(slot), RuntimeValue::Bool(value)) => {
                self.scratch.bools[slot.0] = *value
            }
            (ValueSlot::Color(slot), RuntimeValue::Color(value)) => {
                self.scratch.colors[slot.0] = *value
            }
            (ValueSlot::Fixture(slot), RuntimeValue::Fixture(value)) => {
                self.scratch.fixtures[slot.0] = *value
            }
            (ValueSlot::Pixel(slot), RuntimeValue::Pixel(value)) => {
                self.scratch.pixels[slot.0] = *value
            }
            _ => return Err(self.error("bytecode value type mismatch")),
        }
        Ok(())
    }

    fn write_ref_source(&mut self, slot: RefSlot, value: RefValue) {
        self.scratch.refs[slot.0] = value;
    }

    fn write_context(&mut self, dest: ValueSlot, slot: ContextSlot) {
        match (dest, slot) {
            (ValueSlot::Float(dest), ContextSlot::Progress) => {
                self.scratch.floats[dest.0] = self.progress
            }
            (ValueSlot::Float(dest), ContextSlot::Seconds) => {
                self.scratch.floats[dest.0] = self.seconds
            }
            (ValueSlot::Fixture(dest), ContextSlot::Fixture) => {
                self.scratch.fixtures[dest.0] = self.fixture
            }
            (ValueSlot::Pixel(dest), ContextSlot::Pixel) => {
                self.scratch.pixels[dest.0] = self.pixel
            }
            _ => unreachable!("compiler emits matching context slots"),
        }
    }

    fn copy(&mut self, dest: ValueSlot, source: ValueSlot) {
        match (dest, source) {
            (ValueSlot::Float(dest), ValueSlot::Float(source)) => {
                self.scratch.floats[dest.0] = self.scratch.floats[source.0]
            }
            (ValueSlot::Int(dest), ValueSlot::Int(source)) => {
                self.scratch.ints[dest.0] = self.scratch.ints[source.0]
            }
            (ValueSlot::Bool(dest), ValueSlot::Bool(source)) => {
                self.scratch.bools[dest.0] = self.scratch.bools[source.0]
            }
            (ValueSlot::Color(dest), ValueSlot::Color(source)) => {
                self.scratch.colors[dest.0] = self.scratch.colors[source.0]
            }
            (ValueSlot::Ref(dest, _), ValueSlot::Ref(source, _)) => {
                self.scratch.refs[dest.0] = self.scratch.refs[source.0]
            }
            (ValueSlot::Fixture(dest), ValueSlot::Fixture(source)) => {
                self.scratch.fixtures[dest.0] = self.scratch.fixtures[source.0]
            }
            (ValueSlot::Pixel(dest), ValueSlot::Pixel(source)) => {
                self.scratch.pixels[dest.0] = self.scratch.pixels[source.0]
            }
            _ => unreachable!("compiler emits matching copy slots"),
        }
    }

    fn eval_binary(
        &mut self,
        dest: ValueSlot,
        op: BinaryInstruction,
        left: ValueSlot,
        right: ValueSlot,
    ) -> Result<(), RuntimeError> {
        match op {
            BinaryInstruction::FloatAdd => {
                self.write_float_dest(dest, self.float(left) + self.float(right))
            }
            BinaryInstruction::FloatSubtract => {
                self.write_float_dest(dest, self.float(left) - self.float(right))
            }
            BinaryInstruction::FloatMultiply => {
                self.write_float_dest(dest, self.float(left) * self.float(right))
            }
            BinaryInstruction::FloatDivide => {
                self.write_float_dest(dest, self.float(left) / self.float(right))
            }
            BinaryInstruction::IntAdd => self.write_int_dest(
                dest,
                self.int(left)
                    .checked_add(self.int(right))
                    .ok_or_else(|| self.error("integer overflow"))?,
            ),
            BinaryInstruction::IntSubtract => self.write_int_dest(
                dest,
                self.int(left)
                    .checked_sub(self.int(right))
                    .ok_or_else(|| self.error("integer overflow"))?,
            ),
            BinaryInstruction::IntMultiply => self.write_int_dest(
                dest,
                self.int(left)
                    .checked_mul(self.int(right))
                    .ok_or_else(|| self.error("integer overflow"))?,
            ),
            BinaryInstruction::IntDivide => {
                let right = self.int(right);
                if right == 0 {
                    return Err(self.error("integer divide by zero"));
                }
                self.write_int_dest(
                    dest,
                    self.int(left)
                        .checked_div(right)
                        .ok_or_else(|| self.error("integer overflow"))?,
                );
            }
            BinaryInstruction::FloatLess => {
                self.write_bool_dest(dest, self.float(left) < self.float(right))
            }
            BinaryInstruction::FloatLessEqual => {
                self.write_bool_dest(dest, self.float(left) <= self.float(right))
            }
            BinaryInstruction::FloatGreater => {
                self.write_bool_dest(dest, self.float(left) > self.float(right))
            }
            BinaryInstruction::FloatGreaterEqual => {
                self.write_bool_dest(dest, self.float(left) >= self.float(right))
            }
            BinaryInstruction::IntLess => {
                self.write_bool_dest(dest, self.int(left) < self.int(right))
            }
            BinaryInstruction::IntLessEqual => {
                self.write_bool_dest(dest, self.int(left) <= self.int(right))
            }
            BinaryInstruction::IntGreater => {
                self.write_bool_dest(dest, self.int(left) > self.int(right))
            }
            BinaryInstruction::IntGreaterEqual => {
                self.write_bool_dest(dest, self.int(left) >= self.int(right))
            }
            BinaryInstruction::FloatEqual => {
                self.write_bool_dest(dest, self.float(left) == self.float(right))
            }
            BinaryInstruction::FloatNotEqual => {
                self.write_bool_dest(dest, self.float(left) != self.float(right))
            }
            BinaryInstruction::IntEqual => {
                self.write_bool_dest(dest, self.int(left) == self.int(right))
            }
            BinaryInstruction::IntNotEqual => {
                self.write_bool_dest(dest, self.int(left) != self.int(right))
            }
            BinaryInstruction::BoolEqual => {
                self.write_bool_dest(dest, self.bool(left) == self.bool(right))
            }
            BinaryInstruction::BoolNotEqual => {
                self.write_bool_dest(dest, self.bool(left) != self.bool(right))
            }
            BinaryInstruction::ColorMultiplyFloat => {
                self.write_color_dest(dest, self.color(left).scale(self.float(right)));
            }
            BinaryInstruction::FloatMultiplyColor => {
                self.write_color_dest(dest, self.color(right).scale(self.float(left)));
            }
        }
        Ok(())
    }

    fn write_float_dest(&mut self, dest: ValueSlot, value: f64) {
        self.scratch.floats[dest.float().0] = value;
    }

    fn write_int_dest(&mut self, dest: ValueSlot, value: i64) {
        self.scratch.ints[dest.int().0] = value;
    }

    fn write_bool_dest(&mut self, dest: ValueSlot, value: bool) {
        self.scratch.bools[dest.bool().0] = value;
    }

    fn write_color_dest(&mut self, dest: ValueSlot, value: Color) {
        self.scratch.colors[dest.color().0] = value;
    }

    fn float(&self, slot: ValueSlot) -> f64 {
        self.scratch.floats[slot.float().0]
    }

    fn int(&self, slot: ValueSlot) -> i64 {
        self.scratch.ints[slot.int().0]
    }

    fn bool(&self, slot: ValueSlot) -> bool {
        self.scratch.bools[slot.bool().0]
    }

    fn color(&self, slot: ValueSlot) -> Color {
        self.scratch.colors[slot.color().0]
    }

    fn marks(&self, slot: RefSlot) -> Result<&[f64], RuntimeError> {
        let value = match self.scratch.refs[slot.0] {
            RefValue::Param(index) => &self.params.values[index],
            RefValue::Constant(index) => &self.program.constants[index],
            RefValue::Unset => return Err(self.error("expected marks value")),
        };
        match value {
            RuntimeValue::Marks(value) => Ok(value),
            _ => Err(self.error("expected marks value")),
        }
    }

    fn color_channel(&self, slot: FloatSlot) -> u8 {
        self.scratch.floats[slot.0].round().clamp(0.0, 255.0) as u8
    }

    fn error(&self, message: &str) -> RuntimeError {
        RuntimeError {
            message: message.to_string(),
        }
    }

    fn rand(&mut self) -> f64 {
        self.rng = self
            .rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.rng >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

fn seed_from_float(value: f64) -> u64 {
    let mut seed = value.to_bits();
    seed ^= seed >> 30;
    seed = seed.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    seed ^= seed >> 27;
    seed = seed.wrapping_mul(0x94d0_49bb_1331_11eb);
    seed ^ (seed >> 31)
}

fn hsv_to_rgb(hue: f64, saturation: f64, value: f64) -> Color {
    let hue = hue.rem_euclid(360.0) / 60.0;
    let c = value.clamp(0.0, 1.0) * saturation.clamp(0.0, 1.0);
    let x = c * (1.0 - ((hue % 2.0) - 1.0).abs());
    let m = value.clamp(0.0, 1.0) - c;
    let (red, green, blue) = if hue < 1.0 {
        (c, x, 0.0)
    } else if hue < 2.0 {
        (x, c, 0.0)
    } else if hue < 3.0 {
        (0.0, c, x)
    } else if hue < 4.0 {
        (0.0, x, c)
    } else if hue < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Color::new(
        ((red + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((green + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((blue + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    )
}

fn mark_prev(marks: &[f64], time: f64) -> Option<f64> {
    let index = marks.partition_point(|mark| *mark <= time);
    index.checked_sub(1).map(|index| marks[index])
}

fn mark_next(marks: &[f64], time: f64) -> Option<f64> {
    marks
        .get(marks.partition_point(|mark| *mark <= time))
        .copied()
}

fn mark_nearest(marks: &[f64], time: f64) -> Option<f64> {
    let previous = mark_prev(marks, time);
    let next = mark_next(marks, time);
    match (previous, next) {
        (Some(previous), Some(next)) if (time - previous) <= (next - time) => Some(previous),
        (Some(_), Some(next)) => Some(next),
        (Some(previous), None) => Some(previous),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
}

fn mark_phase(marks: &[f64], time: f64) -> Option<f64> {
    let previous = mark_prev(marks, time)?;
    if (time - previous).abs() < f64::EPSILON {
        return Some(0.0);
    }
    let next = mark_next(marks, time)?;
    let span = next - previous;
    if span <= f64::EPSILON {
        return None;
    }
    Some(((time - previous) / span).clamp(0.0, 1.0))
}

fn mark_elapsed(marks: &[f64], time: f64) -> Option<f64> {
    mark_prev(marks, time).map(|previous| time - previous)
}
