use crate::types::Type;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Constructor {
    Some,
    None,
    Bool(bool),
    Wildcard,
    Deref,
    NonExhaustive,
}

pub fn constructors_of_ty(ty: &Type) -> Vec<Constructor> {
    match ty {
        Type::Bool => vec![Constructor::Bool(true), Constructor::Bool(false)],
        Type::Imm(..) | Type::Mut(..) => vec![Constructor::Deref],
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
        Type::Infer(_) => unreachable!("Cannot have infer here"),
    }
}

pub fn fields_of(ty: &Type, constructor: Constructor) -> &[Type] {
    match constructor {
        Constructor::Bool(_)
        | Constructor::None
        | Constructor::NonExhaustive
        | Constructor::Wildcard => &[],
        Constructor::Some => {
            let Type::Option(ty) = ty else {
                unreachable!("Should be an option")
            };
            std::slice::from_ref(ty)
        }
        Constructor::Deref => {
            let (Type::Imm(_, ty) | Type::Mut(_, ty)) = ty else {
                unreachable!("Should be a type")
            };
            std::slice::from_ref(ty)
        }
    }
}
