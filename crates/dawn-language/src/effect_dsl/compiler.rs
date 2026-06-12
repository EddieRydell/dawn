use super::ast::{BinaryOp, ParamDecl};
use super::bytecode::{
    register_layout_id, ContextRead, FloatBinary, FloatUnary, GeneratorContextId, Instruction,
    LocalId, MarkOp, ParamId, RegisterFunction, RegisterId, Target, TargetItemsOp,
};
use super::checked::{
    CheckedBlock, CheckedEffectDecl, CheckedExpr, CheckedExprKind, CheckedModule, CheckedStmt,
};
use super::types::{Identifier, Type, Value};
use super::{CompiledEffect, EffectKind};
use indexmap::IndexMap;

pub(crate) fn compile_checked_effects(module: CheckedModule) -> Vec<CompiledEffect> {
    module.effects.into_iter().map(compile_effect).collect()
}

fn compile_effect(effect: CheckedEffectDecl) -> CompiledEffect {
    let kind = if effect.entrypoint.name.as_str() == "generate" {
        EffectKind::Generator
    } else {
        EffectKind::Sample
    };
    let function = FunctionCompiler::new(&effect.params, kind).compile(effect.body);
    CompiledEffect {
        name: effect.name,
        params: effect.params,
        kind,
        function,
    }
}

struct FunctionCompiler {
    instructions: Vec<Instruction>,
    constants: Vec<Value>,
    scopes: Vec<IndexMap<Identifier, Binding>>,
    param_types: Vec<Type>,
    register_types: Vec<Type>,
}

#[derive(Clone, Copy)]
enum Binding {
    Param(ParamId),
    Local(LocalId),
    GeneratorContext(GeneratorContextId),
}

impl FunctionCompiler {
    fn new(params: &[ParamDecl], kind: EffectKind) -> Self {
        let mut param_scope = IndexMap::new();
        for (index, param) in params.iter().enumerate() {
            param_scope.insert(param.name.clone(), Binding::Param(index));
        }
        if kind == EffectKind::Generator {
            param_scope.insert(
                static_identifier("timeline"),
                Binding::GeneratorContext(GeneratorContextId::Timeline),
            );
            param_scope.insert(
                static_identifier("target"),
                Binding::GeneratorContext(GeneratorContextId::Target),
            );
            param_scope.insert(
                static_identifier("duration"),
                Binding::GeneratorContext(GeneratorContextId::Duration),
            );
        }
        Self {
            instructions: Vec::new(),
            constants: Vec::new(),
            scopes: vec![param_scope],
            param_types: params.iter().map(|param| param.ty.clone()).collect(),
            register_types: Vec::new(),
        }
    }

    fn compile(mut self, block: CheckedBlock) -> RegisterFunction {
        self.compile_block(block);
        let void = self.allocate_register(Type::Void);
        let constant = self.add_constant(Value::Void);
        self.emit(Instruction::LoadConst {
            dst: void,
            constant,
        });
        self.emit(Instruction::Return(void));
        RegisterFunction {
            instructions: self.instructions,
            constants: self.constants,
            register_count: self.register_types.len(),
            register_layout_id: register_layout_id(&self.register_types),
            register_types: self.register_types,
        }
    }

    fn compile_block(&mut self, block: CheckedBlock) {
        self.scopes.push(IndexMap::new());
        for statement in block.statements {
            self.compile_statement(statement);
        }
        let _ = self.scopes.pop();
    }

