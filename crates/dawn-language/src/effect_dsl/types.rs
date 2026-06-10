use crate::values::{Color, Curve, Marks};

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
    Marks,
    Curve(Box<Type>),
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
    Marks(Marks),
    Curve(Curve),
    Array(Vec<Value>),
    Enum(Identifier),
}

impl Type {
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
