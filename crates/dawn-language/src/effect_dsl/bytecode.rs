use super::ast::{BinaryOp, UnaryOp};
use super::types::{Identifier, Type, Value};

pub(crate) type ConstantId = usize;
pub(crate) type LocalId = usize;
pub(crate) type ParamId = usize;
pub(crate) type Target = usize;

#[derive(Clone, Debug)]
pub(crate) struct BytecodeFunction {
    pub instructions: Vec<Instruction>,
    pub constants: Vec<Value>,
    pub local_count: usize,
    pub max_stack: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Instruction {
    LoadConst(ConstantId),
    LoadDefault(Type),
    LoadParam(ParamId),
    LoadGeneratorContext(GeneratorContextId),
    StoreParam(ParamId),
    LoadLocal(LocalId),
    StoreLocal(LocalId),
    Pop,
    MakeArray(usize),
    Index,
    CurveParamSample(ParamId),
    Member(Identifier),
    CoerceFloat,
    Unary(UnaryOp),
    Binary(BinaryOp),
    Jump(Target),
    JumpIfFalse(Target),
    JumpIfFalseOrPop(Target),
    JumpIfTrueOrPop(Target),
    CallBuiltin(Builtin, usize),
    CurveParamFloatClamped(ParamId),
    CurveParamColorScaled(ParamId),
    CurveParamCrossing {
        param: ParamId,
        has_fallback: bool,
    },
    CheckLoopLimit,
    Emit {
        effect: Identifier,
        fields: Vec<Identifier>,
    },
    Return,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GeneratorContextId {
    Timeline,
    Target,
    Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Builtin {
    Progress,
    Seconds,
    Duration,
    PixelIndex,
    PixelCount,
    PixelFraction,
    SectionPosition,
    Sin,
    Cos,
    Abs,
    Floor,
    Min,
    Max,
    Clamp,
    Smoothstep,
    Mix,
    Rgb,
    Hsv,
    Srand,
    Rand,
    CurveCrossing,
    CurveFloatClamped,
    CurveColorScaled,
    Len,
    MarkCount,
    MarkAt,
    MarkPrev,
    MarkPrevIndex,
    MarkNextIndex,
    MarkElapsed,
    MarkPhase,
    Fixtures,
    Pixels,
    Sections,
    Count,
    Pick,
}

impl Builtin {
    pub(crate) fn from_name(name: &Identifier) -> Option<Self> {
        Some(match name.as_str() {
            "progress" => Self::Progress,
            "seconds" => Self::Seconds,
            "duration" => Self::Duration,
            "pixel_index" => Self::PixelIndex,
            "pixel_count" => Self::PixelCount,
            "pixel_fraction" => Self::PixelFraction,
            "section_position" => Self::SectionPosition,
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "abs" => Self::Abs,
            "floor" => Self::Floor,
            "min" => Self::Min,
            "max" => Self::Max,
            "clamp" => Self::Clamp,
            "smoothstep" => Self::Smoothstep,
            "mix" => Self::Mix,
            "rgb" => Self::Rgb,
            "hsv" => Self::Hsv,
            "srand" => Self::Srand,
            "rand" => Self::Rand,
            "curve_crossing" => Self::CurveCrossing,
            "curve_float_clamped" => Self::CurveFloatClamped,
            "curve_color_scaled" => Self::CurveColorScaled,
            "len" => Self::Len,
            "mark_count" => Self::MarkCount,
            "mark_at" => Self::MarkAt,
            "mark_prev" => Self::MarkPrev,
            "mark_prev_index" => Self::MarkPrevIndex,
            "mark_next_index" => Self::MarkNextIndex,
            "mark_elapsed" => Self::MarkElapsed,
            "mark_phase" => Self::MarkPhase,
            "fixtures" => Self::Fixtures,
            "pixels" => Self::Pixels,
            "sections" => Self::Sections,
            "count" => Self::Count,
            "pick" => Self::Pick,
            _ => return None,
        })
    }
}