    fn compile_statement(&mut self, statement: CheckedStmt) {
        match statement {
            CheckedStmt::Local {
                ty,
                name,
                initializer,
            } => {
                let slot = self.allocate_local(name, ty.clone());
                if let Some(initializer) = initializer {
                    let value = self.compile_expr(initializer);
                    let value = self.coerce_register(value, &ty);
                    self.emit(Instruction::Move {
                        dst: slot,
                        src: value,
                    });
                } else {
                    self.emit(Instruction::LoadDefault { dst: slot, ty });
                }
            }
            CheckedStmt::Assign { name, value } => {
                let value = self.compile_expr(value);
                match self.lookup(&name) {
                    Some(Binding::Param(slot)) => {
                        let value = self.coerce_register(value, &self.param_types[slot].clone());
                        self.emit(Instruction::StoreParam {
                            param: slot,
                            src: value,
                        });
                    }
                    Some(Binding::Local(slot)) => {
                        let value = self.coerce_register(value, &self.register_types[slot].clone());
                        self.emit(Instruction::Move {
                            dst: slot,
                            src: value,
                        });
                    }
                    Some(Binding::GeneratorContext(_)) | None => {}
                }
            }
            CheckedStmt::Expr(expr) => {
                let _ = self.compile_expr(expr);
            }
            CheckedStmt::If {
                condition,
                then_block,
                else_block,
            } => {
                let condition = self.compile_expr(condition);
                let false_jump = self.emit_jump(Instruction::JumpIfFalse {
                    condition,
                    target: usize::MAX,
                });
                self.compile_block(then_block);
                if let Some(else_block) = else_block {
                    let end_jump = self.emit_jump(Instruction::Jump(usize::MAX));
                    self.patch_jump(false_jump, self.current_target());
                    self.compile_block(else_block);
                    self.patch_jump(end_jump, self.current_target());
                } else {
                    self.patch_jump(false_jump, self.current_target());
                }
            }
            CheckedStmt::For {
                initializer,
                condition,
                update,
                body,
            } => {
                self.scopes.push(IndexMap::new());
                self.compile_statement(*initializer);
                let loop_start = self.current_target();
                let condition = self.compile_expr(condition);
                let end_jump = self.emit_jump(Instruction::JumpIfFalse {
                    condition,
                    target: usize::MAX,
                });
                self.emit(Instruction::CheckLoopLimit);
                self.compile_block(body);
                self.compile_statement(*update);
                self.emit(Instruction::Jump(loop_start));
                self.patch_jump(end_jump, self.current_target());
                let _ = self.scopes.pop();
            }
            CheckedStmt::Emit { effect, fields } => {
                let fields = fields
                    .into_iter()
                    .map(|(name, value)| (name, self.compile_expr(value)))
                    .collect();
                self.emit(Instruction::Emit { effect, fields });
            }
            CheckedStmt::Return(expr) => {
                let value = self.compile_expr(expr);
                self.emit(Instruction::Return(value));
            }
        }
    }

    fn compile_expr(&mut self, expr: CheckedExpr) -> RegisterId {
        let result_ty = expr.ty.clone();
        match expr.kind {
            CheckedExprKind::Literal(value) => {
                let dst = self.allocate_register(result_ty);
                let constant = self.add_constant(value);
                self.emit(Instruction::LoadConst { dst, constant });
                dst
            }
            CheckedExprKind::Variable(name) => match self.lookup(&name) {
                Some(Binding::Param(slot)) => {
                    let dst = self.allocate_register(result_ty);
                    self.emit(Instruction::LoadParam { dst, param: slot });
                    dst
                }
                Some(Binding::Local(slot)) => slot,
                Some(Binding::GeneratorContext(slot)) => {
                    let dst = self.allocate_register(result_ty);
                    self.emit(Instruction::LoadGeneratorContext { dst, slot });
                    dst
                }
                None => {
                    let value = match name.as_str() {
                        "PI" => Value::Float(std::f64::consts::PI),
                        "TAU" => Value::Float(std::f64::consts::TAU),
                        _ => Value::Enum(name),
                    };
                    let dst = self.allocate_register(result_ty);
                    let constant = self.add_constant(value);
                    self.emit(Instruction::LoadConst { dst, constant });
                    dst
                }
            },
            CheckedExprKind::Array(items) => {
                let item_registers = items
                    .into_iter()
                    .map(|item| self.compile_expr(item))
                    .collect();
                let dst = self.allocate_register(result_ty);
                self.emit(Instruction::MakeArray {
                    dst,
                    items: item_registers,
                });
                dst
            }
            CheckedExprKind::Index { target, index } => {
                if let Some(param) = self.curve_param_binding(&target) {
                    let position = self.compile_expr(*index);
                    let dst = self.allocate_register(result_ty);
                    self.emit(Instruction::CurveParamSample {
                        dst,
                        param,
                        position,
                    });
                    dst
                } else {
                    let target = self.compile_expr(*target);
                    let index = self.compile_expr(*index);
                    let dst = self.allocate_register(result_ty);
                    self.emit(Instruction::Index { dst, target, index });
                    dst
                }
            }
            CheckedExprKind::Member { target, member } => {
                let target = self.compile_expr(*target);
                let dst = self.allocate_register(result_ty);
                self.emit(Instruction::Member {
                    dst,
                    target,
                    member,
                });
                dst
            }
            CheckedExprKind::Call { callee, args } => {
                let CheckedExprKind::Variable(name) = callee.kind else {
                    let dst = self.allocate_register(result_ty);
                    self.emit(Instruction::LoadDefault {
                        dst,
                        ty: Type::Void,
                    });
                    return dst;
                };
                self.compile_builtin_call(name, args, result_ty)
            }
            CheckedExprKind::Unary { op, expr } => {
                let src = self.compile_expr(*expr);
                let dst = self.allocate_register(result_ty);
                self.emit(Instruction::Unary { dst, op, src });
                dst
            }
            CheckedExprKind::Binary { op, left, right } => match op {
                BinaryOp::And => self.compile_short_circuit(false, *left, *right, result_ty),
                BinaryOp::Or => self.compile_short_circuit(true, *left, *right, result_ty),
                _ => {
                    let left = self.compile_expr(*left);
                    let right = self.compile_expr(*right);
                    let dst = self.allocate_register(result_ty);
                    self.emit(Instruction::Binary {
                        dst,
                        op,
                        left,
                        right,
                    });
                    dst
                }
            },
        }
    }

