use super::{RuntimeValue, ScriptType};

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
