use super::types::{Identifier, Type, Value};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub(crate) type ConstantId = usize;
pub(crate) type LocalId = ValueSlot;
pub(crate) type ParamId = usize;
pub(crate) type Target = usize;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RegisterFunction {
    pub instructions: Vec<Instruction>,
    pub constants: Vec<Value>,
    pub layout: SlotLayout,
    pub layout_id: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct SlotLayout {
    pub ints: usize,
    pub floats: usize,
    pub bools: usize,
    pub colors: usize,
    pub refs: usize,
}

pub(crate) fn slot_layout_id(layout: SlotLayout) -> u64 {
    let mut hasher = DefaultHasher::new();
    layout.hash(&mut hasher);
    hasher.finish()
}

pub(crate) fn register_function_reads_only_written_slots(function: &RegisterFunction) -> bool {
    if function.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::StoreParam { .. } | Instruction::Rand { .. }
        )
    }) {
        return false;
    }

    let slot_count = function.layout.ints
        + function.layout.floats
        + function.layout.bools
        + function.layout.colors
        + function.layout.refs;
    let mut in_states = vec![None::<Vec<bool>>; function.instructions.len()];
    if in_states.is_empty() {
        return true;
    }
    in_states[0] = Some(vec![false; slot_count]);
    let mut worklist = vec![0usize];

    while let Some(index) = worklist.pop() {
        let Some(mut state) = in_states[index].clone() else {
            continue;
        };
        let instruction = &function.instructions[index];
        let mut valid = true;
        instruction_read_slots(instruction, |slot| {
            if !state[slot_index(function.layout, slot)] {
                valid = false;
            }
        });
        if !valid {
            return false;
        }
        instruction_write_slots(instruction, |slot| {
            state[slot_index(function.layout, slot)] = true;
        });
        for successor in instruction_successors(instruction, index, function.instructions.len()) {
            let changed = match &mut in_states[successor] {
                Some(existing) => {
                    let mut changed = false;
                    for (existing_slot, incoming_slot) in existing.iter_mut().zip(state.iter()) {
                        let merged = *existing_slot && *incoming_slot;
                        if *existing_slot != merged {
                            *existing_slot = merged;
                            changed = true;
                        }
                    }
                    changed
                }
                slot @ None => {
                    *slot = Some(state.clone());
                    true
                }
            };
            if changed {
                worklist.push(successor);
            }
        }
    }

    true
}

fn instruction_successors(
    instruction: &Instruction,
    index: usize,
    instruction_count: usize,
) -> Vec<usize> {
    let next = index + 1;
    match instruction {
        Instruction::Jump(target) => valid_successors([*target], instruction_count),
        Instruction::JumpIfFalse { target, .. } | Instruction::JumpIfTrue { target, .. } => {
            valid_successors([*target, next], instruction_count)
        }
        Instruction::Return(_) | Instruction::ReturnColor(_) => Vec::new(),
        _ => valid_successors([next], instruction_count),
    }
}

fn valid_successors<const N: usize>(targets: [usize; N], instruction_count: usize) -> Vec<usize> {
    targets
        .into_iter()
        .filter(|target| *target < instruction_count)
        .collect()
}

