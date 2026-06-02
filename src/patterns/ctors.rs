use crate::types::Type;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Constructor {
    Some,
    None,
    Bool(bool),
    Wildcard,
    Record,
    NonExhaustive,
}

pub fn constructors_of_ty(ty: &Type) -> Vec<Constructor> {
    match ty {
        Type::Bool => vec![Constructor::Bool(true), Constructor::Bool(false)],
        Type::Imm(.., ty) | Type::Mut(.., ty) => constructors_of_ty(ty),
        Type::Option(_) => vec![Constructor::Some, Constructor::None],
        Type::Char
        | Type::Box(_)
        | Type::String
        | Type::Unknown
        | Type::Unit
        | Type::Param(..)
        | Type::Int
        | Type::List(_)
        | Type::Function(..) => vec![Constructor::NonExhaustive],
        Type::Record(_) => {
            vec![Constructor::Record]
        }
        Type::Infer(_) => unreachable!("Cannot have infer here"),
    }
}

pub fn fields_of(ty: &Type, constructor: Constructor) -> Vec<&Type> {
    if let Type::Imm(_, ty) | Type::Mut(_, ty) = ty {
        return fields_of(ty, constructor);
    }
    match constructor {
        Constructor::Bool(_)
        | Constructor::None
        | Constructor::NonExhaustive
        | Constructor::Wildcard => Vec::new(),
        Constructor::Some => {
            let Type::Option(ty) = ty else {
                unreachable!("Should be an option")
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
