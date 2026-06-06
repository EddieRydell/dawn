use super::{RuntimeValue, ScriptType};
use std::collections::{HashSet, VecDeque};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct BytecodeProgram {
    pub(super) instructions: Vec<Instruction>,
    pub(super) constants: Vec<RuntimeValue>,
    pub(super) registers: RegisterCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
    PixelPosition(FloatSlot, PixelSlot),
    SectionPosition(FloatSlot, PixelSlot, FloatSlot),
    MarkCount(IntSlot, RefSlot),
    MarkAt(FloatSlot, RefSlot, IntSlot, FloatSlot),
    MarkSearch(
        FloatSlot,
        MarkSearchInstruction,
        RefSlot,
        FloatSlot,
        FloatSlot,
    ),
    CurveCrossing(FloatSlot, RefSlot, FloatSlot, FloatSlot),
    CurveParamCrossing(FloatSlot, usize, FloatSlot, FloatSlot),
    Min(FloatSlot, FloatSlot, FloatSlot),
    Max(FloatSlot, FloatSlot, FloatSlot),
    Clamp(FloatSlot, FloatSlot, FloatSlot, FloatSlot),
    Smoothstep(FloatSlot, FloatSlot, FloatSlot, FloatSlot),
    MixFloat(FloatSlot, FloatSlot, FloatSlot, FloatSlot),
    MixColor(ColorSlot, ColorSlot, ColorSlot, FloatSlot),
    Rgb(ColorSlot, FloatSlot, FloatSlot, FloatSlot),
    Hsv(ColorSlot, FloatSlot, FloatSlot, FloatSlot),
    CallFloatCurveParam(FloatSlot, usize, FloatSlot),
    CallColorCurveParam(ColorSlot, usize, FloatSlot),
    CurveFloatClamped(FloatSlot, usize, FloatSlot, FloatSlot, FloatSlot),
    CurveColorScaled(ColorSlot, usize, FloatSlot, FloatSlot),
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
    EnumEqual,
    EnumNotEqual,
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
    pub(super) fn value_type(self) -> ScriptType {
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

    pub(super) fn float(self) -> FloatSlot {
        match self {
            Self::Float(slot) => slot,
            _ => unreachable!("type checker validates float slot"),
        }
    }

    pub(super) fn int(self) -> IntSlot {
        match self {
            Self::Int(slot) => slot,
            _ => unreachable!("type checker validates int slot"),
        }
    }

    pub(super) fn bool(self) -> BoolSlot {
        match self {
            Self::Bool(slot) => slot,
            _ => unreachable!("type checker validates bool slot"),
        }
    }

    pub(super) fn color(self) -> ColorSlot {
        match self {
            Self::Color(slot) => slot,
            _ => unreachable!("type checker validates color slot"),
        }
    }

    pub(super) fn reference(self) -> RefSlot {
        match self {
            Self::Ref(slot, _) => slot,
            _ => unreachable!("type checker validates ref slot"),
        }
    }

    pub(super) fn pixel(self) -> PixelSlot {
        match self {
            Self::Pixel(slot) => slot,
            _ => unreachable!("type checker validates pixel slot"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FloatSlot(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct IntSlot(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BoolSlot(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ColorSlot(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RefSlot(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FixtureSlot(pub(super) usize);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PixelSlot(pub(super) usize);

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

pub(super) fn stats_for_program(program: &BytecodeProgram, param_slots: usize) -> BytecodeStats {
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

pub(super) fn specialize_for_params(
    program: &BytecodeProgram,
    param_values: &[RuntimeValue],
) -> BytecodeProgram {
    let mut constants = program.constants.clone();
    let mut instructions = program.instructions.clone();
    let mut ref_sources = vec![RefSource::Unknown; program.registers.refs];
    let mut bool_values = vec![None; program.registers.bools];

    for (index, instruction_ref) in instructions.iter_mut().enumerate() {
        let instruction = *instruction_ref;
        match instruction {
            Instruction::LoadConst(dest, constant_index) => match dest {
                ValueSlot::Ref(slot, ScriptType::Enum) => {
                    ref_sources[slot.0] = match &constants[constant_index] {
                        RuntimeValue::Enum(value) => RefSource::EnumConstant(value.clone()),
                        _ => RefSource::Unknown,
                    };
                }
                ValueSlot::Bool(slot) => {
                    bool_values[slot.0] = match constants[constant_index] {
                        RuntimeValue::Bool(value) => Some(value),
                        _ => None,
                    };
                }
                _ => clear_dest(dest, &mut ref_sources, &mut bool_values),
            },
            Instruction::LoadParam(dest, param_index) => match dest {
                ValueSlot::Ref(slot, ScriptType::Enum) => {
                    ref_sources[slot.0] = match &param_values[param_index] {
                        RuntimeValue::Enum(value) => RefSource::EnumParameterValue(value.clone()),
                        _ => RefSource::Unknown,
                    };
                }
                _ => clear_dest(dest, &mut ref_sources, &mut bool_values),
            },
            Instruction::Copy(dest, source) => match (dest, source) {
                (
                    ValueSlot::Ref(dest, ScriptType::Enum),
                    ValueSlot::Ref(source, ScriptType::Enum),
                ) => {
                    ref_sources[dest.0] = ref_sources[source.0].clone();
                }
                (ValueSlot::Bool(dest), ValueSlot::Bool(source)) => {
                    bool_values[dest.0] = bool_values[source.0];
                }
                (dest, _) => clear_dest(dest, &mut ref_sources, &mut bool_values),
            },
            Instruction::BoolNot(dest, source) => {
                bool_values[dest.0] = bool_values[source.0].map(|value| !value);
            }
            Instruction::Binary(dest, op, left, right)
                if matches!(
                    op,
                    BinaryInstruction::EnumEqual | BinaryInstruction::EnumNotEqual
                ) =>
            {
                let known = enum_source_value(left, &ref_sources)
                    .zip(enum_source_value(right, &ref_sources))
                    .map(|(left, right)| match op {
                        BinaryInstruction::EnumEqual => left == right,
                        BinaryInstruction::EnumNotEqual => left != right,
                        _ => unreachable!(),
                    });
                if let (ValueSlot::Bool(slot), Some(value)) = (dest, known) {
                    bool_values[slot.0] = Some(value);
                    let constant_index = constants.len();
                    constants.push(RuntimeValue::Bool(value));
                    *instruction_ref = Instruction::LoadConst(dest, constant_index);
                } else {
                    clear_dest(dest, &mut ref_sources, &mut bool_values);
                }
            }
            Instruction::Binary(dest, _, _, _) => {
                clear_dest(dest, &mut ref_sources, &mut bool_values)
            }
            Instruction::JumpIfFalse(condition, target) => {
                if let Some(value) = bool_values[condition.0] {
                    *instruction_ref = if value {
                        Instruction::Jump(index + 1)
                    } else {
                        Instruction::Jump(target)
                    };
                }
            }
            Instruction::JumpIfTrue(condition, target) => {
                if let Some(value) = bool_values[condition.0] {
                    *instruction_ref = if value {
                        Instruction::Jump(target)
                    } else {
                        Instruction::Jump(index + 1)
                    };
                }
            }
            _ => {}
        }
    }

    compact_reachable(BytecodeProgram {
        instructions,
        constants,
        registers: program.registers,
    })
}

#[derive(Debug, Clone)]
enum RefSource {
    EnumParameterValue(String),
    EnumConstant(String),
    Unknown,
}

fn clear_dest(dest: ValueSlot, ref_sources: &mut [RefSource], bool_values: &mut [Option<bool>]) {
    match dest {
        ValueSlot::Ref(slot, _) => ref_sources[slot.0] = RefSource::Unknown,
        ValueSlot::Bool(slot) => bool_values[slot.0] = None,
        _ => {}
    }
}

fn enum_source_value(slot: ValueSlot, ref_sources: &[RefSource]) -> Option<&str> {
    match &ref_sources[slot.reference().0] {
        RefSource::EnumParameterValue(value) | RefSource::EnumConstant(value) => Some(value),
        RefSource::Unknown => None,
    }
}

fn compact_reachable(program: BytecodeProgram) -> BytecodeProgram {
    let mut reachable = HashSet::new();
    let mut queue = VecDeque::from([0usize]);
    while let Some(index) = queue.pop_front() {
        if index >= program.instructions.len() || !reachable.insert(index) {
            continue;
        }
        match program.instructions[index] {
            Instruction::Jump(target) => queue.push_back(target),
            Instruction::JumpIfFalse(_, target) | Instruction::JumpIfTrue(_, target) => {
                queue.push_back(index + 1);
                queue.push_back(target);
            }
            Instruction::ReturnColor(_) => {}
            _ => queue.push_back(index + 1),
        }
    }

    let mut remap = vec![usize::MAX; program.instructions.len() + 1];
    let mut instructions = Vec::new();
    for (index, instruction) in program.instructions.iter().copied().enumerate() {
        if reachable.contains(&index) {
            remap[index] = instructions.len();
            instructions.push(instruction);
        }
    }
    remap[program.instructions.len()] = instructions.len();
    for instruction in &mut instructions {
        match instruction {
            Instruction::Jump(target)
            | Instruction::JumpIfFalse(_, target)
            | Instruction::JumpIfTrue(_, target) => *target = remap[*target],
            _ => {}
        }
    }
    BytecodeProgram {
        instructions,
        constants: program.constants,
        registers: program.registers,
    }
}
