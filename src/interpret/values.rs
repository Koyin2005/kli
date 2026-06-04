#[derive(Debug,PartialEq, Eq,Hash,Clone)]
pub enum Value {
    Int(i128),
    Bool(bool),
    Tuple(Vec<Value>)
}
impl Value{
    pub fn as_int(&self) -> Option<i128>{
        match self {
            Self::Int(value) => Some(*value),
            _ => None
        }
    }
    pub fn as_bool(&self) -> Option<bool>{
        match self {
            Self::Bool(value) => Some(*value),
            _ => None
        }
    }
    pub fn as_tuple(&self) -> Option<&[Value]>{
        match self {
            Self::Tuple(values) => Some(values),
            _ => None
        }
    }
}