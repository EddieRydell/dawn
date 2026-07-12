use crate::values::{Color, Curve, Gradient, Marks};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Identifier(String);

impl Identifier {
    pub fn new(value: String) -> Result<Self, IdentifierError> {
        if value.is_empty() {
            return Err(IdentifierError::Empty);
        }

        let mut chars = value.chars();
        let Some(first) = chars.next() else {
            return Err(IdentifierError::Empty);
        };

        if !is_identifier_start(first) {
            return Err(IdentifierError::InvalidStart);
        }

        if chars.any(|candidate| !is_identifier_continue(candidate)) {
            return Err(IdentifierError::InvalidCharacter);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentifierError {
    Empty,
    InvalidStart,
    InvalidCharacter,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum Type {
    Void,
    Int,
    Float,
    Bool,
    Color,
    Signal,
    Marks,
    Timeline,
    Target,
    TargetItems,
    TargetItem,
    Curve,
    Gradient,
    Array(Box<Type>),
    Enum(Vec<Identifier>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Void,
    Int(i64),
    Float(f64),
    Bool(bool),
    Color(Color),
    Marks(Arc<Marks>),
    Target(Arc<TargetValue>),
    TargetItems(Arc<TargetItemsValue>),
    TargetItem(Arc<TargetItemValue>),
    Curve(Arc<Curve>),
    Gradient(Arc<Gradient>),
    Array(Arc<Vec<Value>>),
    Enum(Identifier),
}

impl Type {
    pub fn array(item_type: Self) -> Self {
        Self::Array(Box::new(item_type))
    }

    pub fn default_value(&self) -> Value {
        match self {
            Self::Void | Self::Signal | Self::Timeline => Value::Void,
            Self::Int => Value::Int(0),
            Self::Float => Value::Float(0.0),
            Self::Bool => Value::Bool(false),
            Self::Color => Value::Color(Color {
                red: 0,
                green: 0,
                blue: 0,
            }),
            Self::Marks => Value::Marks(Arc::new(Marks { marks: Vec::new() })),
            Self::Target => Value::Target(Arc::new(TargetValue { groups: Vec::new() })),
            Self::TargetItems => {
                Value::TargetItems(Arc::new(TargetItemsValue { groups: Vec::new() }))
            }
            Self::TargetItem => Value::TargetItem(Arc::new(TargetItemValue {
                pixels: Arc::new(Vec::new()),
            })),
            Self::Curve => Value::Curve(Arc::new(Curve { points: Vec::new() })),
            Self::Gradient => Value::Gradient(Arc::new(Gradient { stops: Vec::new() })),
            Self::Array(_) => Value::Array(Arc::new(Vec::new())),
            Self::Enum(options) => Value::Enum(options[0].clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TargetValue {
    pub groups: Vec<Arc<TargetItemValue>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TargetItemsValue {
    pub groups: Vec<Arc<TargetItemValue>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TargetItemValue {
    pub pixels: Arc<Vec<TargetPixelValue>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetPixelValue {
    pub fixture_index: i64,
    pub fixture_pixel_index: i64,
    pub pixel_index: i64,
    pub pixel_count: i64,
    pub pixel_fraction: f64,
}

fn is_identifier_start(candidate: char) -> bool {
    candidate == '_' || candidate.is_ascii_alphabetic()
}

fn is_identifier_continue(candidate: char) -> bool {
    candidate == '_' || candidate.is_ascii_alphanumeric()
}
