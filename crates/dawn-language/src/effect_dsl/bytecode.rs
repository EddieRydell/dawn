use super::ast::{BinaryOp, UnaryOp};
use super::types::{Identifier, Type, Value};

pub(crate) type ConstantId = usize;
pub(crate) type LocalId = usize;
pub(crate) type ParamId = usize;
pub(crate) type RegisterId = usize;
pub(crate) type Target = usize;

#[derive(Clone, Debug)]
pub(crate) struct RegisterFunction {
    pub instructions: Vec<Instruction>,
    pub constants: Vec<Value>,
    pub register_count: usize,
    pub register_types: Vec<Type>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Instruction {
    LoadConst {
        dst: RegisterId,
        constant: ConstantId,
    },
    LoadDefault {
        dst: RegisterId,
        ty: Type,
    },
    LoadParam {
        dst: RegisterId,
        param: ParamId,
    },
    LoadGeneratorContext {
        dst: RegisterId,
        slot: GeneratorContextId,
    },
    StoreParam {
        param: ParamId,
        src: RegisterId,
    },
    Move {
        dst: RegisterId,
        src: RegisterId,
    },
    MakeArray {
        dst: RegisterId,
        items: Vec<RegisterId>,
    },
    Index {
        dst: RegisterId,
        target: RegisterId,
        index: RegisterId,
    },
    CurveParamSample {
        dst: RegisterId,
        param: ParamId,
        position: RegisterId,
    },
    Member {
        dst: RegisterId,
        target: RegisterId,
        member: Identifier,
    },
    CoerceFloat {
        dst: RegisterId,
        src: RegisterId,
    },
    Unary {
        dst: RegisterId,
        op: UnaryOp,
        src: RegisterId,
    },
    Binary {
        dst: RegisterId,
        op: BinaryOp,
        left: RegisterId,
        right: RegisterId,
    },
    Jump(Target),
    JumpIfFalse {
        condition: RegisterId,
        target: Target,
    },
    JumpIfTrue {
        condition: RegisterId,
        target: Target,
    },
    ContextRead {
        dst: RegisterId,
        read: ContextRead,
    },
    SectionPosition {
        dst: RegisterId,
        width: RegisterId,
    },
    FloatUnary {
        dst: RegisterId,
        op: FloatUnary,
        value: RegisterId,
    },
    FloatBinary {
        dst: RegisterId,
        op: FloatBinary,
        left: RegisterId,
        right: RegisterId,
    },
    Clamp {
        dst: RegisterId,
        value: RegisterId,
        min: RegisterId,
        max: RegisterId,
    },
    Smoothstep {
        dst: RegisterId,
        edge0: RegisterId,
        edge1: RegisterId,
        value: RegisterId,
    },
    Mix {
        dst: RegisterId,
        left: RegisterId,
        right: RegisterId,
        amount: RegisterId,
    },
    Rgb {
        dst: RegisterId,
        red: RegisterId,
        green: RegisterId,
        blue: RegisterId,
    },
    Hsv {
        dst: RegisterId,
        hue: RegisterId,
        saturation: RegisterId,
        value: RegisterId,
    },
    Rand {
        dst: RegisterId,
        args: Vec<RegisterId>,
    },
    CurveFloatClamped {
        dst: RegisterId,
        curve: RegisterId,
        position: RegisterId,
        min: RegisterId,
        max: RegisterId,
    },
    CurveParamFloatClamped {
        dst: RegisterId,
        param: ParamId,
        position: RegisterId,
        min: RegisterId,
        max: RegisterId,
    },
    CurveColorScaled {
        dst: RegisterId,
        curve: RegisterId,
        position: RegisterId,
        scale: RegisterId,
    },
    CurveParamColorScaled {
        dst: RegisterId,
        param: ParamId,
        position: RegisterId,
        scale: RegisterId,
    },
    CurveCrossing {
        dst: RegisterId,
        curve: RegisterId,
        value: RegisterId,
        fallback: Option<RegisterId>,
    },
    CurveParamCrossing {
        dst: RegisterId,
        param: ParamId,
        value: RegisterId,
        fallback: Option<RegisterId>,
    },
    Len {
        dst: RegisterId,
        value: RegisterId,
    },
    Mark {
        dst: RegisterId,
        op: MarkOp,
        args: Vec<RegisterId>,
    },
    TargetItems {
        dst: RegisterId,
        op: TargetItemsOp,
        args: Vec<RegisterId>,
    },
    CheckLoopLimit,
    Emit {
        effect: Identifier,
        fields: Vec<(Identifier, RegisterId)>,
    },
    Return(RegisterId),
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
