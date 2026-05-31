use std::collections::HashMap;

use crate::model::{Color, Curve, CurveValue, Flags};

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
    pub(super) local_slots: usize,
    pub(super) max_stack_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytecodeStats {
    pub instruction_count: usize,
    pub constant_count: usize,
    pub param_slots: usize,
    pub local_slots: usize,
    pub max_stack_depth: usize,
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

    pub fn local_slots(&self) -> usize {
        self.local_slots
    }

    pub fn max_stack_depth(&self) -> usize {
        self.max_stack_depth
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedEffectParams {
    values: Vec<RuntimeValue>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum VmValue<'a> {
    Float(f64),
    Int(i64),
    Bool(bool),
    Color(Color),
    Marks(&'a [f64]),
    Curve(&'a Curve),
    Enum(&'a str),
    Flags(&'a Flags),
    Fixture(FixtureContext),
    Pixel(PixelContext),
    Unset,
}

impl<'a> VmValue<'a> {
    fn from_runtime(value: &'a RuntimeValue) -> Self {
        match value {
            RuntimeValue::Float(value) => Self::Float(*value),
            RuntimeValue::Int(value) => Self::Int(*value),
            RuntimeValue::Bool(value) => Self::Bool(*value),
            RuntimeValue::Color(value) => Self::Color(*value),
            RuntimeValue::Marks(value) => Self::Marks(value),
            RuntimeValue::Curve(value) => Self::Curve(value),
            RuntimeValue::Enum(value) => Self::Enum(value),
            RuntimeValue::Flags(value) => Self::Flags(value),
            RuntimeValue::Fixture(value) => Self::Fixture(*value),
            RuntimeValue::Pixel(value) => Self::Pixel(*value),
        }
    }

    fn value_type(self) -> Option<ScriptType> {
        match self {
            Self::Float(_) => Some(ScriptType::Float),
            Self::Int(_) => Some(ScriptType::Int),
            Self::Bool(_) => Some(ScriptType::Bool),
            Self::Color(_) => Some(ScriptType::Color),
            Self::Marks(_) => Some(ScriptType::Marks),
            Self::Curve(curve) => Some(match curve.value_type {
                crate::model::CurveValueType::Float => ScriptType::CurveFloat,
                crate::model::CurveValueType::Color => ScriptType::CurveColor,
            }),
            Self::Enum(_) => Some(ScriptType::Enum),
            Self::Flags(_) => Some(ScriptType::Flags),
            Self::Fixture(_) => Some(ScriptType::Fixture),
            Self::Pixel(_) => Some(ScriptType::Pixel),
            Self::Unset => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Instruction {
    LoadConst(usize),
    LoadContext(ContextSlot),
    LoadParam(usize),
    LoadLocal(usize),
    StoreLocal(usize, ScriptType),
    Unary(UnaryOp),
    IntToFloat,
    Binary(BinaryInstruction),
    JumpIfFalse(usize),
    JumpIfTrue(usize),
    JumpIfFalsePop(usize),
    Jump(usize),
    Pop,
    LoopTick,
    Sin,
    Cos,
    Abs,
    Floor,
    Srand,
    Rand,
    PixelIndex,
    PixelCount,
    MarkCount,
    MarkAt,
    MarkPrev,
    MarkNext,
    MarkNearest,
    MarkPhase,
    MarkElapsed,
    Min,
    Max,
    Clamp,
    Smoothstep,
    MixFloat,
    MixColor,
    Rgb,
    Hsv,
    CallCurveParam(usize),
    ReturnColor,
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
pub(super) enum ContextSlot {
    Progress,
    Seconds,
    Fixture,
    Pixel,
}

#[derive(Debug, Clone, Copy)]
enum Binding {
    Context(ContextSlot),
    Constant(usize),
    Param {
        index: usize,
        value_type: ScriptType,
    },
    Local {
        slot: usize,
        value_type: ScriptType,
    },
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
        BytecodeVm {
            program,
            progress,
            seconds,
            fixture,
            pixel,
            params,
            stack: Vec::with_capacity(program.max_stack_depth),
            locals: vec![VmValue::Unset; program.local_slots],
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

    fn coerce_vm_value<'a>(
        value: VmValue<'a>,
        expected: ScriptType,
    ) -> Result<VmValue<'a>, RuntimeError> {
        match (expected, value) {
            (ScriptType::Float, VmValue::Int(value)) => Ok(VmValue::Float(value as f64)),
            (expected, value) if value.value_type() == Some(expected) => Ok(value),
            (expected, value) => Err(RuntimeError {
                message: format!(
                    "expected {expected} value, but found {}",
                    value
                        .value_type()
                        .map(|value_type| value_type.to_string())
                        .unwrap_or_else(|| "unset".to_string())
                ),
            }),
        }
    }
}

struct Compiler<'a> {
    effect: &'a EffectAst,
    instructions: Vec<Instruction>,
    constants: Vec<RuntimeValue>,
    scopes: Vec<HashMap<String, Binding>>,
    local_slots: usize,
    stack_depth: usize,
    max_stack_depth: usize,
}

impl<'a> Compiler<'a> {
    fn new(effect: &'a EffectAst) -> Self {
        let mut compiler = Self {
            effect,
            instructions: Vec::new(),
            constants: Vec::new(),
            scopes: vec![HashMap::new()],
            local_slots: 0,
            stack_depth: 0,
            max_stack_depth: 0,
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
            local_slots: self.local_slots,
            max_stack_depth: self.max_stack_depth,
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
                let slot = self.allocate_local();
                self.compile_expr(expr);
                self.emit(Instruction::StoreLocal(slot, *value_type), -1);
                self.define(
                    name,
                    Binding::Local {
                        slot,
                        value_type: *value_type,
                    },
                );
            }
            Stmt::Assign { name, expr } => {
                let Binding::Local { slot, value_type } = self.expect_binding(name) else {
                    unreachable!("type checker rejects assignment to non-local bindings");
                };
                self.compile_expr(expr);
                self.emit(Instruction::StoreLocal(slot, value_type), -1);
            }
            Stmt::Expr(expr) => {
                self.compile_expr(expr);
                self.emit(Instruction::Pop, -1);
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
                let slot = self.allocate_local();
                self.compile_expr(initializer);
                self.emit(Instruction::StoreLocal(slot, *value_type), -1);
                self.define(
                    name,
                    Binding::Local {
                        slot,
                        value_type: *value_type,
                    },
                );
                let loop_start = self.instructions.len();
                self.compile_expr(condition);
                let exit_jump = self.emit_jump_if_false_pop();
                self.emit(Instruction::LoopTick, 0);
                self.push_scope();
                for statement in body {
                    self.compile_statement(statement);
                }
                self.pop_scope();
                self.compile_statement(update);
                self.emit(Instruction::Jump(loop_start), 0);
                self.patch_jump(exit_jump);
                self.pop_scope();
            }
            Stmt::If {
                condition,
                then_body,
                else_body,
            } => {
                self.compile_expr(condition);
                let else_jump = self.emit_jump_if_false_pop();
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
                self.compile_expr(expr);
                self.emit(Instruction::ReturnColor, -1);
            }
        }
    }

    fn compile_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Float(value) => self.load_constant(RuntimeValue::Float(*value)),
            Expr::Int(value) => self.load_constant(RuntimeValue::Int(*value)),
            Expr::Bool(value) => self.load_constant(RuntimeValue::Bool(*value)),
            Expr::Color(value) => self.load_constant(RuntimeValue::Color(*value)),
            Expr::Ident(name) => match self.expect_binding(name) {
                Binding::Context(slot) => self.emit(Instruction::LoadContext(slot), 1),
                Binding::Constant(index) => self.emit(Instruction::LoadConst(index), 1),
                Binding::Param { index, .. } => self.emit(Instruction::LoadParam(index), 1),
                Binding::Local { slot, .. } => self.emit(Instruction::LoadLocal(slot), 1),
            },
            Expr::Unary { op, expr } => {
                self.compile_expr(expr);
                self.emit(Instruction::Unary(*op), 0);
            }
            Expr::Binary { left, op, right } => self.compile_binary(left, *op, right),
            Expr::Call { name, args } => self.compile_call(name, args),
        }
    }

    fn compile_binary(&mut self, left: &Expr, op: BinaryOp, right: &Expr) {
        match op {
            BinaryOp::LogicalAnd => {
                self.compile_expr(left);
                let false_jump = self.emit_jump_if_false();
                self.compile_expr(right);
                self.patch_jump(false_jump);
            }
            BinaryOp::LogicalOr => {
                self.compile_expr(left);
                let true_jump = self.emit_jump_if_true();
                self.compile_expr(right);
                self.patch_jump(true_jump);
            }
            _ => {
                let left_type = self.expr_type(left);
                let right_type = self.expr_type(right);
                self.compile_expr(left);
                if left_type == ScriptType::Int
                    && right_type == ScriptType::Float
                    && binary_result_type(left_type, op, right_type).is_some()
                {
                    self.emit(Instruction::IntToFloat, 0);
                }
                if matches!(
                    (left_type, op, right_type),
                    (ScriptType::Int, BinaryOp::Multiply, ScriptType::Color)
                ) {
                    self.emit(Instruction::IntToFloat, 0);
                }
                self.compile_expr(right);
                if right_type == ScriptType::Int
                    && left_type == ScriptType::Float
                    && binary_result_type(left_type, op, right_type).is_some()
                {
                    self.emit(Instruction::IntToFloat, 0);
                }
                if matches!(
                    (left_type, op, right_type),
                    (ScriptType::Color, BinaryOp::Multiply, ScriptType::Int)
                ) {
                    self.emit(Instruction::IntToFloat, 0);
                }
                let instruction = self.binary_instruction(left_type, op, right_type);
                self.emit(Instruction::Binary(instruction), -1);
            }
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
                Binding::Param { value_type, .. } | Binding::Local { value_type, .. } => value_type,
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

    fn compile_call(&mut self, name: &str, args: &[Expr]) {
        if let Some(Binding::Param { index, value_type }) = self.binding(name) {
            if matches!(value_type, ScriptType::CurveFloat | ScriptType::CurveColor) {
                self.compile_expr(&args[0]);
                self.emit(Instruction::CallCurveParam(index), 0);
                return;
            }
        }
        for arg in args {
            self.compile_expr(arg);
        }
        let instruction = match name {
            "sin" => Instruction::Sin,
            "cos" => Instruction::Cos,
            "abs" => Instruction::Abs,
            "floor" => Instruction::Floor,
            "srand" => Instruction::Srand,
            "rand" => Instruction::Rand,
            "pixel_index" => Instruction::PixelIndex,
            "pixel_count" => Instruction::PixelCount,
            "mark_count" => Instruction::MarkCount,
            "mark_at" => Instruction::MarkAt,
            "mark_prev" => Instruction::MarkPrev,
            "mark_next" => Instruction::MarkNext,
            "mark_nearest" => Instruction::MarkNearest,
            "mark_phase" => Instruction::MarkPhase,
            "mark_elapsed" => Instruction::MarkElapsed,
            "min" => Instruction::Min,
            "max" => Instruction::Max,
            "clamp" => Instruction::Clamp,
            "smoothstep" => Instruction::Smoothstep,
            "mix" if self.call_type(name, args) == ScriptType::Color => Instruction::MixColor,
            "mix" => Instruction::MixFloat,
            "rgb" => Instruction::Rgb,
            "hsv" => Instruction::Hsv,
            _ => unreachable!("type checker validates builtins"),
        };
        self.emit(instruction, 1 - args.len() as isize);
    }

    fn load_constant(&mut self, value: RuntimeValue) {
        let index = self.add_constant(value);
        self.emit(Instruction::LoadConst(index), 1);
    }

    fn add_constant(&mut self, value: RuntimeValue) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    fn emit_jump_if_false(&mut self) -> usize {
        let index = self.instructions.len();
        self.emit(Instruction::JumpIfFalse(usize::MAX), 0);
        index
    }

    fn emit_jump_if_true(&mut self) -> usize {
        let index = self.instructions.len();
        self.emit(Instruction::JumpIfTrue(usize::MAX), 0);
        index
    }

    fn emit_jump_if_false_pop(&mut self) -> usize {
        let index = self.instructions.len();
        self.emit(Instruction::JumpIfFalsePop(usize::MAX), -1);
        index
    }

    fn emit_jump(&mut self) -> usize {
        let index = self.instructions.len();
        self.emit(Instruction::Jump(usize::MAX), 0);
        index
    }

    fn patch_jump(&mut self, index: usize) {
        let target = self.instructions.len();
        match &mut self.instructions[index] {
            Instruction::JumpIfFalse(slot)
            | Instruction::JumpIfTrue(slot)
            | Instruction::JumpIfFalsePop(slot)
            | Instruction::Jump(slot) => {
                *slot = target;
            }
            _ => unreachable!("patch target must be a jump"),
        }
    }

    fn emit(&mut self, instruction: Instruction, stack_delta: isize) {
        self.instructions.push(instruction);
        if stack_delta < 0 {
            self.stack_depth -= stack_delta.unsigned_abs();
        } else {
            self.stack_depth += stack_delta as usize;
            self.max_stack_depth = self.max_stack_depth.max(self.stack_depth);
        }
    }

    fn allocate_local(&mut self) -> usize {
        let slot = self.local_slots;
        self.local_slots += 1;
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

struct BytecodeVm<'a> {
    program: &'a BytecodeProgram,
    progress: f64,
    seconds: f64,
    fixture: FixtureContext,
    pixel: PixelContext,
    params: &'a PreparedEffectParams,
    stack: Vec<VmValue<'a>>,
    locals: Vec<VmValue<'a>>,
    ip: usize,
    rng: u64,
    loop_iterations: usize,
}

impl<'a> BytecodeVm<'a> {
    fn run(&mut self) -> Result<Color, RuntimeError> {
        while let Some(instruction) = self.program.instructions.get(self.ip) {
            self.ip += 1;
            match *instruction {
                Instruction::LoadConst(index) => {
                    self.stack
                        .push(VmValue::from_runtime(&self.program.constants[index]));
                }
                Instruction::LoadContext(slot) => self.stack.push(match slot {
                    ContextSlot::Progress => VmValue::Float(self.progress),
                    ContextSlot::Seconds => VmValue::Float(self.seconds),
                    ContextSlot::Fixture => VmValue::Fixture(self.fixture),
                    ContextSlot::Pixel => VmValue::Pixel(self.pixel),
                }),
                Instruction::LoadParam(index) => {
                    self.stack
                        .push(VmValue::from_runtime(&self.params.values[index]));
                }
                Instruction::LoadLocal(index) => self.stack.push(self.locals[index]),
                Instruction::StoreLocal(index, value_type) => {
                    let value = Vm::coerce_vm_value(self.pop()?, value_type)?;
                    self.locals[index] = value;
                }
                Instruction::Unary(op) => {
                    let value = self.pop()?;
                    self.stack.push(self.eval_unary(op, value)?);
                }
                Instruction::IntToFloat => {
                    let value = self.pop_int()? as f64;
                    self.stack.push(VmValue::Float(value));
                }
                Instruction::Binary(op) => {
                    let right = self.pop()?;
                    let left = self.pop()?;
                    self.stack.push(self.eval_binary(left, op, right)?);
                }
                Instruction::JumpIfFalse(target) => {
                    let value = self.peek_bool("logical branch condition was not bool")?;
                    if !value {
                        self.ip = target;
                    } else {
                        self.pop()?;
                    }
                }
                Instruction::JumpIfTrue(target) => {
                    let value = self.peek_bool("logical branch condition was not bool")?;
                    if value {
                        self.ip = target;
                    } else {
                        self.pop()?;
                    }
                }
                Instruction::JumpIfFalsePop(target) => {
                    let value = self.pop_bool("branch condition was not bool")?;
                    if !value {
                        self.ip = target;
                    }
                }
                Instruction::Jump(target) => self.ip = target,
                Instruction::Pop => {
                    self.pop()?;
                }
                Instruction::LoopTick => {
                    self.loop_iterations += 1;
                    if self.loop_iterations > MAX_LOOP_ITERATIONS {
                        return Err(self.error("effect exceeded the maximum loop iteration count"));
                    }
                }
                Instruction::Sin => {
                    let value = self.pop()?;
                    self.stack
                        .push(VmValue::Float(self.expect_float(value)?.sin()));
                }
                Instruction::Cos => {
                    let value = self.pop()?;
                    self.stack
                        .push(VmValue::Float(self.expect_float(value)?.cos()));
                }
                Instruction::Abs => {
                    let value = self.pop()?;
                    self.stack
                        .push(VmValue::Float(self.expect_float(value)?.abs()));
                }
                Instruction::Floor => {
                    let value = self.pop()?;
                    self.stack
                        .push(VmValue::Float(self.expect_float(value)?.floor()));
                }
                Instruction::Srand => {
                    let value = self.pop()?;
                    self.rng = seed_from_float(self.expect_float(value)?);
                    self.stack.push(VmValue::Float(0.0));
                }
                Instruction::Rand => {
                    let value = self.rand();
                    self.stack.push(VmValue::Float(value));
                }
                Instruction::PixelIndex => match self.pop()? {
                    VmValue::Pixel(pixel) => self.stack.push(VmValue::Int(pixel.index as i64)),
                    _ => return Err(self.error("expected pixel value")),
                },
                Instruction::PixelCount => match self.pop()? {
                    VmValue::Pixel(pixel) => self.stack.push(VmValue::Int(pixel.count as i64)),
                    _ => return Err(self.error("expected pixel value")),
                },
                Instruction::MarkCount => {
                    let marks = self.pop()?;
                    self.stack
                        .push(VmValue::Int(self.expect_marks(marks)?.len() as i64));
                }
                Instruction::MarkAt => {
                    let fallback = self.pop_float()?;
                    let index = self.pop_int()?;
                    let marks = self.pop_marks()?;
                    let value = usize::try_from(index)
                        .ok()
                        .and_then(|index| marks.get(index))
                        .copied()
                        .unwrap_or(fallback);
                    self.stack.push(VmValue::Float(value));
                }
                Instruction::MarkPrev => self.push_mark_search(mark_prev)?,
                Instruction::MarkNext => self.push_mark_search(mark_next)?,
                Instruction::MarkNearest => self.push_mark_search(mark_nearest)?,
                Instruction::MarkPhase => self.push_mark_search(mark_phase)?,
                Instruction::MarkElapsed => self.push_mark_search(mark_elapsed)?,
                Instruction::Min => {
                    let right = self.pop_float()?;
                    let left = self.pop_float()?;
                    self.stack.push(VmValue::Float(left.min(right)));
                }
                Instruction::Max => {
                    let right = self.pop_float()?;
                    let left = self.pop_float()?;
                    self.stack.push(VmValue::Float(left.max(right)));
                }
                Instruction::Clamp => {
                    let max = self.pop_float()?;
                    let min = self.pop_float()?;
                    let value = self.pop_float()?;
                    self.stack.push(VmValue::Float(value.clamp(min, max)));
                }
                Instruction::Smoothstep => {
                    let value = self.pop_float()?;
                    let edge1 = self.pop_float()?;
                    let edge0 = self.pop_float()?;
                    let x = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                    self.stack.push(VmValue::Float(x * x * (3.0 - 2.0 * x)));
                }
                Instruction::MixFloat => {
                    let amount = self.pop_float()?;
                    let right = self.pop_float()?;
                    let left = self.pop_float()?;
                    self.stack
                        .push(VmValue::Float(left + (right - left) * amount));
                }
                Instruction::MixColor => {
                    let amount = self.pop_float()?;
                    let VmValue::Color(right) = self.pop()? else {
                        return Err(self.error("expected color value"));
                    };
                    let VmValue::Color(left) = self.pop()? else {
                        return Err(self.error("expected color value"));
                    };
                    self.stack.push(VmValue::Color(left.mix(right, amount)));
                }
                Instruction::Rgb => {
                    let blue = self.pop_float()?;
                    let green = self.pop_float()?;
                    let red = self.pop_float()?;
                    self.stack.push(VmValue::Color(Color::new(
                        red.round().clamp(0.0, 255.0) as u8,
                        green.round().clamp(0.0, 255.0) as u8,
                        blue.round().clamp(0.0, 255.0) as u8,
                    )));
                }
                Instruction::Hsv => {
                    let value = self.pop_float()?;
                    let saturation = self.pop_float()?;
                    let hue = self.pop_float()?;
                    self.stack
                        .push(VmValue::Color(hsv_to_rgb(hue, saturation, value)));
                }
                Instruction::CallCurveParam(index) => {
                    let amount = self.pop_float()?;
                    let RuntimeValue::Curve(curve) = &self.params.values[index] else {
                        return Err(self.error("expected curve parameter"));
                    };
                    self.stack.push(match curve.evaluate(amount) {
                        Some(CurveValue::Float(value)) => VmValue::Float(value),
                        Some(CurveValue::Color(value)) => VmValue::Color(value),
                        None => return Err(self.error("curve has no points")),
                    });
                }
                Instruction::ReturnColor => {
                    let VmValue::Color(color) = self.pop()? else {
                        return Err(self.error("sample returned a non-color value"));
                    };
                    return Ok(color);
                }
            }
        }
        Err(self.error("sample did not return"))
    }

    fn eval_unary(&self, op: UnaryOp, value: VmValue<'a>) -> Result<VmValue<'a>, RuntimeError> {
        match (op, value) {
            (UnaryOp::Negate, VmValue::Float(value)) => Ok(VmValue::Float(-value)),
            (UnaryOp::Negate, VmValue::Int(value)) => value
                .checked_neg()
                .map(VmValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (UnaryOp::Not, VmValue::Bool(value)) => Ok(VmValue::Bool(!value)),
            _ => Err(self.error("invalid unary expression")),
        }
    }

    fn eval_binary(
        &self,
        left: VmValue<'a>,
        op: BinaryInstruction,
        right: VmValue<'a>,
    ) -> Result<VmValue<'a>, RuntimeError> {
        match op {
            BinaryInstruction::FloatAdd => Ok(VmValue::Float(
                self.expect_float(left)? + self.expect_float(right)?,
            )),
            BinaryInstruction::FloatSubtract => Ok(VmValue::Float(
                self.expect_float(left)? - self.expect_float(right)?,
            )),
            BinaryInstruction::FloatMultiply => Ok(VmValue::Float(
                self.expect_float(left)? * self.expect_float(right)?,
            )),
            BinaryInstruction::FloatDivide => Ok(VmValue::Float(
                self.expect_float(left)? / self.expect_float(right)?,
            )),
            BinaryInstruction::IntAdd => self
                .expect_int(left)?
                .checked_add(self.expect_int(right)?)
                .map(VmValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            BinaryInstruction::IntSubtract => self
                .expect_int(left)?
                .checked_sub(self.expect_int(right)?)
                .map(VmValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            BinaryInstruction::IntMultiply => self
                .expect_int(left)?
                .checked_mul(self.expect_int(right)?)
                .map(VmValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            BinaryInstruction::IntDivide => {
                let right = self.expect_int(right)?;
                if right == 0 {
                    return Err(self.error("integer divide by zero"));
                }
                self.expect_int(left)?
                    .checked_div(right)
                    .map(VmValue::Int)
                    .ok_or_else(|| self.error("integer overflow"))
            }
            BinaryInstruction::FloatLess => Ok(VmValue::Bool(
                self.expect_float(left)? < self.expect_float(right)?,
            )),
            BinaryInstruction::FloatLessEqual => Ok(VmValue::Bool(
                self.expect_float(left)? <= self.expect_float(right)?,
            )),
            BinaryInstruction::FloatGreater => Ok(VmValue::Bool(
                self.expect_float(left)? > self.expect_float(right)?,
            )),
            BinaryInstruction::FloatGreaterEqual => Ok(VmValue::Bool(
                self.expect_float(left)? >= self.expect_float(right)?,
            )),
            BinaryInstruction::IntLess => Ok(VmValue::Bool(
                self.expect_int(left)? < self.expect_int(right)?,
            )),
            BinaryInstruction::IntLessEqual => Ok(VmValue::Bool(
                self.expect_int(left)? <= self.expect_int(right)?,
            )),
            BinaryInstruction::IntGreater => Ok(VmValue::Bool(
                self.expect_int(left)? > self.expect_int(right)?,
            )),
            BinaryInstruction::IntGreaterEqual => Ok(VmValue::Bool(
                self.expect_int(left)? >= self.expect_int(right)?,
            )),
            BinaryInstruction::FloatEqual => Ok(VmValue::Bool(
                self.expect_float(left)? == self.expect_float(right)?,
            )),
            BinaryInstruction::FloatNotEqual => Ok(VmValue::Bool(
                self.expect_float(left)? != self.expect_float(right)?,
            )),
            BinaryInstruction::IntEqual => Ok(VmValue::Bool(
                self.expect_int(left)? == self.expect_int(right)?,
            )),
            BinaryInstruction::IntNotEqual => Ok(VmValue::Bool(
                self.expect_int(left)? != self.expect_int(right)?,
            )),
            BinaryInstruction::BoolEqual => {
                let (VmValue::Bool(left), VmValue::Bool(right)) = (left, right) else {
                    return Err(self.error("expected bool value"));
                };
                Ok(VmValue::Bool(left == right))
            }
            BinaryInstruction::BoolNotEqual => {
                let (VmValue::Bool(left), VmValue::Bool(right)) = (left, right) else {
                    return Err(self.error("expected bool value"));
                };
                Ok(VmValue::Bool(left != right))
            }
            BinaryInstruction::ColorMultiplyFloat => {
                let VmValue::Color(color) = left else {
                    return Err(self.error("expected color value"));
                };
                Ok(VmValue::Color(color.scale(self.expect_float(right)?)))
            }
            BinaryInstruction::FloatMultiplyColor => {
                let VmValue::Color(color) = right else {
                    return Err(self.error("expected color value"));
                };
                Ok(VmValue::Color(color.scale(self.expect_float(left)?)))
            }
        }
    }

    fn push_mark_search(
        &mut self,
        search: fn(&[f64], f64) -> Option<f64>,
    ) -> Result<(), RuntimeError> {
        let fallback = self.pop_float()?;
        let time = self.pop_float()?;
        let marks = self.pop_marks()?;
        let value = search(marks, time).unwrap_or(fallback);
        self.stack.push(VmValue::Float(value));
        Ok(())
    }

    fn expect_float(&self, value: VmValue<'a>) -> Result<f64, RuntimeError> {
        match value {
            VmValue::Float(value) => Ok(value),
            VmValue::Int(value) => Ok(value as f64),
            _ => Err(self.error("expected float value")),
        }
    }

    fn expect_int(&self, value: VmValue<'a>) -> Result<i64, RuntimeError> {
        match value {
            VmValue::Int(value) => Ok(value),
            _ => Err(self.error("expected int value")),
        }
    }

    fn expect_marks(&self, value: VmValue<'a>) -> Result<&'a [f64], RuntimeError> {
        match value {
            VmValue::Marks(value) => Ok(value),
            _ => Err(self.error("expected marks value")),
        }
    }

    fn peek_bool(&self, message: &str) -> Result<bool, RuntimeError> {
        match self.stack.last() {
            Some(VmValue::Bool(value)) => Ok(*value),
            _ => Err(self.error(message)),
        }
    }

    fn pop_bool(&mut self, message: &str) -> Result<bool, RuntimeError> {
        match self.pop()? {
            VmValue::Bool(value) => Ok(value),
            _ => Err(self.error(message)),
        }
    }

    fn pop(&mut self) -> Result<VmValue<'a>, RuntimeError> {
        self.stack
            .pop()
            .ok_or_else(|| self.error("stack underflow"))
    }

    fn pop_float(&mut self) -> Result<f64, RuntimeError> {
        let value = self.pop()?;
        self.expect_float(value)
    }

    fn pop_int(&mut self) -> Result<i64, RuntimeError> {
        let value = self.pop()?;
        self.expect_int(value)
    }

    fn pop_marks(&mut self) -> Result<&[f64], RuntimeError> {
        let value = self.pop()?;
        self.expect_marks(value)
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