fn instruction_read_slots(instruction: &Instruction, mut read: impl FnMut(ValueSlot)) {
    match instruction {
        Instruction::StoreParam { src, .. } | Instruction::Move { src, .. } => read(*src),
        Instruction::MakeArray { items, .. } => {
            for item in items {
                read(*item);
            }
        }
        Instruction::Index { target, index, .. } => {
            read(*target);
            read(*index);
        }
        Instruction::CurveParamSample { position, .. } => read(ValueSlot::Float(*position)),
        Instruction::SignalSample { seconds, .. } => read(ValueSlot::Float(*seconds)),
        Instruction::Member { target, .. } => read(ValueSlot::Ref(*target)),
        Instruction::IntToFloat { src, .. } | Instruction::NegInt { src, .. } => {
            read(ValueSlot::Int(*src));
        }
        Instruction::Not { src, .. } => read(ValueSlot::Bool(*src)),
        Instruction::NegFloat { src, .. } | Instruction::FloatUnary { value: src, .. } => {
            read(ValueSlot::Float(*src));
        }
        Instruction::FloatArithmetic { left, right, .. }
        | Instruction::FloatCompare { left, right, .. }
        | Instruction::FloatBinary { left, right, .. }
        | Instruction::MixFloat { left, right, .. } => {
            read(ValueSlot::Float(*left));
            read(ValueSlot::Float(*right));
        }
        Instruction::FloatArithmeticConst { value, .. }
        | Instruction::FloatCompareConst { value, .. }
        | Instruction::FloatBinaryConst { value, .. }
        | Instruction::ClampConst { value, .. } => {
            read(ValueSlot::Float(*value));
        }
        Instruction::IntArithmetic { left, right, .. } => {
            read(ValueSlot::Int(*left));
            read(ValueSlot::Int(*right));
        }
        Instruction::ValueEqual { left, right, .. } => {
            read(*left);
            read(*right);
        }
        Instruction::JumpIfFalse { condition, .. } | Instruction::JumpIfTrue { condition, .. } => {
            read(ValueSlot::Bool(*condition));
        }
        Instruction::SectionPosition { width, .. } => read(ValueSlot::Float(*width)),
        Instruction::Clamp {
            value, min, max, ..
        } => {
            read(ValueSlot::Float(*value));
            read(ValueSlot::Float(*min));
            read(ValueSlot::Float(*max));
        }
        Instruction::Smoothstep {
            edge0,
            edge1,
            value,
            ..
        } => {
            read(ValueSlot::Float(*edge0));
            read(ValueSlot::Float(*edge1));
            read(ValueSlot::Float(*value));
        }
        Instruction::MixColor {
            left,
            right,
            amount,
            ..
        } => {
            read(ValueSlot::Color(*left));
            read(ValueSlot::Color(*right));
            read(ValueSlot::Float(*amount));
        }
        Instruction::ColorBinary { left, right, .. } => {
            read(ValueSlot::Color(*left));
            read(ValueSlot::Color(*right));
        }
        Instruction::ColorScale { color, scale, .. } => {
            read(ValueSlot::Color(*color));
            read(ValueSlot::Float(*scale));
        }
        Instruction::ColorIntensity { color, .. } | Instruction::ColorInvert { color, .. } => {
            read(ValueSlot::Color(*color));
        }
        Instruction::Rgb {
            red, green, blue, ..
        } => {
            read(ValueSlot::Float(*red));
            read(ValueSlot::Float(*green));
            read(ValueSlot::Float(*blue));
        }
        Instruction::Hsv {
            hue,
            saturation,
            value,
            ..
        } => {
            read(ValueSlot::Float(*hue));
            read(ValueSlot::Float(*saturation));
            read(ValueSlot::Float(*value));
        }
        Instruction::Rand { args, .. } => {
            for arg in args {
                read(ValueSlot::Float(*arg));
            }
        }
        Instruction::CurveFloatClamped {
            curve,
            position,
            min,
            max,
            ..
        } => {
            read(ValueSlot::Ref(*curve));
            read(ValueSlot::Float(*position));
            read(ValueSlot::Float(*min));
            read(ValueSlot::Float(*max));
        }
        Instruction::CurveParamFloatClamped {
            position, min, max, ..
        } => {
            read(ValueSlot::Float(*position));
            read(ValueSlot::Float(*min));
            read(ValueSlot::Float(*max));
        }
        Instruction::CurveColorScaled {
            curve,
            position,
            scale,
            ..
        } => {
            read(ValueSlot::Ref(*curve));
            read(ValueSlot::Float(*position));
            read(ValueSlot::Float(*scale));
        }
        Instruction::CurveParamColorScaled {
            position, scale, ..
        } => {
            read(ValueSlot::Float(*position));
            read(ValueSlot::Float(*scale));
        }
        Instruction::CurveCrossing {
            curve,
            value,
            fallback,
            ..
        } => {
            read(ValueSlot::Ref(*curve));
            read(ValueSlot::Float(*value));
            if let Some(fallback) = fallback {
                read(ValueSlot::Float(*fallback));
            }
        }
        Instruction::CurveParamCrossing {
            value, fallback, ..
        } => {
            read(ValueSlot::Float(*value));
            if let Some(fallback) = fallback {
                read(ValueSlot::Float(*fallback));
            }
        }
        Instruction::Len { value, .. } => read(*value),
        Instruction::Mark { args, .. } | Instruction::TargetItems { args, .. } => {
            for arg in args {
                read(*arg);
            }
        }
        Instruction::Emit { fields, .. } => {
            for (_, value) in fields {
                read(*value);
            }
        }
        Instruction::Return(value) => read(*value),
        Instruction::ReturnColor(value) => read(ValueSlot::Color(*value)),
        Instruction::LoadConst { .. }
        | Instruction::LoadDefault { .. }
        | Instruction::LoadIntParam { .. }
        | Instruction::LoadFloatParam { .. }
        | Instruction::LoadBoolParam { .. }
        | Instruction::LoadColorParam { .. }
        | Instruction::LoadRefParam { .. }
        | Instruction::LoadGeneratorContext { .. }
        | Instruction::EnumParamEqualConst { .. }
        | Instruction::Jump(_)
        | Instruction::ContextRead { .. }
        | Instruction::CheckLoopLimit => {}
    }
}