    fn compile_short_circuit(
        &mut self,
        jump_when_true: bool,
        left: CheckedExpr,
        right: CheckedExpr,
        result_ty: Type,
    ) -> RegisterId {
        let dst = self.allocate_register(result_ty);
        let left = self.compile_expr(left);
        self.emit(Instruction::Move { dst, src: left });
        let jump = if jump_when_true {
            self.emit_jump(Instruction::JumpIfTrue {
                condition: dst,
                target: usize::MAX,
            })
        } else {
            self.emit_jump(Instruction::JumpIfFalse {
                condition: dst,
                target: usize::MAX,
            })
        };
        let right = self.compile_expr(right);
        self.emit(Instruction::Move { dst, src: right });
        self.patch_jump(jump, self.current_target());
        dst
    }

    fn compile_builtin_call(
        &mut self,
        name: Identifier,
        args: Vec<CheckedExpr>,
        result_ty: Type,
    ) -> RegisterId {
        let dst = self.allocate_register(result_ty);
        match name.as_str() {
            "progress" => self.emit(Instruction::ContextRead {
                dst,
                read: ContextRead::Progress,
            }),
            "seconds" => self.emit(Instruction::ContextRead {
                dst,
                read: ContextRead::Seconds,
            }),
            "duration" => self.emit(Instruction::ContextRead {
                dst,
                read: ContextRead::Duration,
            }),
            "pixel_index" => self.emit(Instruction::ContextRead {
                dst,
                read: ContextRead::PixelIndex,
            }),
            "pixel_count" => self.emit(Instruction::ContextRead {
                dst,
                read: ContextRead::PixelCount,
            }),
            "pixel_fraction" => self.emit(Instruction::ContextRead {
                dst,
                read: ContextRead::PixelFraction,
            }),
            "section_position" => {
                let args = self.compile_args(args);
                self.emit(Instruction::SectionPosition {
                    dst,
                    width: args[0],
                });
            }
            "sin" | "cos" | "abs" | "floor" => {
                let args = self.compile_args(args);
                self.emit(Instruction::FloatUnary {
                    dst,
                    op: match name.as_str() {
                        "sin" => FloatUnary::Sin,
                        "cos" => FloatUnary::Cos,
                        "abs" => FloatUnary::Abs,
                        "floor" => FloatUnary::Floor,
                        _ => unreachable!("matched float unary builtin"),
                    },
                    value: args[0],
                });
            }
            "min" | "max" => {
                let args = self.compile_args(args);
                self.emit(Instruction::FloatBinary {
                    dst,
                    op: if name.as_str() == "min" {
                        FloatBinary::Min
                    } else {
                        FloatBinary::Max
                    },
                    left: args[0],
                    right: args[1],
                });
            }
            "clamp" => {
                let args = self.compile_args(args);
                self.emit(Instruction::Clamp {
                    dst,
                    value: args[0],
                    min: args[1],
                    max: args[2],
                });
            }
            "smoothstep" => {
                let args = self.compile_args(args);
                self.emit(Instruction::Smoothstep {
                    dst,
                    edge0: args[0],
                    edge1: args[1],
                    value: args[2],
                });
            }
            "mix" => {
                let args = self.compile_args(args);
                self.emit(Instruction::Mix {
                    dst,
                    left: args[0],
                    right: args[1],
                    amount: args[2],
                });
            }
            "rgb" => {
                let args = self.compile_args(args);
                self.emit(Instruction::Rgb {
                    dst,
                    red: args[0],
                    green: args[1],
                    blue: args[2],
                });
            }
            "hsv" => {
                let args = self.compile_args(args);
                self.emit(Instruction::Hsv {
                    dst,
                    hue: args[0],
                    saturation: args[1],
                    value: args[2],
                });
            }
            "srand" | "rand" => {
                let args = self.compile_args(args);
                self.emit(Instruction::Rand { dst, args });
            }
            "curve_float_clamped" if args.len() == 4 => {
                if let Some(param) = self.curve_param_binding(&args[0]) {
                    let registers = self.compile_args(args.into_iter().skip(1).collect());
                    self.emit(Instruction::CurveParamFloatClamped {
                        dst,
                        param,
                        position: registers[0],
                        min: registers[1],
                        max: registers[2],
                    });
                } else {
                    let args = self.compile_args(args);
                    self.emit(Instruction::CurveFloatClamped {
                        dst,
                        curve: args[0],
                        position: args[1],
                        min: args[2],
                        max: args[3],
                    });
                }
            }
            "curve_color_scaled" if args.len() == 3 => {
                if let Some(param) = self.curve_param_binding(&args[0]) {
                    let registers = self.compile_args(args.into_iter().skip(1).collect());
                    self.emit(Instruction::CurveParamColorScaled {
                        dst,
                        param,
                        position: registers[0],
                        scale: registers[1],
                    });
                } else {
                    let args = self.compile_args(args);
                    self.emit(Instruction::CurveColorScaled {
                        dst,
                        curve: args[0],
                        position: args[1],
                        scale: args[2],
                    });
                }
            }
            "curve_crossing" if args.len() == 2 || args.len() == 3 => {
                if let Some(param) = self.curve_param_binding(&args[0]) {
                    let registers = self.compile_args(args.into_iter().skip(1).collect());
                    self.emit(Instruction::CurveParamCrossing {
                        dst,
                        param,
                        value: registers[0],
                        fallback: registers.get(1).copied(),
                    });
                } else {
                    let args = self.compile_args(args);
                    self.emit(Instruction::CurveCrossing {
                        dst,
                        curve: args[0],
                        value: args[1],
                        fallback: args.get(2).copied(),
                    });
                }
            }
            "len" => {
                let args = self.compile_args(args);
                self.emit(Instruction::Len {
                    dst,
                    value: args[0],
                });
            }
            "mark_count" | "mark_at" | "mark_prev" | "mark_prev_index" | "mark_next_index"
            | "mark_elapsed" | "mark_phase" => {
                let op = match name.as_str() {
                    "mark_count" => MarkOp::Count,
                    "mark_at" => MarkOp::At,
                    "mark_prev" => MarkOp::Prev,
                    "mark_prev_index" => MarkOp::PrevIndex,
                    "mark_next_index" => MarkOp::NextIndex,
                    "mark_elapsed" => MarkOp::Elapsed,
                    "mark_phase" => MarkOp::Phase,
                    _ => unreachable!("matched mark builtin"),
                };
                let args = self.compile_args(args);
                self.emit(Instruction::Mark { dst, op, args });
            }
            "fixtures" | "pixels" | "sections" | "count" | "pick" => {
                let op = match name.as_str() {
                    "fixtures" => TargetItemsOp::Fixtures,
                    "pixels" => TargetItemsOp::Pixels,
                    "sections" => TargetItemsOp::Sections,
                    "count" => TargetItemsOp::Count,
                    "pick" => TargetItemsOp::Pick,
                    _ => unreachable!("matched target builtin"),
                };
                let args = self.compile_args(args);
                self.emit(Instruction::TargetItems { dst, op, args });
            }
            _ => self.emit(Instruction::LoadDefault {
                dst,
                ty: Type::Void,
            }),
        }
        dst
    }

