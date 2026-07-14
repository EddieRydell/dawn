use crate::dsl::types::Identifier;
use crate::dsl::{CompiledEffect, EffectKind, ParamDecl, Type, Value};
use crate::element::ElementSelection;
use crate::identity::SourceIdentity;
use crate::sequence::{MarkCollectionKey, SequenceLayerId};
use crate::values::{Curve, DawnDuration, DawnTime, Gradient};
use indexmap::IndexMap;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq)]
pub struct EffectInst {
    pub id: EffectInstId,
    pub layer_id: SequenceLayerId,
    pub start: DawnTime,
    pub duration: DawnDuration,
    pub target: ElementSelection,
    pub scope: EffectScope,
    pub definition: EffectRef,
    pub param_overrides: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectInstId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectDefinitionId(pub SourceIdentity);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum EffectRef {
    Builtin(BuiltinEffect),
    Custom(EffectDefinitionId),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum BuiltinEffect {
    Pulse,
    Chase,
    Spin,
    MarkPulse,
    MarkChase,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectScope {
    PerFixture,
    WholeTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectParamValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Color(crate::values::Color),
    Enum(Identifier),
    Marks(MarkCollectionKey),
    Curve(CurveSource),
    Gradient(GradientSource),
    Array(Vec<EffectParamValue>),
}

impl EffectParamValue {
    pub fn default_for_type(ty: &Type) -> Option<Self> {
        Self::from_default_value(ty.default_value())
    }

    fn from_default_value(value: Value) -> Option<Self> {
        match value {
            Value::Int(value) => Some(Self::Int(value)),
            Value::Float(value) => Some(Self::Float(value)),
            Value::Bool(value) => Some(Self::Bool(value)),
            Value::Color(value) => Some(Self::Color(value)),
            Value::Curve(value) => Some(Self::Curve(CurveSource::Inline((*value).clone()))),
            Value::Gradient(value) => {
                Some(Self::Gradient(GradientSource::Inline((*value).clone())))
            }
            Value::Array(values) => values
                .iter()
                .cloned()
                .map(Self::from_default_value)
                .collect::<Option<Vec<_>>>()
                .map(Self::Array),
            Value::Enum(value) => Some(Self::Enum(value)),
            Value::Void
            | Value::Marks(_)
            | Value::Target(_)
            | Value::TargetItems(_)
            | Value::TargetItem(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CurveSource {
    Inline(Curve),
    Reference(CurveId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum GradientSource {
    Inline(Gradient),
    Reference(GradientId),
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectDefinition {
    pub id: EffectRef,
    pub source_name: String,
    pub display_name: String,
    pub kind: EffectKind,
    pub params: Vec<ParamDecl>,
    pub implementation: EffectImplementation,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectImplementation {
    Native(BuiltinEffect),
    Dsl(CompiledEffect),
}

impl EffectDefinition {
    pub fn custom(id: EffectDefinitionId, compiled: CompiledEffect) -> Self {
        Self {
            id: EffectRef::Custom(id),
            source_name: compiled.name().as_str().to_string(),
            display_name: compiled.name().as_str().to_string(),
            kind: compiled.kind(),
            params: compiled.params().to_vec(),
            implementation: EffectImplementation::Dsl(compiled),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectDefinitionStore {
    pub definitions: IndexMap<EffectDefinitionId, EffectDefinition>,
}

impl EffectDefinitionStore {
    pub fn get(&self, key: &EffectDefinitionId) -> Option<&EffectDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(
        &mut self,
        key: EffectDefinitionId,
        definition: EffectDefinition,
    ) -> Option<EffectDefinition> {
        self.definitions.insert(key, definition)
    }

    pub fn resolve(&self, reference: &EffectRef) -> Option<&EffectDefinition> {
        match reference {
            EffectRef::Builtin(builtin) => Some(builtin.definition()),
            EffectRef::Custom(id) => self.get(id),
        }
    }
}

impl BuiltinEffect {
    pub const ALL: [Self; 5] = [
        Self::Pulse,
        Self::Chase,
        Self::Spin,
        Self::MarkPulse,
        Self::MarkChase,
    ];

    pub fn definition(self) -> &'static EffectDefinition {
        &BUILTIN_EFFECT_DEFINITIONS[self.index()]
    }

    pub fn from_source_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|effect| effect.definition().source_name == name)
    }

    fn index(self) -> usize {
        match self {
            Self::Pulse => 0,
            Self::Chase => 1,
            Self::Spin => 2,
            Self::MarkPulse => 3,
            Self::MarkChase => 4,
        }
    }
}

fn identifier(name: &str) -> crate::dsl::Identifier {
    crate::dsl::Identifier::new(name.to_string())
        .unwrap_or_else(|_| unreachable!("static identifier is valid"))
}

fn required(name: &str, ty: Type) -> ParamDecl {
    ParamDecl {
        name: identifier(name),
        ty,
        default: None,
    }
}
fn optional(name: &str, ty: Type, default: Value) -> ParamDecl {
    ParamDecl {
        name: identifier(name),
        ty,
        default: Some(default),
    }
}
fn gradient_mode() -> Type {
    Type::Enum(vec![
        identifier("through_effect"),
        identifier("across_items"),
        identifier("per_pulse"),
    ])
}
fn black() -> Value {
    Value::Color(crate::values::Color {
        red: 0,
        green: 0,
        blue: 0,
    })
}

static BUILTIN_EFFECT_DEFINITIONS: LazyLock<[EffectDefinition; 5]> = LazyLock::new(|| {
    let chase_params = || {
        vec![
            required("gradient", Type::Gradient),
            optional(
                "gradient_mode",
                gradient_mode(),
                Value::Enum(identifier("per_pulse")),
            ),
            optional("pulse_overlap", Type::Float, Value::Float(8.0)),
            optional("section_width_pixels", Type::Int, Value::Int(1)),
            required("chase_position", Type::Curve),
            optional("reverse", Type::Bool, Value::Bool(false)),
            optional("extend_to_start", Type::Bool, Value::Bool(false)),
            optional("extend_to_end", Type::Bool, Value::Bool(false)),
            required("pulse_shape", Type::Curve),
        ]
    };
    let make = |builtin, source_name: &str, display_name: &str, kind, params| EffectDefinition {
        id: EffectRef::Builtin(builtin),
        source_name: source_name.to_string(),
        display_name: display_name.to_string(),
        kind,
        params,
        implementation: EffectImplementation::Native(builtin),
    };
    [
        make(
            BuiltinEffect::Pulse,
            "pulse",
            "Pulse",
            EffectKind::Sample,
            vec![
                required("gradient", Type::Gradient),
                required("pulse_shape", Type::Curve),
            ],
        ),
        make(
            BuiltinEffect::Chase,
            "chase",
            "Chase",
            EffectKind::Sample,
            chase_params(),
        ),
        make(BuiltinEffect::Spin, "spin", "Spin", EffectKind::Sample, {
            let mut params = chase_params();
            params.insert(5, optional("revolutions", Type::Int, Value::Int(2)));
            params
        }),
        make(
            BuiltinEffect::MarkPulse,
            "mark_pulse",
            "Mark Pulse",
            EffectKind::Generator,
            vec![
                required("beats", Type::Marks),
                optional("base", Type::Color, black()),
                required("accent", Type::Gradient),
                required("hue", Type::Curve),
                optional("hue_mix", Type::Float, Value::Float(0.35)),
                optional("offset_seconds", Type::Float, Value::Float(0.0)),
                optional("decay_seconds", Type::Float, Value::Float(0.18)),
                optional("section_width_pixels", Type::Int, Value::Int(5)),
                optional("section_edge_fade_pixels", Type::Float, Value::Float(0.0)),
                optional("sections_per_mark", Type::Int, Value::Int(3)),
                optional("seed", Type::Float, Value::Float(0.0)),
            ],
        ),
        make(
            BuiltinEffect::MarkChase,
            "mark_chase",
            "Mark Chase",
            EffectKind::Generator,
            vec![
                required("beats", Type::Marks),
                optional("base", Type::Color, black()),
                optional(
                    "gradient_mode",
                    gradient_mode(),
                    Value::Enum(identifier("per_pulse")),
                ),
                required("gradients", Type::Array(Box::new(Type::Gradient))),
                required("hue", Type::Curve),
                optional("hue_mix", Type::Float, Value::Float(0.35)),
                optional("offset_seconds", Type::Float, Value::Float(0.0)),
                optional("chase_seconds", Type::Float, Value::Float(0.5)),
                optional("pulse_overlap", Type::Float, Value::Float(8.0)),
                optional("section_width_pixels", Type::Int, Value::Int(5)),
                required("chase_positions", Type::Array(Box::new(Type::Curve))),
                required("pulse_shape", Type::Curve),
            ],
        ),
    ]
});

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CurveId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct CurveDefinition {
    pub curve: Curve,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GradientId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct GradientDefinition {
    pub gradient: Gradient,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurveDefinitionStore {
    pub definitions: IndexMap<CurveId, CurveDefinition>,
}

impl CurveDefinitionStore {
    pub fn get(&self, key: &CurveId) -> Option<&CurveDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(&mut self, key: CurveId, curve: CurveDefinition) -> Option<CurveDefinition> {
        self.definitions.insert(key, curve)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradientDefinitionStore {
    pub definitions: IndexMap<GradientId, GradientDefinition>,
}

impl GradientDefinitionStore {
    pub fn get(&self, key: &GradientId) -> Option<&GradientDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(
        &mut self,
        key: GradientId,
        gradient: GradientDefinition,
    ) -> Option<GradientDefinition> {
        self.definitions.insert(key, gradient)
    }
}
