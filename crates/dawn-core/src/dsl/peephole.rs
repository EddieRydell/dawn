use super::compiler::Op;
use super::ops;

/// Apply peephole optimizations to compiled bytecode.
/// Runs until no more changes are made (fixpoint).
pub fn peephole(mut ops: Vec<Op>, constants: &mut Vec<f64>) -> Vec<Op> {
    loop {
        let (new_ops, changed) = peephole_pass(&ops, constants);
        ops = new_ops;
        if !changed {
            break;
        }
    }
    ops
}

/// Single pass of peephole optimization. Returns (new_ops, changed).
fn peephole_pass(ops: &[Op], constants: &mut Vec<f64>) -> (Vec<Op>, bool) {
    let mut result = Vec::with_capacity(ops.len());
    let mut changed = false;
    let mut i = 0;

    while i < ops.len() {
        // Pattern: PushConst(a), PushConst(b), <binop> → PushConst(a op b)
        if i + 2 < ops.len() {
            if let (Op::PushConst(ai), Op::PushConst(bi)) = (ops[i], ops[i + 1]) {
                let a = const_val(constants, ai);
                let b = const_val(constants, bi);
                if let Some(folded) = try_fold_op(ops[i + 2], a, b) {
                    if let Some(idx) = add_or_reuse_constant(constants, folded) {
                        result.push(Op::PushConst(idx));
                        changed = true;
                        i += 3;
                        continue;
                    }
                }
            }
        }

        // Pattern: PushConst(0.0), Add → remove both (identity: x + 0 = x)
        // Pattern: PushConst(0.0), Sub → remove both (identity: x - 0 = x)
        if i + 1 < ops.len() {
            if let Op::PushConst(ci) = ops[i] {
                let c = const_val(constants, ci);
                if c == 0.0 && matches!(ops[i + 1], Op::Add | Op::Sub) {
                    changed = true;
                    i += 2;
                    continue;
                }
                // Pattern: PushConst(1.0), Mul → remove both (identity: x * 1 = x)
                // Pattern: PushConst(1.0), Div → remove both (identity: x / 1 = x)
                #[allow(clippy::float_cmp)]
                if c == 1.0 && matches!(ops[i + 1], Op::Mul | Op::Div) {
                    changed = true;
                    i += 2;
                    continue;
                }
                // Pattern: PushConst(0.0), Mul → Pop, PushConst(0.0) (absorption: x * 0 = 0)
                if c == 0.0 && ops[i + 1] == Op::Mul {
                    result.push(Op::Pop);
                    result.push(Op::PushConst(ci));
                    changed = true;
                    i += 2;
                    continue;
                }
            }
        }

        // Pattern: Not, Not → remove both
        if i + 1 < ops.len() && ops[i] == Op::Not && ops[i + 1] == Op::Not {
            changed = true;
            i += 2;
            continue;
        }

        // Pattern: Neg, Neg → remove both
        if i + 1 < ops.len() && ops[i] == Op::Neg && ops[i + 1] == Op::Neg {
            changed = true;
            i += 2;
            continue;
        }

        result.push(ops[i]);
        i += 1;
    }

    // Fix up jump targets if we changed anything
    if changed {
        fixup_jumps(ops, constants, &mut result);
    }

    (result, changed)
}

/// Get the constant value at the given pool index.
fn const_val(constants: &[f64], idx: u16) -> f64 {
    debug_assert!((idx as usize) < constants.len(), "const_val: OOB index {idx}");
    constants.get(idx as usize).copied().unwrap_or(0.0)
}

/// Add a constant to the pool, reusing an existing index if possible.
/// Returns `None` if the pool would exceed u16 capacity.
fn add_or_reuse_constant(constants: &mut Vec<f64>, value: f64) -> Option<u16> {
    super::intern_constant(constants, value)
}

