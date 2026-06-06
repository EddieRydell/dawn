use super::{is_float_compatible, EffectScriptKind, ScriptType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltinContext {
    Progress,
    Seconds,
    Fixture,
    Pixel,
}

impl BuiltinContext {
    pub(super) const ALL: [Self; 4] = [Self::Progress, Self::Seconds, Self::Fixture, Self::Pixel];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Progress => "progress",
            Self::Seconds => "seconds",
            Self::Fixture => "fixture",
            Self::Pixel => "pixel",
        }
    }

    pub(super) fn value_type(self) -> ScriptType {
        match self {
            Self::Progress | Self::Seconds => ScriptType::Float,
            Self::Fixture => ScriptType::Fixture,
            Self::Pixel => ScriptType::Pixel,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltinConstant {
    Pi,
    Tau,
}

impl BuiltinConstant {
    pub(super) const ALL: [Self; 2] = [Self::Pi, Self::Tau];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Pi => "PI",
            Self::Tau => "TAU",
        }
    }

    pub(super) fn value(self) -> f64 {
        match self {
            Self::Pi => std::f64::consts::PI,
            Self::Tau => std::f64::consts::TAU,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltinFunction {
    Sin,
    Cos,
    Abs,
    Floor,
    Srand,
    Rand,
    PixelIndex,
    PixelCount,
    Fixtures,
    Pixels,
    Sections,
    Count,
    Pick,
    CurveCrossing,
    MarkCount,
    MarkAt,
    MarkPrev,
    MarkNext,
    MarkNearest,
    MarkPhase,
    MarkElapsed,
    Min,
    Max,
    Clamp,
    Smoothstep,
    Mix,
    Rgb,
    Hsv,
}

impl BuiltinFunction {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "sin" => Self::Sin,
            "cos" => Self::Cos,
            "abs" => Self::Abs,
            "floor" => Self::Floor,
            "srand" => Self::Srand,
            "rand" => Self::Rand,
            "pixel_index" => Self::PixelIndex,
            "pixel_count" => Self::PixelCount,
            "fixtures" => Self::Fixtures,
            "pixels" => Self::Pixels,
            "sections" => Self::Sections,
            "count" => Self::Count,
            "pick" => Self::Pick,
            "curve_crossing" => Self::CurveCrossing,
            "mark_count" => Self::MarkCount,
            "mark_at" => Self::MarkAt,
            "mark_prev" => Self::MarkPrev,
            "mark_next" => Self::MarkNext,
            "mark_nearest" => Self::MarkNearest,
            "mark_phase" => Self::MarkPhase,
            "mark_elapsed" => Self::MarkElapsed,
            "min" => Self::Min,
            "max" => Self::Max,
            "clamp" => Self::Clamp,
            "smoothstep" => Self::Smoothstep,
            "mix" => Self::Mix,
            "rgb" => Self::Rgb,
            "hsv" => Self::Hsv,
            _ => return None,
        })
    }

    pub(super) fn return_type(self, args: &[ScriptType]) -> Option<ScriptType> {
        self.return_type_for_kind(args, EffectScriptKind::Sample)
    }

    pub(super) fn return_type_for_kind(
        self,
        args: &[ScriptType],
        kind: EffectScriptKind,
    ) -> Option<ScriptType> {
        match (self, args) {
            (Self::Floor, [value])
                if kind == EffectScriptKind::Generator && is_float_compatible(*value) =>
            {
                Some(ScriptType::Int)
            }
            (Self::Sin | Self::Cos | Self::Abs | Self::Floor | Self::Srand, [value])
                if is_float_compatible(*value) =>
            {
                Some(ScriptType::Float)
            }
            (Self::Rand, []) => Some(ScriptType::Float),
            (Self::Rand, [seed, ScriptType::Int])
                if kind == EffectScriptKind::Generator && is_float_compatible(*seed) =>
            {
                Some(ScriptType::Float)
            }
            (Self::PixelIndex | Self::PixelCount, [ScriptType::Pixel]) => Some(ScriptType::Int),
            (Self::Fixtures | Self::Pixels, [ScriptType::Target])
                if kind == EffectScriptKind::Generator =>
            {
                Some(ScriptType::TargetItems)
            }
            (Self::Sections, [ScriptType::Target, ScriptType::Int])
                if kind == EffectScriptKind::Generator =>
            {
                Some(ScriptType::TargetItems)
            }
            (Self::Count, [ScriptType::TargetItems]) if kind == EffectScriptKind::Generator => {
                Some(ScriptType::Int)
            }
            (Self::Pick, [ScriptType::TargetItems, ScriptType::Int])
                if kind == EffectScriptKind::Generator =>
            {
                Some(ScriptType::TargetItem)
            }
            (Self::CurveCrossing, [ScriptType::CurveFloat, value, fallback])
                if is_float_compatible(*value) && is_float_compatible(*fallback) =>
            {
                Some(ScriptType::Float)
            }
            (Self::MarkCount, [ScriptType::Marks]) => Some(ScriptType::Int),
            (Self::MarkAt, [ScriptType::Marks, ScriptType::Int, fallback])
                if is_float_compatible(*fallback) =>
            {
                Some(ScriptType::Float)
            }
            (
                Self::MarkPrev
                | Self::MarkNext
                | Self::MarkNearest
                | Self::MarkPhase
                | Self::MarkElapsed,
                [ScriptType::Marks, time, fallback],
            ) if is_float_compatible(*time) && is_float_compatible(*fallback) => {
                Some(ScriptType::Float)
            }
            (Self::Min | Self::Max, [left, right])
                if is_float_compatible(*left) && is_float_compatible(*right) =>
            {
                Some(ScriptType::Float)
            }
            (Self::Clamp | Self::Smoothstep | Self::Mix, [first, second, third])
                if is_float_compatible(*first)
                    && is_float_compatible(*second)
                    && is_float_compatible(*third) =>
            {
                Some(ScriptType::Float)
            }
            (Self::Rgb | Self::Hsv, [first, second, third])
                if is_float_compatible(*first)
                    && is_float_compatible(*second)
                    && is_float_compatible(*third) =>
            {
                Some(ScriptType::Color)
            }
            (Self::Mix, [ScriptType::Color, ScriptType::Color, amount])
                if is_float_compatible(*amount) =>
            {
                Some(ScriptType::Color)
            }
            _ => None,
        }
    }
}