fn instruction_write_slots(instruction: &Instruction, mut write: impl FnMut(ValueSlot)) {
    match instruction {
        Instruction::LoadConst { dst, .. }
        | Instruction::LoadDefault { dst, .. }
        | Instruction::LoadGeneratorContext { dst, .. }
        | Instruction::Move { dst, .. }
        | Instruction::Index { dst, .. }
        | Instruction::CurveParamSample { dst, .. }
        | Instruction::ContextRead { dst, .. }
        | Instruction::Mark { dst, .. }
        | Instruction::TargetItems { dst, .. } => write(*dst),
        Instruction::LoadIntParam { dst, .. }
        | Instruction::NegInt { dst, .. }
        | Instruction::IntArithmetic { dst, .. }
        | Instruction::Len { dst, .. } => write(ValueSlot::Int(*dst)),
        Instruction::LoadFloatParam { dst, .. }
        | Instruction::IntToFloat { dst, .. }
        | Instruction::NegFloat { dst, .. }
        | Instruction::FloatArithmetic { dst, .. }
        | Instruction::FloatArithmeticConst { dst, .. }
        | Instruction::SectionPosition { dst, .. }
        | Instruction::FloatUnary { dst, .. }
        | Instruction::FloatBinary { dst, .. }
        | Instruction::FloatBinaryConst { dst, .. }
        | Instruction::Clamp { dst, .. }
        | Instruction::ClampConst { dst, .. }
        | Instruction::Smoothstep { dst, .. }
        | Instruction::MixFloat { dst, .. }
        | Instruction::Rand { dst, .. }
        | Instruction::CurveFloatClamped { dst, .. }
        | Instruction::CurveParamFloatClamped { dst, .. }
        | Instruction::CurveCrossing { dst, .. }
        | Instruction::CurveParamCrossing { dst, .. } => write(ValueSlot::Float(*dst)),
        Instruction::ColorIntensity { dst, .. } => write(ValueSlot::Float(*dst)),
        Instruction::LoadBoolParam { dst, .. }
        | Instruction::Not { dst, .. }
        | Instruction::FloatCompare { dst, .. }
        | Instruction::FloatCompareConst { dst, .. }
        | Instruction::ValueEqual { dst, .. }
        | Instruction::EnumParamEqualConst { dst, .. } => write(ValueSlot::Bool(*dst)),
        Instruction::LoadColorParam { dst, .. }
        | Instruction::MixColor { dst, .. }
        | Instruction::Rgb { dst, .. }
        | Instruction::Hsv { dst, .. }
        | Instruction::CurveColorScaled { dst, .. }
        | Instruction::CurveParamColorScaled { dst, .. } => write(ValueSlot::Color(*dst)),
        Instruction::SignalSample { dst, .. }
        | Instruction::ColorBinary { dst, .. }
        | Instruction::ColorScale { dst, .. }
        | Instruction::ColorInvert { dst, .. } => write(ValueSlot::Color(*dst)),
        Instruction::LoadRefParam { dst, .. } | Instruction::MakeArray { dst, .. } => {
            write(ValueSlot::Ref(*dst));
        }
        Instruction::Member { dst, .. } => write(*dst),
        Instruction::StoreParam { .. }
        | Instruction::Jump(_)
        | Instruction::JumpIfFalse { .. }
        | Instruction::JumpIfTrue { .. }
        | Instruction::CheckLoopLimit
        | Instruction::Emit { .. }
        | Instruction::Return(_)
        | Instruction::ReturnColor(_) => {}
    }
}

