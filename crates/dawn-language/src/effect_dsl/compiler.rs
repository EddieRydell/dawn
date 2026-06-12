use super::ast::{BinaryOp, EffectDecl, Expr, ExprKind, Module, ParamDecl, Stmt};
use super::bytecode::{Builtin, BytecodeFunction, Instruction, LocalId, ParamId, Target};
use super::types::{Identifier, Type, Value};
use super::CompiledEffect;
use indexmap::IndexMap;

pub(crate) fn compile_checked_effects(module: Module) -> Vec<CompiledEffect> {
    module.effects.into_iter().map(compile_effect).collect()
}

fn compile_effect(effect: EffectDecl) -> CompiledEffect {
    let params = effect
        .params
        .into_iter()
        .map(compile_param)
        .collect::<Vec<_>>();
    let sample = FunctionCompiler::new(&params).compile(effect.sample.body.statements);
    CompiledEffect {
        name: effect.name,
        params,
        sample,
    }
}

fn compile_param(param: ParamDecl) -> ParamDecl {
    param
}

struct FunctionCompiler {
    instructions: Vec<Instruction>,
    constants: Vec<Value>,
    scopes: Vec<IndexMap<Identifier, Binding>>,
    local_count: usize,
    stack_depth: isize,
    max_stack: usize,
}

#[derive(Clone, Copy)]
enum Binding {
    Param(ParamId),
    Local(LocalId),
}

impl FunctionCompiler {
    fn new(params: &[ParamDecl]) -> Self {
        let mut param_scope = IndexMap::new();
        for (index, param) in params.iter().enumerate() {
            param_scope.insert(param.name.clone(), Binding::Param(index));
        }
        Self {
            instructions: Vec::new(),
            constants: Vec::new(),
            scopes: vec![param_scope],
            local_count: 0,
            stack_depth: 0,
            max_stack: 0,
        }
    }

    fn compile(mut self, statements: Vec<Stmt>) -> BytecodeFunction {
        self.compile_block(statements);
        BytecodeFunction {
            instructions: self.instructions,
            constants: self.constants,
            local_count: self.local_count,
            max_stack: self.max_stack,
        }
    }

    fn compile_block(&mut self, statements: Vec<Stmt>) {
        self.scopes.push(IndexMap::new());
        for statement in statements {
            self.compile_statement(statement);
        }
        let _ = self.scopes.pop();
    }

    fn compile_statement(&mut self, statement: Stmt) {
        match statement {
            Stmt::Local {
                ty,
                name,
                initializer,
            } => {
                let slot = self.allocate_local(name);
                if let Some(initializer) = initializer {
                    self.compile_expr(initializer);
                    if ty == Type::Float {
                        self.emit(Instruction::CoerceFloat, 0);
                    }
                } else {
                    self.emit(Instruction::LoadDefault(ty), 1);
                }
                self.emit(Instruction::StoreLocal(slot), -1);
            }
            Stmt::Assign { name, value } => {
                self.compile_expr(value);
                match self.lookup(&name) {
                    Some(Binding::Param(slot)) => self.emit(Instruction::StoreParam(slot), -1),
                    Some(Binding::Local(slot)) => self.emit(Instruction::StoreLocal(slot), -1),
                    None => {}
                }
            }
            Stmt::Expr(expr) => {
                self.compile_expr(expr);
                self.emit(Instruction::Pop, -1);
            }
            Stmt::If {
                condition,
                then_block,
                else_block,
            } => {
                self.compile_expr(condition);
                let false_jump = self.emit_jump(Instruction::JumpIfFalse(usize::MAX), -1);
                self.compile_block(then_block.statements);
                if let Some(else_block) = else_block {
                    let end_jump = self.emit_jump(Instruction::Jump(usize::MAX), 0);
                    self.patch_jump(false_jump, self.current_target());
                    self.compile_block(else_block.statements);
                    self.patch_jump(end_jump, self.current_target());
                } else {
                    self.patch_jump(false_jump, self.current_target());
                }
            }
            Stmt::For {
                initializer,
                condition,
                update,
                body,
            } => {
                self.scopes.push(IndexMap::new());
                self.compile_statement(*initializer);
                let loop_start = self.current_target();
                self.compile_expr(condition);
                let end_jump = self.emit_jump(Instruction::JumpIfFalse(usize::MAX), -1);
                self.emit(Instruction::CheckLoopLimit, 0);
                self.compile_block(body.statements);
                self.compile_statement(*update);
                self.emit(Instruction::Jump(loop_start), 0);
                self.patch_jump(end_jump, self.current_target());
                let _ = self.scopes.pop();
            }
            Stmt::Return(expr) => {
                self.compile_expr(expr);
                self.emit(Instruction::Return, -1);
            }
        }
    }

