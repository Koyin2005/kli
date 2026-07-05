use crate::{
    Symbol,
    collect::{CtxtRef, TypeDefKind},
    types::Type,
};
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Constructor {
    Bool(bool),
    Int(i64),
    Wildcard,
    Record,
    Ref,
    Case(Symbol),
    Unit,
    NonExhaustive,
    Missing,
}

pub fn constructors_of_ty(ctxt: CtxtRef<'_>, ty: &Type) -> Vec<Constructor> {
    match ty {
        Type::Bool => vec![Constructor::Bool(true), Constructor::Bool(false)],
        Type::Imm(..) | Type::Mut(..) => vec![Constructor::Ref],

        Type::Unit => vec![Constructor::Unit],
        Type::Char
        | Type::Box(_)
        | Type::String
        | Type::Unknown
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
        Type::Named(id, ..) => match ctxt.type_def(*id).kind {
            TypeDefKind::Record(_) => {
                vec![Constructor::Record]
            }
            TypeDefKind::Variant(ref cases) => cases
                .iter()
                .map(|case| case.name)
                .map(Constructor::Case)
                .collect(),
        },
    }
}

pub fn fields_of(ty: &Type, constructor: Constructor, ctxt: CtxtRef<'_>) -> Vec<Type> {
    match constructor {
        Constructor::Int(_)
        | Constructor::Bool(_)
        | Constructor::NonExhaustive
        | Constructor::Wildcard
        | Constructor::Missing
        | Constructor::Unit => Vec::new(),
        Constructor::Ref => {
            let (Type::Imm(_, ty) | Type::Mut(_, ty)) = ty else {
                unreachable!("Should be a view")
            };
            vec![(**ty).clone()]
        }
        Constructor::Record => match ty {
            Type::Record(fields) => fields.iter().map(|field| field.ty.clone()).collect(),
            Type::Named(id, _, args) => ctxt
                .type_def(*id)
                .fields()
                .iter()
                .map(|&field_def| field_def.type_of(args, ctxt))
                .collect(),
            _ => unreachable!("should be a record type"),
        },
        Constructor::Case(name) => {
            let Type::Named(ty_id, .., args) = ty else {
                unreachable!("should be named")
            };
            match ctxt.type_def(*ty_id).kind {
                TypeDefKind::Record(fields) => fields
                    .iter()
                    .map(|&field| field.type_of(args, ctxt))
                    .collect(),
                TypeDefKind::Variant(cases) => {
                    let &case = cases
                        .iter()
                        .find(|&&case| case.name == name)
                        .expect("should have this case");
                    case.field
                        .map(|field| field.type_of(args, ctxt))
                        .into_iter()
                        .collect()
                }
            }
        }
    }
}
