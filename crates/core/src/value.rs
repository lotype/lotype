use loro::LoroValue;

/// A scalar stored in block attributes and mark values.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Value {
    /// On or off.
    Bool(bool),
    /// A whole number.
    Int(i64),
    /// Text.
    Str(String),
}

impl Value {
    /// Non-scalar values written by other peers are ignored rather than rejected.
    pub(crate) fn from_loro(value: &LoroValue) -> Option<Self> {
        match value {
            LoroValue::Bool(b) => Some(Self::Bool(*b)),
            LoroValue::I64(i) => Some(Self::Int(*i)),
            LoroValue::String(s) => Some(Self::Str(s.to_string())),
            _ => None,
        }
    }

    pub(crate) fn to_loro(&self) -> LoroValue {
        match self {
            Self::Bool(b) => LoroValue::from(*b),
            Self::Int(i) => LoroValue::from(*i),
            Self::Str(s) => LoroValue::from(s.as_str()),
        }
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Self::Str(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_loro() {
        for value in [Value::from(true), Value::from(7), Value::from("x")] {
            assert_eq!(Value::from_loro(&value.to_loro()), Some(value));
        }
    }

    #[test]
    fn ignores_non_scalar_loro_values() {
        assert_eq!(Value::from_loro(&LoroValue::Null), None);
        assert_eq!(Value::from_loro(&LoroValue::Double(1.5)), None);
    }
}