fn slot_index(layout: SlotLayout, slot: ValueSlot) -> usize {
    match slot {
        ValueSlot::Int(slot) => slot.0,
        ValueSlot::Float(slot) => layout.ints + slot.0,
        ValueSlot::Bool(slot) => layout.ints + layout.floats + slot.0,
        ValueSlot::Color(slot) => layout.ints + layout.floats + layout.bools + slot.0,
        ValueSlot::Ref(slot) => layout.ints + layout.floats + layout.bools + layout.colors + slot.0,
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct IntSlot(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct FloatSlot(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct BoolSlot(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ColorSlot(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RefSlot(pub usize);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ValueSlot {
    Int(IntSlot),
    Float(FloatSlot),
    Bool(BoolSlot),
    Color(ColorSlot),
    Ref(RefSlot),
}

impl ValueSlot {
    pub(crate) fn for_type(ty: &Type, layout: &mut SlotLayout) -> Self {
        match ty {
            Type::Int => {
                let slot = IntSlot(layout.ints);
                layout.ints += 1;
                Self::Int(slot)
            }
            Type::Float => {
                let slot = FloatSlot(layout.floats);
                layout.floats += 1;
                Self::Float(slot)
            }
            Type::Bool => {
                let slot = BoolSlot(layout.bools);
                layout.bools += 1;
                Self::Bool(slot)
            }
            Type::Color => {
                let slot = ColorSlot(layout.colors);
                layout.colors += 1;
                Self::Color(slot)
            }
            Type::Void
            | Type::Signal
            | Type::Marks
            | Type::Timeline
            | Type::Target
            | Type::TargetItems
            | Type::TargetItem
            | Type::Curve(_)
            | Type::Array(_)
            | Type::Enum(_) => {
                let slot = RefSlot(layout.refs);
                layout.refs += 1;
                Self::Ref(slot)
            }
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq)]
pub(crate) enum Instruction {
    LoadConst {
        dst: ValueSlot,
        constant: ConstantId,
    },
    LoadDefault {
        dst: ValueSlot,
        ty: Type,
    },
    LoadIntParam {
        dst: IntSlot,
        param: ParamId,
    },
    LoadFloatParam {
        dst: FloatSlot,
        param: ParamId,
    },
    LoadBoolParam {
        dst: BoolSlot,
        param: ParamId,
    },
    LoadColorParam {
        dst: ColorSlot,
        param: ParamId,
    },
    LoadRefParam {
        dst: RefSlot,
        param: ParamId,
    },
    LoadGeneratorContext {
        dst: ValueSlot,
        slot: GeneratorContextId,
    },
    StoreParam {
        param: ParamId,
        src: ValueSlot,
    },
    Move {
        dst: ValueSlot,
        src: ValueSlot,
    },
    MakeArray {
        dst: RefSlot,
        items: Vec<ValueSlot>,
    },
    Index {
        dst: ValueSlot,
        target: ValueSlot,
        index: ValueSlot,
    },
    CurveParamSample {
        dst: ValueSlot,
        param: ParamId,
        position: FloatSlot,
    },
    SignalSample {
        dst: ColorSlot,
        input: usize,
        seconds: FloatSlot,
    },
    Member {
        dst: ValueSlot,
        target: RefSlot,
        member: Identifier,
    },
    IntToFloat {
        dst: FloatSlot,
        src: IntSlot,
    },
    Not {
        dst: BoolSlot,
        src: BoolSlot,
    },
    NegInt {
        dst: IntSlot,
        src: IntSlot,
    },
    NegFloat {
        dst: FloatSlot,
        src: FloatSlot,
    },
    FloatArithmetic {
        dst: FloatSlot,
        op: ArithmeticOp,
        left: FloatSlot,
        right: FloatSlot,
    },
    FloatArithmeticConst {
        dst: FloatSlot,
        op: ArithmeticOp,
        value: FloatSlot,
        constant_bits: u64,
        constant_left: bool,
    },
    IntArithmetic {
        dst: IntSlot,
        op: IntArithmeticOp,
        left: IntSlot,
        right: IntSlot,
    },
    FloatCompare {
        dst: BoolSlot,
        op: CompareOp,
        left: FloatSlot,
        right: FloatSlot,
    },
    FloatCompareConst {
        dst: BoolSlot,
        op: CompareOp,
        value: FloatSlot,
        constant_bits: u64,
        constant_left: bool,
    },
    ValueEqual {
        dst: BoolSlot,
        negate: bool,
        left: ValueSlot,
        right: ValueSlot,
    },
    EnumParamEqualConst {
        dst: BoolSlot,
        param: ParamId,
        constant: ConstantId,
        negate: bool,
    },
    Jump(Target),
    JumpIfFalse {
        condition: BoolSlot,
        target: Target,
    },
    JumpIfTrue {
        condition: BoolSlot,
        target: Target,
    },
    ContextRead {
        dst: ValueSlot,
        read: ContextRead,
    },
    SectionPosition {
        dst: FloatSlot,
        width: FloatSlot,
    },
    FloatUnary {
        dst: FloatSlot,
        op: FloatUnary,
        value: FloatSlot,
    },
    FloatBinary {
        dst: FloatSlot,
        op: FloatBinary,
        left: FloatSlot,
        right: FloatSlot,
    },
    FloatBinaryConst {
        dst: FloatSlot,
        op: FloatBinary,
        value: FloatSlot,
        constant_bits: u64,
    },
    Clamp {
        dst: FloatSlot,
        value: FloatSlot,
        min: FloatSlot,
        max: FloatSlot,
    },
    ClampConst {
        dst: FloatSlot,
        value: FloatSlot,
        min_bits: u64,
        max_bits: u64,
    },
    Smoothstep {
        dst: FloatSlot,
        edge0: FloatSlot,
        edge1: FloatSlot,
        value: FloatSlot,
    },
    MixFloat {
        dst: FloatSlot,
        left: FloatSlot,
        right: FloatSlot,
        amount: FloatSlot,
    },
    MixColor {
        dst: ColorSlot,
        left: ColorSlot,
        right: ColorSlot,
        amount: FloatSlot,
    },
    ColorBinary {
        dst: ColorSlot,
        op: ColorBinary,
        left: ColorSlot,
        right: ColorSlot,
    },
    ColorScale {
        dst: ColorSlot,
        color: ColorSlot,
        scale: FloatSlot,
    },
    ColorIntensity {
        dst: FloatSlot,
        color: ColorSlot,
    },
    ColorInvert {
        dst: ColorSlot,
        color: ColorSlot,
    },
    Rgb {
        dst: ColorSlot,
        red: FloatSlot,
        green: FloatSlot,
        blue: FloatSlot,
    },
    Hsv {
        dst: ColorSlot,
        hue: FloatSlot,
        saturation: FloatSlot,
        value: FloatSlot,
    },
    Rand {
        dst: FloatSlot,
        args: Vec<FloatSlot>,
    },
    CurveFloatClamped {
        dst: FloatSlot,
        curve: RefSlot,
        position: FloatSlot,
        min: FloatSlot,
        max: FloatSlot,
    },
    CurveParamFloatClamped {
        dst: FloatSlot,
        param: ParamId,
        position: FloatSlot,
        min: FloatSlot,
        max: FloatSlot,
    },
    CurveColorScaled {
        dst: ColorSlot,
        curve: RefSlot,
        position: FloatSlot,
        scale: FloatSlot,
    },
    CurveParamColorScaled {
        dst: ColorSlot,
        param: ParamId,
        position: FloatSlot,
        scale: FloatSlot,
    },
    CurveCrossing {
        dst: FloatSlot,
        curve: RefSlot,
        value: FloatSlot,
        fallback: Option<FloatSlot>,
    },
    CurveParamCrossing {
        dst: FloatSlot,
        param: ParamId,
        value: FloatSlot,
        fallback: Option<FloatSlot>,
    },
    Len {
        dst: IntSlot,
        value: ValueSlot,
    },
    Mark {
        dst: ValueSlot,
        op: MarkOp,
        args: Vec<ValueSlot>,
    },
    TargetItems {
        dst: ValueSlot,
        op: TargetItemsOp,
        args: Vec<ValueSlot>,
    },
    CheckLoopLimit,
    Emit {
        effect: Identifier,
        fields: Vec<(Identifier, ValueSlot)>,
    },
    Return(ValueSlot),
    ReturnColor(ColorSlot),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum GeneratorContextId {
    Timeline,
    Target,
    Duration,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ContextRead {
    Progress,
    Seconds,
    Duration,
    PixelIndex,
    PixelCount,
    PixelFraction,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum FloatUnary {
    Sin,
    Cos,
    Abs,
    Floor,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ColorBinary {
    Add,
    Multiply,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum ArithmeticOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum IntArithmeticOp {
    Add,
    Subtract,
    Multiply,
    Remainder,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum CompareOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum FloatBinary {
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum MarkOp {
    Count,
    At,
    Prev,
    PrevIndex,
    NextIndex,
    Elapsed,
    Phase,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum TargetItemsOp {
    Fixtures,
    Pixels,
    Sections,
    Count,
    Pick,
}
