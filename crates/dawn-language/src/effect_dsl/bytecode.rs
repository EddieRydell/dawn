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

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Instruction {
    LoadConst {
        dst: ValueSlot,
        constant: ConstantId,
    },
    LoadDefault {
        dst: ValueSlot,
        ty: Type,
    },
    LoadParam {
        dst: ValueSlot,
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
    Clamp {
        dst: FloatSlot,
        value: FloatSlot,
        min: FloatSlot,
        max: FloatSlot,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GeneratorContextId {
    Timeline,
    Target,
    Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContextRead {
    Progress,
    Seconds,
    Duration,
    PixelIndex,
    PixelCount,
    PixelFraction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloatUnary {
    Sin,
    Cos,
    Abs,
    Floor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArithmeticOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum IntArithmeticOp {
    Add,
    Subtract,
    Multiply,
    Remainder,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompareOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloatBinary {
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MarkOp {
    Count,
    At,
    Prev,
    PrevIndex,
    NextIndex,
    Elapsed,
    Phase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TargetItemsOp {
    Fixtures,
    Pixels,
    Sections,
    Count,
    Pick,
}
