use crate::{Symbol, collect::CtxtRef, types::Type};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Constructor {
    Bool(bool),
    Int(i64),
    Wildcard,
    Record,
    Ref,
    Case(Symbol),
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

pub fn fields_of(ty: &Type, constructor: Constructor, ctxt: CtxtRef<'_>) -> Vec<Type> {
    match constructor {
        Constructor::Int(_)
        | Constructor::Bool(_)
        | Constructor::NonExhaustive
        | Constructor::Wildcard => Vec::new(),
        Constructor::Ref => {
            let (Type::Imm(_, ty) | Type::Mut(_, ty)) = ty else {
                unreachable!("Should be a view")
            };
            vec![(**ty).clone()]
        }
        Constructor::Record => {
            let Type::Record(fields) = ty else {
                unreachable!("Should be a record")
            };
            fields.iter().map(|field| field.ty.clone()).collect()
        }
        Constructor::Case(name) => {
            let Type::Named(ty_id, .., args) = ty else {
                unreachable!("should be named")
            };
            let variant = ctxt.expect_type(*ty_id).expect_variant();
            let case = variant
                .cases
                .iter()
                .find(|case| case.name.symbol == name)
                .expect("should have this case");
            case.ty
                .as_ref()
                .map(|ty| ctxt.type_of(ty.id).bind(args))
                .into_iter()
                .collect()
        }
    }
}
