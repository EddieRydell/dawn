use super::GeneratedEffectRef;
use super::types::{Identifier, Type, Value};
use alloc::boxed::Box;

pub type ConstantId = usize;
pub type LocalId = ValueSlot;
pub type ParamId = usize;
pub type Target = usize;

#[derive(Clone, Debug, PartialEq)]
pub struct BytecodeProgram {
    pub instructions: Box<[Instruction]>,
    pub constants: Box<[Value]>,
    pub value_operands: Box<[ValueSlot]>,
    pub emit_fields: Box<[(Identifier, ValueSlot)]>,
    pub generated_effects: Box<[GeneratedEffectRef]>,
    pub layout: SlotLayout,
}

impl BytecodeProgram {
    pub fn value_operands(&self, span: PoolSpan) -> Option<&[ValueSlot]> {
        self.value_operands.get(span.range())
    }

    pub fn emit_fields(&self, span: PoolSpan) -> Option<&[(Identifier, ValueSlot)]> {
        self.emit_fields.get(span.range())
    }

    pub fn generated_effect(&self, index: u32) -> Option<&GeneratedEffectRef> {
        self.generated_effects.get(index as usize)
    }

    pub fn sample_effect(
        &self,
        params: &super::BoundParams,
        context: &super::RunContext,
        workspace: &mut super::VmWorkspace,
    ) -> Result<crate::values::Color, super::RuntimeError> {
        super::vm::run_sample_program(self, params, context, workspace)
    }

    pub fn sample_operator(
        &self,
        params: &super::BoundParams,
        context: &super::OperatorRunContext,
        sampler: &mut dyn super::SignalSampler,
        workspace: &mut super::VmWorkspace,
    ) -> Result<crate::values::Color, super::RuntimeError> {
        super::vm::run_operator_program(self, params, context, sampler, workspace)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PoolSpan {
    pub start: u32,
    pub len: u32,
}

impl PoolSpan {
    fn range(self) -> core::ops::Range<usize> {
        self.start as usize..self.start.saturating_add(self.len) as usize
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct SlotLayout {
    pub ints: u32,
    pub floats: u32,
    pub bools: u32,
    pub colors: u32,
    pub refs: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct IntSlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct FloatSlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BoolSlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ColorSlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RefSlot(pub u32);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ValueSlot {
    Int(IntSlot),
    Float(FloatSlot),
    Bool(BoolSlot),
    Color(ColorSlot),
    Ref(RefSlot),
}

impl ValueSlot {
    pub fn for_type(ty: &Type, layout: &mut SlotLayout) -> Self {
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
            | Type::Curve
            | Type::Gradient
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
pub enum Instruction {
    LoadConst {
        dst: ValueSlot,
        constant: ConstantId,
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
        items: PoolSpan,
    },
    Index {
        dst: ValueSlot,
        target: ValueSlot,
        index: ValueSlot,
    },
    CurveParamSample {
        dst: FloatSlot,
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
        member: TargetMember,
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
        constant_bits: u32,
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
        constant_bits: u32,
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
        constant_bits: u32,
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
        min_bits: u32,
        max_bits: u32,
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
        args: PoolSpan,
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
    GradientColorScaled {
        dst: ColorSlot,
        gradient: RefSlot,
        position: FloatSlot,
        scale: FloatSlot,
    },
    GradientParamColorScaled {
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
        args: PoolSpan,
    },
    TargetItems {
        dst: ValueSlot,
        op: TargetItemsOp,
        args: PoolSpan,
    },
    CheckLoopLimit,
    Emit {
        effect: u32,
        fields: PoolSpan,
    },
    Return(ValueSlot),
    ReturnColor(ColorSlot),
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TargetMember {
    ElementIndex,
    ElementCellIndex,
    PixelIndex,
    PixelCount,
    PixelFraction,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum GeneratorContextId {
    Timeline,
    Target,
    Duration,
}

#[cfg(test)]
mod representation_tests {
    use super::{BytecodeProgram, Instruction};

    #[test]
    fn bytecode_headers_stay_compact() {
        assert!(size_of::<Instruction>() <= 32);
        assert!(size_of::<BytecodeProgram>() <= 104);
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContextRead {
    Progress,
    Seconds,
    Duration,
    PixelIndex,
    PixelCount,
    PixelFraction,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FloatUnary {
    Sin,
    Cos,
    Abs,
    Floor,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ColorBinary {
    Add,
    Multiply,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArithmeticOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntArithmeticOp {
    Add,
    Subtract,
    Multiply,
    Remainder,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CompareOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FloatBinary {
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MarkOp {
    Count,
    At,
    Prev,
    PrevIndex,
    NextIndex,
    Elapsed,
    Phase,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TargetItemsOp {
    Fixtures,
    Pixels,
    Sections,
    Count,
    Pick,
}