/// Try to evaluate a binary op on two constants.
/// Delegates to `ops::eval_binary_op` for standard binary operators,
/// and handles 2-arg all-float builtins via CallBuiltin.
fn try_fold_op(op: Op, a: f64, b: f64) -> Option<f64> {
    // Standard binary operators (Add, Sub, Mul, Div, etc.)
    if let Some(binop) = ops::op_to_binop(op) {
        return Some(ops::eval_binary_op(binop, a, b));
    }
    // Fold 2-arg all-float const-foldable builtins (e.g. min, max, pow, atan2)
    if let Op::CallBuiltin(idx) = op {
        use super::ast::TypeName;
        use super::builtins::BUILTINS;
        let bi = BUILTINS.get(idx as usize)?;
        if bi.params.len() == 2
            && bi.const_foldable
            && bi.params.iter().all(|(_, ty)| *ty == TypeName::Float)
            && bi.ret == TypeName::Float
        {
            let args = [ops::Value::Float(a), ops::Value::Float(b)];
            if let ops::Value::Float(result) = (bi.eval)(&args) {
                return Some(result);
            }
        }
    }
    None
}

/// Rebuild jump targets after peephole changes.
///
/// Strategy: build an offset map from old instruction indices to new ones,
/// then rewrite all Jump/JumpIfFalse targets.
fn fixup_jumps(old_ops: &[Op], constants: &[f64], new_ops: &mut [Op]) {
    // Build map: for old instruction index → new instruction index.
    // Walk old ops, replaying the same pattern detection to track how many
    // old ops map to how many new ops at each position.
    let mut old_to_new = vec![0usize; old_ops.len() + 1];
    let mut pos_in_new = 0usize;
    let mut i = 0;
    while i < old_ops.len() {
        old_to_new[i] = pos_in_new;

        // Detect the same patterns peephole_pass uses to know how many old ops
        // map to how many new ops at this position
        let (skip, new_count) = pattern_match(old_ops, constants, i);
        if skip > 0 {
            // This pattern was transformed — count how many new ops it produced
            pos_in_new += new_count;
            i += skip;
        } else {
            // Instruction survived as-is
            pos_in_new += 1;
            i += 1;
        }
    }
    old_to_new[old_ops.len()] = pos_in_new;

    // Rewrite jumps in new_ops
    for op in new_ops.iter_mut() {
        match op {
            Op::Jump(ref mut target) | Op::JumpIfFalse(ref mut target) => {
                let old_target = *target as usize;
                if old_target <= old_ops.len() {
                    *target = old_to_new[old_target] as u16;
                }
            }
            _ => {}
        }
    }
}

/// Detect peephole patterns at position `i`, returning `(old_ops_consumed, new_ops_produced)`.
/// Returns `(0, 0)` if no pattern matches (instruction survives as-is).
fn pattern_match(ops: &[Op], constants: &[f64], i: usize) -> (usize, usize) {
    // PushConst(a), PushConst(b), <binop> → PushConst(result): consumes 3, produces 1
    if i + 2 < ops.len() {
        if let (Op::PushConst(ai), Op::PushConst(bi)) = (ops[i], ops[i + 1]) {
            let a = const_val(constants, ai);
            let b = const_val(constants, bi);
            if try_fold_op(ops[i + 2], a, b).is_some() {
                return (3, 1);
            }
        }
    }

    if i + 1 < ops.len() {
        if let Op::PushConst(ci) = ops[i] {
            let c = const_val(constants, ci);
            // Identity: PushConst(0) + Add/Sub → removed (consumes 2, produces 0)
            if c == 0.0 && matches!(ops[i + 1], Op::Add | Op::Sub) {
                return (2, 0);
            }
            // Identity: PushConst(1) * Mul/Div → removed (consumes 2, produces 0)
            #[allow(clippy::float_cmp)]
            if c == 1.0 && matches!(ops[i + 1], Op::Mul | Op::Div) {
                return (2, 0);
            }
            // Absorption: PushConst(0) * Mul → Pop + PushConst(0) (consumes 2, produces 2)
            if c == 0.0 && ops[i + 1] == Op::Mul {
                return (2, 2);
            }
        }

        // Not, Not or Neg, Neg → removed (consumes 2, produces 0)
        if ops[i] == Op::Not && ops[i + 1] == Op::Not {
            return (2, 0);
        }
        if ops[i] == Op::Neg && ops[i + 1] == Op::Neg {
            return (2, 0);
        }
    }

    (0, 0)
}
