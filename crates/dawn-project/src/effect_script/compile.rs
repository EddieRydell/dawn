use std::collections::HashMap;

use super::ast::{BinaryOp, EffectAst, EffectEntrypoint, Expr, Stmt, UnaryOp};
use super::builtins::{BuiltinConstant, BuiltinContext, BuiltinFunction};
use super::bytecode::{
    BinaryInstruction, BoolSlot, BytecodeProgram, ColorSlot, ContextSlot, FixtureSlot, FloatSlot,
    Instruction, IntSlot, MarkSearchInstruction, PixelSlot, RefSlot, RegisterCounts,
    UnaryFloatInstruction, ValueSlot,
};
use super::{binary_result_type, is_float_compatible, RuntimeValue, ScriptType};

pub(super) fn compile_effect(effect: &EffectAst) -> BytecodeProgram {
    Compiler::new(effect).finish()
}

#[derive(Debug, Clone, Copy)]
enum Binding {
    Context(BuiltinContext),
    Constant(usize),
    Param {
        index: usize,
        value_type: ScriptType,
    },
    Local(ValueSlot),
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
    fn finish(mut self) -> BytecodeProgram {
        let EffectEntrypoint::Sample(statements) = &self.effect.entrypoint else {
            unreachable!("generator effects are not sample-compiled");
        };
        for statement in statements {
            self.compile_statement(statement);
        }
        BytecodeProgram {
            instructions: self.instructions,
            constants: self.constants,
            registers: self.registers,
        }
    }
    fn define_builtin_bindings(&mut self) {
        for context in BuiltinContext::ALL {
            self.define(context.name(), Binding::Context(context));
        }
        for constant in BuiltinConstant::ALL {
            let index = self.add_constant(RuntimeValue::Float(constant.value()));
            self.define(constant.name(), Binding::Constant(index));
        }
        for (index, param) in self.effect.params.iter().enumerate() {
            self.define(
                &param.name,
                Binding::Param {
                    index,
                    value_type: param.value_type,
                },
            );
            if param.value_type == ScriptType::Enum {
                for option in &param.options {
                    let index = self.add_constant(RuntimeValue::Enum(option.clone()));
                    self.define(option, Binding::Constant(index));
                }
            }
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
            Stmt::Emit(_) => unreachable!("sample effects cannot emit"),
        }
    }
    fn compile_expr(&mut self, expr: &Expr) -> ValueSlot {
        match expr {
            Expr::Float(value) => self.load_constant(RuntimeValue::Float(*value)),
            Expr::Int(value) => self.load_constant(RuntimeValue::Int(*value)),
            Expr::Bool(value) => self.load_constant(RuntimeValue::Bool(*value)),
            Expr::Color(value) => self.load_constant(RuntimeValue::Color(*value)),
            Expr::Ident(name) => match self.expect_binding(name) {
                Binding::Context(context) => {
                    let dest = self.allocate_slot(context.value_type());
                    self.emit(Instruction::LoadContext(dest, context_slot(context)));
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
            Expr::Member { .. } | Expr::Qualified { .. } => {
                unreachable!("sample type checker rejects generator expressions")
            }
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
                let result_type = match binary_result_type(left_type, op, right_type) {
                    Some(result_type) => result_type,
                    None => unreachable!("type checker validates binary expression"),
                };
                let dest = self.allocate_slot(result_type);
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
        let needs_float = matches!(
            (instruction, is_left),
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
            ) | (BinaryInstruction::ColorMultiplyFloat, false)
                | (BinaryInstruction::FloatMultiplyColor, true)
        );
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
            (ScriptType::Enum, BinaryOp::Equal, ScriptType::Enum) => BinaryInstruction::EnumEqual,
            (ScriptType::Enum, BinaryOp::NotEqual, ScriptType::Enum) => {
                BinaryInstruction::EnumNotEqual
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
                Binding::Context(context) => context.value_type(),
                Binding::Constant(index) => self.constants[index].value_type(),
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
                match binary_result_type(left, *op, right) {
                    Some(result_type) => result_type,
                    None => unreachable!("type checker validates binary expression"),
                }
            }
            Expr::Call { name, args } => self.call_type(name, args),
            Expr::Member { .. } | Expr::Qualified { .. } => {
                unreachable!("sample type checker rejects generator expressions")
            }
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
        let arg_types = args
            .iter()
            .map(|arg| self.expr_type(arg))
            .collect::<Vec<_>>();
        match BuiltinFunction::from_name(name).and_then(|function| function.return_type(&arg_types))
        {
            Some(return_type) => return_type,
            None => unreachable!("type checker validates builtins"),
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
        let function = match BuiltinFunction::from_name(name) {
            Some(function) => function,
            None => unreachable!("type checker validates builtins"),
        };
        match function {
            BuiltinFunction::Sin => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Sin(dest, value));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Cos => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Cos(dest, value));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Abs => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Abs(dest, value));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Floor => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Floor(dest, value));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Srand => {
                let value = self.compile_float_arg(&args[0]);
                let dest = self.allocate_float();
                self.emit(Instruction::Srand(dest, value));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Rand => {
                let dest = self.allocate_float();
                self.emit(Instruction::Rand(dest));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::PixelIndex => {
                let pixel = self.compile_expr(&args[0]).pixel();
                let dest = self.allocate_int();
                self.emit(Instruction::PixelIndex(dest, pixel));
                ValueSlot::Int(dest)
            }
            BuiltinFunction::PixelCount => {
                let pixel = self.compile_expr(&args[0]).pixel();
                let dest = self.allocate_int();
                self.emit(Instruction::PixelCount(dest, pixel));
                ValueSlot::Int(dest)
            }
            BuiltinFunction::MarkCount => {
                let marks = self.compile_expr(&args[0]).reference();
                let dest = self.allocate_int();
                self.emit(Instruction::MarkCount(dest, marks));
                ValueSlot::Int(dest)
            }
            BuiltinFunction::MarkAt => {
                let marks = self.compile_expr(&args[0]).reference();
                let index = self.compile_expr(&args[1]).int();
                let fallback = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                self.emit(Instruction::MarkAt(dest, marks, index, fallback));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::MarkPrev
            | BuiltinFunction::MarkNext
            | BuiltinFunction::MarkNearest
            | BuiltinFunction::MarkPhase
            | BuiltinFunction::MarkElapsed => {
                let marks = self.compile_expr(&args[0]).reference();
                let time = self.compile_float_arg(&args[1]);
                let fallback = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                let search = match function {
                    BuiltinFunction::MarkPrev => MarkSearchInstruction::Prev,
                    BuiltinFunction::MarkNext => MarkSearchInstruction::Next,
                    BuiltinFunction::MarkNearest => MarkSearchInstruction::Nearest,
                    BuiltinFunction::MarkPhase => MarkSearchInstruction::Phase,
                    BuiltinFunction::MarkElapsed => MarkSearchInstruction::Elapsed,
                    _ => unreachable!("matched above"),
                };
                self.emit(Instruction::MarkSearch(dest, search, marks, time, fallback));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::CurveCrossing => {
                if let Expr::Ident(name) = &args[0] {
                    if let Some(Binding::Param {
                        index,
                        value_type: ScriptType::CurveFloat,
                    }) = self.binding(name)
                    {
                        let value = self.compile_float_arg(&args[1]);
                        let fallback = self.compile_float_arg(&args[2]);
                        let dest = self.allocate_float();
                        self.emit(Instruction::CurveParamCrossing(
                            dest, index, value, fallback,
                        ));
                        return ValueSlot::Float(dest);
                    }
                }
                let curve = self.compile_expr(&args[0]).reference();
                let value = self.compile_float_arg(&args[1]);
                let fallback = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                self.emit(Instruction::CurveCrossing(dest, curve, value, fallback));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Min => {
                let left = self.compile_float_arg(&args[0]);
                let right = self.compile_float_arg(&args[1]);
                let dest = self.allocate_float();
                self.emit(Instruction::Min(dest, left, right));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Max => {
                let left = self.compile_float_arg(&args[0]);
                let right = self.compile_float_arg(&args[1]);
                let dest = self.allocate_float();
                self.emit(Instruction::Max(dest, left, right));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Clamp => {
                let value = self.compile_float_arg(&args[0]);
                let min = self.compile_float_arg(&args[1]);
                let max = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                self.emit(Instruction::Clamp(dest, value, min, max));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Smoothstep => {
                let edge0 = self.compile_float_arg(&args[0]);
                let edge1 = self.compile_float_arg(&args[1]);
                let value = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                self.emit(Instruction::Smoothstep(dest, edge0, edge1, value));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Mix if self.call_type(name, args) == ScriptType::Color => {
                let left = self.compile_expr(&args[0]).color();
                let right = self.compile_expr(&args[1]).color();
                let amount = self.compile_float_arg(&args[2]);
                let dest = self.allocate_color();
                self.emit(Instruction::MixColor(dest, left, right, amount));
                ValueSlot::Color(dest)
            }
            BuiltinFunction::Mix => {
                let left = self.compile_float_arg(&args[0]);
                let right = self.compile_float_arg(&args[1]);
                let amount = self.compile_float_arg(&args[2]);
                let dest = self.allocate_float();
                self.emit(Instruction::MixFloat(dest, left, right, amount));
                ValueSlot::Float(dest)
            }
            BuiltinFunction::Rgb => {
                let red = self.compile_float_arg(&args[0]);
                let green = self.compile_float_arg(&args[1]);
                let blue = self.compile_float_arg(&args[2]);
                let dest = self.allocate_color();
                self.emit(Instruction::Rgb(dest, red, green, blue));
                ValueSlot::Color(dest)
            }
            BuiltinFunction::Hsv => {
                let hue = self.compile_float_arg(&args[0]);
                let saturation = self.compile_float_arg(&args[1]);
                let value = self.compile_float_arg(&args[2]);
                let dest = self.allocate_color();
                self.emit(Instruction::Hsv(dest, hue, saturation, value));
                ValueSlot::Color(dest)
            }
            BuiltinFunction::Fixtures
            | BuiltinFunction::Pixels
            | BuiltinFunction::Sections
            | BuiltinFunction::Count
            | BuiltinFunction::Pick => {
                unreachable!("sample compiler does not emit generator builtins")
            }
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
            ScriptType::Timeline
            | ScriptType::Target
            | ScriptType::TargetItems
            | ScriptType::TargetItem
            | ScriptType::Void => {
                unreachable!("generator values are not stored in sample bytecode")
            }
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
        match self.binding(name) {
            Some(binding) => binding,
            None => unreachable!("type checker validates binding `{name}`"),
        }
    }
    fn define(&mut self, name: &str, binding: Binding) {
        let Some(scope) = self.scopes.last_mut() else {
            unreachable!("compiler always has a scope");
        };
        scope.insert(name.to_string(), binding);
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

fn context_slot(context: BuiltinContext) -> ContextSlot {
    match context {
        BuiltinContext::Progress => ContextSlot::Progress,
        BuiltinContext::Seconds => ContextSlot::Seconds,
        BuiltinContext::Fixture => ContextSlot::Fixture,
        BuiltinContext::Pixel => ContextSlot::Pixel,
    }
}
