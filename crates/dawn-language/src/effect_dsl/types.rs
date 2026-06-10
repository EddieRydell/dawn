#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectDslIdentifier(String);

impl EffectDslIdentifier {
    pub fn new(value: String) -> Result<Self, EffectDslIdentifierError> {
        if value.is_empty() {
            return Err(EffectDslIdentifierError::Empty);
        }

        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return Err(EffectDslIdentifierError::Empty);
        };

        if !is_identifier_start(first) {
            return Err(EffectDslIdentifierError::InvalidStart);
        }

        if chars.any(|candidate| !is_identifier_continue(candidate)) {
            return Err(EffectDslIdentifierError::InvalidCharacter);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectDslIdentifierError {
    Empty,
    InvalidStart,
    InvalidCharacter,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectDslEnumOption {
    pub name: EffectDslIdentifier,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectDslEnumType {
    pub options: Vec<EffectDslEnumOption>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum EffectDslType {
    Void,
    Int,
    Float,
    Bool,
    Color,
    Curve(Box<EffectDslType>),
    Array(Box<EffectDslType>),
    Enum(EffectDslEnumType),
}

pub enum EffectDslValue {
    Void,
    Int(i64),
    Float(f64),
    Bool(bool),
    Color(EffectDslColor),
    Curve(EffectDslCurve),
    Array(Vec<EffectDslValue>),
    Enum(EffectDslIdentifier),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct EffectDslColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectDslCurve {
    pub points: Vec<EffectDslCurvePoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectDslCurvePoint {
    pub position: f64,
    pub value: EffectDslCurveValue,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectDslCurveValue {
    Float(f64),
    Color(EffectDslColor),
}

impl EffectDslType {
    pub fn curve(value_type: Self) -> Self {
        Self::Curve(Box::new(value_type))
    }

    pub fn array(item_type: Self) -> Self {
        Self::Array(Box::new(item_type))
    }
}

fn is_identifier_start(candidate: char) -> bool {
    candidate == '_' || candidate.is_ascii_alphabetic()
}

fn is_identifier_continue(candidate: char) -> bool {
    candidate == '_' || candidate.is_ascii_alphanumeric()
}
