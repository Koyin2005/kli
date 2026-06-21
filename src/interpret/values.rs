use crate::interpret::ints::Int;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pointer {
    pub address: usize,
    pub alloc: Option<usize>,
}
impl Pointer {
    pub fn aligned(self, alignment: usize) -> bool {
        self.address.is_multiple_of(alignment)
    }
}
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub enum Value {
    Pointer(Pointer),
    Int(Int),
    Char(char),
    Bool(bool),
    Tuple(Vec<Value>),
    Variant(u128, Vec<Value>),
}
impl Value {
    pub const SOME_DISCRIMINANT: u128 = 1;
    pub const NONE_DISCRIMINANT: u128 = 0;
    pub fn unit() -> Self {
        Self::Tuple(Vec::new())
    }
    pub fn pair(first: Self, second: Self) -> Self {
        Self::Tuple(vec![first, second])
    }
    pub fn into_pair(self) -> Option<(Value, Value)> {
        match self {
            Self::Tuple(values) => {
                let mut values = values.into_iter();
                let first = values.next()?;
                let second = values.next()?;
                if values.next().is_some() {
                    return None;
                }
                Some((first, second))
            }
            _ => None,
        }
    }
    pub fn as_pair(&self) -> Option<(&Value, &Value)> {
        match self {
            Self::Tuple(values) => {
                let mut values = values.iter();
                let first = values.next()?;
                let second = values.next()?;
                if values.next().is_some() {
                    return None;
                }
                Some((first, second))
            }
            _ => None,
        }
    }
    pub fn into_int(self) -> Option<Int> {
        match self {
            Self::Int(value) => Some(value),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }
    pub fn as_char(&self) -> Option<char> {
        match self {
            Self::Char(value) => Some(*value),
            _ => None,
        }
    }
    pub fn as_pointer(&self) -> Option<Pointer> {
        match self {
            Self::Pointer(pointer) => Some(*pointer),
            _ => None,
        }
    }
    pub fn into_tuple(self) -> Option<Vec<Value>> {
        match self {
            Self::Tuple(values) => Some(values),
            _ => None,
        }
    }
    pub fn as_tuple(&self) -> Option<&[Value]> {
        match self {
            Self::Tuple(values) => Some(values),
            _ => None,
        }
    }
    pub fn into_variant(self) -> Option<(u128, Vec<Value>)> {
        match self {
            Self::Variant(discriminant, values) => Some((discriminant, values)),
            _ => None,
        }
    }
    pub fn is_unit(&self) -> bool {
        matches!(self,Self::Tuple(values) if values.is_empty())
    }
    pub fn into_string(self) -> Option<StringValue> {
        let fields = self.into_tuple()?;
        let [ptr, cap, len] = fields.try_into().ok()?;
        let ptr = ptr.as_pointer()?;
        let cap = cap.into_int()?;
        let len = len.into_int()?;
        Some(StringValue {
            pointer: ptr,
            cap,
            len,
        })
    }
    pub fn into_option(self) -> Option<Option<Value>> {
        let (discr, fields) = self.into_variant()?;
        if discr == Self::NONE_DISCRIMINANT {
            let [] = fields.as_array()?;
            Some(None)
        } else if discr == Self::SOME_DISCRIMINANT {
            let mut fields = fields.into_iter();
            let field = fields.next()?;
            if fields.next().is_some() {
                return None;
            }
            Some(Some(field))
        } else {
            None
        }
    }
    pub fn from_string(value: StringValue) -> Self {
        Self::Tuple(vec![
            Value::Pointer(value.pointer),
            Value::Int(value.cap),
            Value::Int(value.len),
        ])
    }
}

pub struct StringValue {
    pub pointer: Pointer,
    pub cap: Int,
    pub len: Int,
}
