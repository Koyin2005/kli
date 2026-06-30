use crate::types::Type;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Constructor {
    Bool(bool),
    Int(i64),
    Wildcard,
    Record,
    Ref,
    NonExhaustive,
}

pub fn constructors_of_ty(ty: &Type) -> Vec<Constructor> {
    match ty {
        Type::Bool => vec![Constructor::Bool(true), Constructor::Bool(false)],
        Type::Imm(..) | Type::Mut(..) => vec![Constructor::Ref],
        Type::Char
        | Type::Box(_)
        | Type::String
        | Type::Unknown
        | Type::Unit
        | Type::Param(..)
        | Type::Int
        | Type::List(_)
        | Type::Function(..)
        | Type::RawPointer(..)
        | Type::Byte
        | Type::Array(..) => vec![Constructor::NonExhaustive],
        Type::Record(_) => {
            vec![Constructor::Record]
        }
        Type::Infer(_) => unreachable!("Cannot have infer here"),
        Type::Named(..) => {
            //TODO : Allow matching the actual constructors
            vec![Constructor::NonExhaustive]
        }
    }
}

pub fn fields_of(ty: &Type, constructor: Constructor) -> Vec<&Type> {
    match constructor {
        Constructor::Int(_)
        | Constructor::Bool(_)
        | Constructor::NonExhaustive
        | Constructor::Wildcard => Vec::new(),
        Constructor::Ref => {
            let (Type::Imm(_, ty) | Type::Mut(_, ty)) = ty else {
                unreachable!("Should be a view")
            };
            vec![ty]
        }
        Constructor::Record => {
            let Type::Record(fields) = ty else {
                unreachable!("Should be a record")
            };
            fields.iter().map(|field| &field.ty).collect()
        }
    }
}
