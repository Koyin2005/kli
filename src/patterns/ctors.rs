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

pub fn constructors_of_ty(ctxt: CtxtRef<'_>, ty: &Type) -> Vec<Constructor> {
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
        Type::Named(id, ..) => match ctxt.expect_type(*id).kind {
            crate::resolved_ast::TypeDefKind::Record(_) => vec![Constructor::Record],
            crate::resolved_ast::TypeDefKind::Variant(ref variant_def) => variant_def
                .cases
                .iter()
                .map(|case| case.name.symbol)
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
        | Constructor::Wildcard => Vec::new(),
        Constructor::Ref => {
            let (Type::Imm(_, ty) | Type::Mut(_, ty)) = ty else {
                unreachable!("Should be a view")
            };
            vec![(**ty).clone()]
        }
        Constructor::Record => match ty {
            Type::Record(fields) => fields.iter().map(|field| field.ty.clone()).collect(),
            Type::Named(id, _, args) => ctxt
                .expect_type(*id)
                .expect_record()
                .fields
                .iter()
                .map(|field_def| ctxt.type_of(field_def.id).bind(args))
                .collect(),
            _ => unreachable!("should be a record type"),
        },
        Constructor::Case(name) => {
            let Type::Named(ty_id, .., args) = ty else {
                unreachable!("should be named")
            };
            match ctxt.expect_type(*ty_id).kind {
                crate::resolved_ast::TypeDefKind::Record(ref record) => record
                    .fields
                    .iter()
                    .map(|field| ctxt.type_of(field.id).bind(args))
                    .collect(),
                crate::resolved_ast::TypeDefKind::Variant(ref variant) => {
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
    }
}
