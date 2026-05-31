use std::collections::HashMap;

use crate::model::{Color, CurveValue};

use super::ast::{BinaryOp, EffectAst, Expr, Stmt, UnaryOp};
use super::{
    EffectParamSchema, FixtureContext, ParamDefault, PixelContext, RuntimeError, RuntimeValue,
    ScriptDiagnostic, ScriptType,
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

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Instruction {
    LoadConst(usize),
    LoadContext(ContextSlot),
    LoadParam(usize),
    LoadLocal(usize),
    StoreLocal(usize, ScriptType),
    Unary(UnaryOp),
    Binary(BinaryOp),
    JumpIfFalse(usize),
    JumpIfTrue(usize),
    JumpIfFalsePop(usize),
    Jump(usize),
    Pop,
    LoopTick,
    Call(BuiltinFn),
    CallCurveParam(usize),
    ReturnColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContextSlot {
    Progress,
    Seconds,
    Fixture,
    Pixel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltinFn {
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
    Mix,
    Rgb,
    Hsv,
}

impl BuiltinFn {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "sin" => Some(Self::Sin),
            "cos" => Some(Self::Cos),
            "abs" => Some(Self::Abs),
            "floor" => Some(Self::Floor),
            "srand" => Some(Self::Srand),
            "rand" => Some(Self::Rand),
            "pixel_index" => Some(Self::PixelIndex),
            "pixel_count" => Some(Self::PixelCount),
            "mark_count" => Some(Self::MarkCount),
            "mark_at" => Some(Self::MarkAt),
            "mark_prev" => Some(Self::MarkPrev),
            "mark_next" => Some(Self::MarkNext),
            "mark_nearest" => Some(Self::MarkNearest),
            "mark_phase" => Some(Self::MarkPhase),
            "mark_elapsed" => Some(Self::MarkElapsed),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            "clamp" => Some(Self::Clamp),
            "smoothstep" => Some(Self::Smoothstep),
            "mix" => Some(Self::Mix),
            "rgb" => Some(Self::Rgb),
            "hsv" => Some(Self::Hsv),
            _ => None,
        }
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
        let mut prepared = Vec::with_capacity(params.len());
        for param in params {
            let value = values
                .get(&param.name)
                .cloned()
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
            locals: vec![RuntimeValue::Bool(false); program.local_slots],
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
                self.compile_expr(left);
                self.compile_expr(right);
                self.emit(Instruction::Binary(op), -1);
            }
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
        let builtin = BuiltinFn::from_name(name).expect("type checker validates builtins");
        self.emit(Instruction::Call(builtin), 1 - args.len() as isize);
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

struct BytecodeVm<'a> {
    program: &'a BytecodeProgram,
    progress: f64,
    seconds: f64,
    fixture: FixtureContext,
    pixel: PixelContext,
    params: &'a PreparedEffectParams,
    stack: Vec<RuntimeValue>,
    locals: Vec<RuntimeValue>,
    ip: usize,
    rng: u64,
    loop_iterations: usize,
}

impl BytecodeVm<'_> {
    fn run(&mut self) -> Result<Color, RuntimeError> {
        while let Some(instruction) = self.program.instructions.get(self.ip) {
            self.ip += 1;
            match *instruction {
                Instruction::LoadConst(index) => {
                    self.stack.push(self.program.constants[index].clone());
                }
                Instruction::LoadContext(slot) => self.stack.push(match slot {
                    ContextSlot::Progress => RuntimeValue::Float(self.progress),
                    ContextSlot::Seconds => RuntimeValue::Float(self.seconds),
                    ContextSlot::Fixture => RuntimeValue::Fixture(self.fixture),
                    ContextSlot::Pixel => RuntimeValue::Pixel(self.pixel),
                }),
                Instruction::LoadParam(index) => self.stack.push(self.params.values[index].clone()),
                Instruction::LoadLocal(index) => self.stack.push(self.locals[index].clone()),
                Instruction::StoreLocal(index, value_type) => {
                    let value = Vm::coerce_value(self.pop()?, value_type)?;
                    self.locals[index] = value;
                }
                Instruction::Unary(op) => {
                    let value = self.pop()?;
                    self.stack.push(self.eval_unary(op, value)?);
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
                Instruction::Call(function) => {
                    let value = self.eval_call(function)?;
                    self.stack.push(value);
                }
                Instruction::CallCurveParam(index) => {
                    let amount = self.pop_float()?;
                    let RuntimeValue::Curve(curve) = &self.params.values[index] else {
                        return Err(self.error("expected curve parameter"));
                    };
                    self.stack.push(match curve.evaluate(amount) {
                        Some(CurveValue::Float(value)) => RuntimeValue::Float(value),
                        Some(CurveValue::Color(value)) => RuntimeValue::Color(value),
                        None => return Err(self.error("curve has no points")),
                    });
                }
                Instruction::ReturnColor => {
                    let RuntimeValue::Color(color) = self.pop()? else {
                        return Err(self.error("sample returned a non-color value"));
                    };
                    return Ok(color);
                }
            }
        }
        Err(self.error("sample did not return"))
    }

    fn eval_unary(&self, op: UnaryOp, value: RuntimeValue) -> Result<RuntimeValue, RuntimeError> {
        match (op, value) {
            (UnaryOp::Negate, RuntimeValue::Float(value)) => Ok(RuntimeValue::Float(-value)),
            (UnaryOp::Negate, RuntimeValue::Int(value)) => value
                .checked_neg()
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (UnaryOp::Not, RuntimeValue::Bool(value)) => Ok(RuntimeValue::Bool(!value)),
            _ => Err(self.error("invalid unary expression")),
        }
    }

    fn eval_binary(
        &self,
        left: RuntimeValue,
        op: BinaryOp,
        right: RuntimeValue,
    ) -> Result<RuntimeValue, RuntimeError> {
        match (left, op, right) {
            (RuntimeValue::Float(left), BinaryOp::Add, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left + right))
            }
            (RuntimeValue::Float(left), BinaryOp::Add, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left + right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Add, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 + right))
            }
            (RuntimeValue::Int(left), BinaryOp::Add, RuntimeValue::Int(right)) => left
                .checked_add(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Subtract, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left - right))
            }
            (RuntimeValue::Float(left), BinaryOp::Subtract, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left - right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Subtract, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 - right))
            }
            (RuntimeValue::Int(left), BinaryOp::Subtract, RuntimeValue::Int(right)) => left
                .checked_sub(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Multiply, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left * right))
            }
            (RuntimeValue::Float(left), BinaryOp::Multiply, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left * right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Multiply, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 * right))
            }
            (RuntimeValue::Int(left), BinaryOp::Multiply, RuntimeValue::Int(right)) => left
                .checked_mul(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Divide, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left / right))
            }
            (RuntimeValue::Float(left), BinaryOp::Divide, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Float(left / right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Divide, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Float(left as f64 / right))
            }
            (RuntimeValue::Int(_), BinaryOp::Divide, RuntimeValue::Int(0)) => {
                Err(self.error("integer divide by zero"))
            }
            (RuntimeValue::Int(left), BinaryOp::Divide, RuntimeValue::Int(right)) => left
                .checked_div(right)
                .map(RuntimeValue::Int)
                .ok_or_else(|| self.error("integer overflow")),
            (RuntimeValue::Float(left), BinaryOp::Less, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left < right))
            }
            (RuntimeValue::Float(left), BinaryOp::Less, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left < right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Less, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool((left as f64) < right))
            }
            (RuntimeValue::Int(left), BinaryOp::Less, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left < right))
            }
            (RuntimeValue::Float(left), BinaryOp::LessEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left <= right))
            }
            (RuntimeValue::Float(left), BinaryOp::LessEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left <= right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::LessEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool((left as f64) <= right))
            }
            (RuntimeValue::Int(left), BinaryOp::LessEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left <= right))
            }
            (RuntimeValue::Float(left), BinaryOp::Greater, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left > right))
            }
            (RuntimeValue::Float(left), BinaryOp::Greater, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left > right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Greater, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool((left as f64) > right))
            }
            (RuntimeValue::Int(left), BinaryOp::Greater, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left > right))
            }
            (RuntimeValue::Float(left), BinaryOp::GreaterEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left >= right))
            }
            (RuntimeValue::Float(left), BinaryOp::GreaterEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left >= right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::GreaterEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool((left as f64) >= right))
            }
            (RuntimeValue::Int(left), BinaryOp::GreaterEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left >= right))
            }
            (RuntimeValue::Float(left), BinaryOp::Equal, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left == right))
            }
            (RuntimeValue::Float(left), BinaryOp::Equal, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left == right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::Equal, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left as f64 == right))
            }
            (RuntimeValue::Int(left), BinaryOp::Equal, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left == right))
            }
            (RuntimeValue::Bool(left), BinaryOp::Equal, RuntimeValue::Bool(right)) => {
                Ok(RuntimeValue::Bool(left == right))
            }
            (RuntimeValue::Float(left), BinaryOp::NotEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left != right))
            }
            (RuntimeValue::Float(left), BinaryOp::NotEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left != right as f64))
            }
            (RuntimeValue::Int(left), BinaryOp::NotEqual, RuntimeValue::Float(right)) => {
                Ok(RuntimeValue::Bool(left as f64 != right))
            }
            (RuntimeValue::Int(left), BinaryOp::NotEqual, RuntimeValue::Int(right)) => {
                Ok(RuntimeValue::Bool(left != right))
            }
            (RuntimeValue::Bool(left), BinaryOp::NotEqual, RuntimeValue::Bool(right)) => {
                Ok(RuntimeValue::Bool(left != right))
            }
            (RuntimeValue::Color(color), BinaryOp::Multiply, RuntimeValue::Float(factor))
            | (RuntimeValue::Float(factor), BinaryOp::Multiply, RuntimeValue::Color(color)) => {
                Ok(RuntimeValue::Color(color.scale(factor)))
            }
            (RuntimeValue::Color(color), BinaryOp::Multiply, RuntimeValue::Int(factor))
            | (RuntimeValue::Int(factor), BinaryOp::Multiply, RuntimeValue::Color(color)) => {
                Ok(RuntimeValue::Color(color.scale(factor as f64)))
            }
            _ => Err(self.error("invalid binary expression")),
        }
    }

    fn eval_call(&mut self, function: BuiltinFn) -> Result<RuntimeValue, RuntimeError> {
        match function {
            BuiltinFn::Sin => {
                let value = self.pop()?;
                Ok(RuntimeValue::Float(self.expect_float(value)?.sin()))
            }
            BuiltinFn::Cos => {
                let value = self.pop()?;
                Ok(RuntimeValue::Float(self.expect_float(value)?.cos()))
            }
            BuiltinFn::Abs => {
                let value = self.pop()?;
                Ok(RuntimeValue::Float(self.expect_float(value)?.abs()))
            }
            BuiltinFn::Floor => {
                let value = self.pop()?;
                Ok(RuntimeValue::Float(self.expect_float(value)?.floor()))
            }
            BuiltinFn::Srand => {
                let value = self.pop()?;
                self.rng = seed_from_float(self.expect_float(value)?);
                Ok(RuntimeValue::Float(0.0))
            }
            BuiltinFn::Rand => Ok(RuntimeValue::Float(self.rand())),
            BuiltinFn::PixelIndex => match self.pop()? {
                RuntimeValue::Pixel(pixel) => Ok(RuntimeValue::Int(pixel.index as i64)),
                _ => Err(self.error("expected pixel value")),
            },
            BuiltinFn::PixelCount => match self.pop()? {
                RuntimeValue::Pixel(pixel) => Ok(RuntimeValue::Int(pixel.count as i64)),
                _ => Err(self.error("expected pixel value")),
            },
            BuiltinFn::MarkCount => {
                let marks = self.pop()?;
                Ok(RuntimeValue::Int(self.expect_marks(marks)?.len() as i64))
            }
            BuiltinFn::MarkAt => {
                let fallback = self.pop_float()?;
                let index = self.pop_int()?;
                let marks = self.pop_marks()?;
                let value = usize::try_from(index)
                    .ok()
                    .and_then(|index| marks.get(index))
                    .copied()
                    .unwrap_or(fallback);
                Ok(RuntimeValue::Float(value))
            }
            BuiltinFn::MarkPrev => self.eval_mark_search(mark_prev),
            BuiltinFn::MarkNext => self.eval_mark_search(mark_next),
            BuiltinFn::MarkNearest => self.eval_mark_search(mark_nearest),
            BuiltinFn::MarkPhase => self.eval_mark_search(mark_phase),
            BuiltinFn::MarkElapsed => self.eval_mark_search(mark_elapsed),
            BuiltinFn::Min => {
                let right = self.pop_float()?;
                let left = self.pop_float()?;
                Ok(RuntimeValue::Float(left.min(right)))
            }
            BuiltinFn::Max => {
                let right = self.pop_float()?;
                let left = self.pop_float()?;
                Ok(RuntimeValue::Float(left.max(right)))
            }
            BuiltinFn::Clamp => {
                let max = self.pop_float()?;
                let min = self.pop_float()?;
                let value = self.pop_float()?;
                Ok(RuntimeValue::Float(value.clamp(min, max)))
            }
            BuiltinFn::Smoothstep => {
                let value = self.pop_float()?;
                let edge1 = self.pop_float()?;
                let edge0 = self.pop_float()?;
                let x = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
                Ok(RuntimeValue::Float(x * x * (3.0 - 2.0 * x)))
            }
            BuiltinFn::Mix => {
                let amount = self.pop_float()?;
                let right = self.pop()?;
                let left = self.pop()?;
                match (left, right) {
                    (RuntimeValue::Color(left), RuntimeValue::Color(right)) => {
                        Ok(RuntimeValue::Color(left.mix(right, amount)))
                    }
                    (left, right) => {
                        let left = self.expect_float(left)?;
                        let right = self.expect_float(right)?;
                        Ok(RuntimeValue::Float(left + (right - left) * amount))
                    }
                }
            }
            BuiltinFn::Rgb => {
                let blue = self.pop_float()?;
                let green = self.pop_float()?;
                let red = self.pop_float()?;
                Ok(RuntimeValue::Color(Color::new(
                    red.round().clamp(0.0, 255.0) as u8,
                    green.round().clamp(0.0, 255.0) as u8,
                    blue.round().clamp(0.0, 255.0) as u8,
                )))
            }
            BuiltinFn::Hsv => {
                let value = self.pop_float()?;
                let saturation = self.pop_float()?;
                let hue = self.pop_float()?;
                Ok(RuntimeValue::Color(hsv_to_rgb(hue, saturation, value)))
            }
        }
    }

    fn eval_mark_search(
        &mut self,
        search: fn(&[f64], f64) -> Option<f64>,
    ) -> Result<RuntimeValue, RuntimeError> {
        let fallback = self.pop_float()?;
        let time = self.pop_float()?;
        let marks = self.pop_marks()?;
        Ok(RuntimeValue::Float(
            search(&marks, time).unwrap_or(fallback),
        ))
    }

    fn expect_float(&self, value: RuntimeValue) -> Result<f64, RuntimeError> {
        match value {
            RuntimeValue::Float(value) => Ok(value),
            RuntimeValue::Int(value) => Ok(value as f64),
            _ => Err(self.error("expected float value")),
        }
    }

    fn expect_int(&self, value: RuntimeValue) -> Result<i64, RuntimeError> {
        match value {
            RuntimeValue::Int(value) => Ok(value),
            _ => Err(self.error("expected int value")),
        }
    }

    fn expect_marks(&self, value: RuntimeValue) -> Result<Vec<f64>, RuntimeError> {
        match value {
            RuntimeValue::Marks(value) => Ok(value),
            _ => Err(self.error("expected marks value")),
        }
    }

    fn peek_bool(&self, message: &str) -> Result<bool, RuntimeError> {
        match self.stack.last() {
            Some(RuntimeValue::Bool(value)) => Ok(*value),
            _ => Err(self.error(message)),
        }
    }

    fn pop_bool(&mut self, message: &str) -> Result<bool, RuntimeError> {
        match self.pop()? {
            RuntimeValue::Bool(value) => Ok(value),
            _ => Err(self.error(message)),
        }
    }

    fn pop(&mut self) -> Result<RuntimeValue, RuntimeError> {
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

    fn pop_marks(&mut self) -> Result<Vec<f64>, RuntimeError> {
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
    marks.iter().rev().copied().find(|mark| *mark <= time)
}

fn mark_next(marks: &[f64], time: f64) -> Option<f64> {
    marks.iter().copied().find(|mark| *mark > time)
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