    fn compile_expr(&mut self, expr: Expr) {
        match expr.kind {
            ExprKind::Literal(value) => {
                let constant = self.add_constant(value);
                self.emit(Instruction::LoadConst(constant), 1);
            }
            ExprKind::Variable(name) => match self.lookup(&name) {
                Some(Binding::Param(slot)) => self.emit(Instruction::LoadParam(slot), 1),
                Some(Binding::Local(slot)) => self.emit(Instruction::LoadLocal(slot), 1),
                None => {
                    let value = match name.as_str() {
                        "PI" => Value::Float(std::f64::consts::PI),
                        "TAU" => Value::Float(std::f64::consts::TAU),
                        _ => Value::Enum(name),
                    };
                    let constant = self.add_constant(value);
                    self.emit(Instruction::LoadConst(constant), 1);
                }
            },
            ExprKind::Array(items) => {
                let count = items.len();
                for item in items {
                    self.compile_expr(item);
                }
                self.emit(Instruction::MakeArray(count), 1 - count as isize);
            }
            ExprKind::Index { target, index } => {
                self.compile_expr(*target);
                self.compile_expr(*index);
                self.emit(Instruction::Index, -1);
            }
            ExprKind::Call { callee, args } => {
                let ExprKind::Variable(name) = callee.kind else {
                    return;
                };
                let Some(builtin) = Builtin::from_name(&name) else {
                    return;
                };
                let arity = args.len();
                for arg in args {
                    self.compile_expr(arg);
                }
                self.emit(Instruction::CallBuiltin(builtin, arity), 1 - arity as isize);
            }
            ExprKind::Unary { op, expr } => {
                self.compile_expr(*expr);
                self.emit(Instruction::Unary(op), 0);
            }
            ExprKind::Binary { op, left, right } => match op {
                BinaryOp::And => {
                    self.compile_expr(*left);
                    let end_jump = self.emit_jump(Instruction::JumpIfFalseOrPop(usize::MAX), 0);
                    self.compile_expr(*right);
                    self.patch_jump(end_jump, self.current_target());
                }
                BinaryOp::Or => {
                    self.compile_expr(*left);
                    let end_jump = self.emit_jump(Instruction::JumpIfTrueOrPop(usize::MAX), 0);
                    self.compile_expr(*right);
                    self.patch_jump(end_jump, self.current_target());
                }
                _ => {
                    self.compile_expr(*left);
                    self.compile_expr(*right);
                    self.emit(Instruction::Binary(op), -1);
                }
            },
        }
    }

    fn allocate_local(&mut self, name: Identifier) -> LocalId {
        let slot = self.local_count;
        self.local_count += 1;
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, Binding::Local(slot));
        }
        slot
    }

    fn lookup(&self, name: &Identifier) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    fn add_constant(&mut self, value: Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }

    fn current_target(&self) -> Target {
        self.instructions.len()
    }

    fn emit(&mut self, instruction: Instruction, stack_delta: isize) {
        self.instructions.push(instruction);
        self.adjust_stack(stack_delta);
    }

    fn emit_jump(&mut self, instruction: Instruction, stack_delta: isize) -> usize {
        let offset = self.instructions.len();
        self.instructions.push(instruction);
        self.adjust_stack(stack_delta);
        offset
    }

    fn patch_jump(&mut self, offset: usize, target: Target) {
        match &mut self.instructions[offset] {
            Instruction::Jump(existing)
            | Instruction::JumpIfFalse(existing)
            | Instruction::JumpIfFalseOrPop(existing)
            | Instruction::JumpIfTrueOrPop(existing) => *existing = target,
            _ => {}
        }
    }

    fn adjust_stack(&mut self, delta: isize) {
        self.stack_depth += delta;
        if self.stack_depth > self.max_stack as isize {
            self.max_stack = self.stack_depth as usize;
        }
    }
}