    fn compile_args(&mut self, args: Vec<CheckedExpr>) -> Vec<RegisterId> {
        args.into_iter().map(|arg| self.compile_expr(arg)).collect()
    }

    fn curve_param_binding(&self, expr: &CheckedExpr) -> Option<ParamId> {
        let CheckedExprKind::Variable(name) = &expr.kind else {
            return None;
        };
        let Some(Binding::Param(param)) = self.lookup(name) else {
            return None;
        };
        match self.param_types.get(param) {
            Some(Type::Curve(_)) => Some(param),
            _ => None,
        }
    }

    fn coerce_register(&mut self, register: RegisterId, target: &Type) -> RegisterId {
        if target == &Type::Float && self.register_types.get(register) == Some(&Type::Int) {
            let dst = self.allocate_register(Type::Float);
            self.emit(Instruction::CoerceFloat { dst, src: register });
            dst
        } else {
            register
        }
    }

    fn allocate_local(&mut self, name: Identifier, ty: Type) -> LocalId {
        let slot = self.allocate_register(ty);
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, Binding::Local(slot));
        }
        slot
    }

    fn allocate_register(&mut self, ty: Type) -> RegisterId {
        let slot = self.register_types.len();
        self.register_types.push(ty);
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

    fn emit(&mut self, instruction: Instruction) {
        self.instructions.push(instruction);
    }

    fn emit_jump(&mut self, instruction: Instruction) -> usize {
        let offset = self.instructions.len();
        self.instructions.push(instruction);
        offset
    }

    fn patch_jump(&mut self, offset: usize, target: Target) {
        match &mut self.instructions[offset] {
            Instruction::Jump(existing) => *existing = target,
            Instruction::JumpIfFalse {
                target: existing, ..
            }
            | Instruction::JumpIfTrue {
                target: existing, ..
            } => *existing = target,
            _ => {}
        }
    }
}

fn static_identifier(value: &str) -> Identifier {
    match Identifier::new(value.to_string()) {
        Ok(identifier) => identifier,
        Err(_) => unreachable!("static identifier is valid"),
    }
}
