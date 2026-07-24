use crate::{
    Symbol,
    collect::{CtxtRef, TypeDefKind},
    def_ids::DefId,
    index_vec::IndexVec,
    types::{CaseId, Type},
};
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Constructor {
    Bool(bool),
    Int(i128),
    Wildcard,
    Record,
    Case(Symbol),
    NonExhaustive,
    Missing,
}

pub enum ConstructorSet {
    Bool,
    Never,
    NonExhaustive,
    Record,
    Cases(IndexVec<CaseId, Symbol>),
}
pub fn constructors_of_ty(from: DefId, ctxt: CtxtRef<'_>, ty: &Type) -> ConstructorSet {
    match ty {
        Type::Bool => ConstructorSet::Bool,
        Type::Never => ConstructorSet::Never,
        Type::Char
        | Type::Unknown
        | Type::Param(..)
        | Type::Int(_)
        | Type::Function(..)
        | Type::Byte
        | Type::Array(_)
        | Type::String => ConstructorSet::NonExhaustive,
        Type::Record(_) | Type::Tuple(_) => ConstructorSet::Record,
        Type::Infer(_) => unreachable!("Cannot have infer here"),
        Type::Named(id, _, args) => {
            if !ctxt.same_module(*id, from) && ctxt.is_opaque(*id) {
                return ConstructorSet::NonExhaustive;
            }
            match ctxt.type_def(*id).kind {
                TypeDefKind::Record(ref fields) => {
                    if fields
                        .iter()
                        .any(|field| field.type_of(args, ctxt).is_uninhabited(ctxt))
                    {
                        return ConstructorSet::Never;
                    }
                    ConstructorSet::Record
                }
                TypeDefKind::Variant(ref cases) => ConstructorSet::Cases(
                    cases
                        .iter()
                        .filter_map(|case| {
                            if let Some(field) = case.field
                                && field.type_of(args, ctxt).is_uninhabited(ctxt)
                            {
                                None
                            } else {
                                Some(case.name)
                            }
                        })
                        .collect(),
                ),
            }
        }
    }
}

pub fn fields_of(ty: &Type, constructor: Constructor, ctxt: CtxtRef<'_>) -> Vec<Type> {
    match constructor {
        Constructor::Int(_)
        | Constructor::Bool(_)
        | Constructor::NonExhaustive
        | Constructor::Wildcard
        | Constructor::Missing => Vec::new(),
        Constructor::Record => match ty {
            Type::Record(fields) => fields.iter().map(|field| field.ty.clone()).collect(),
            Type::Named(id, _, args) => ctxt
                .type_def(*id)
                .fields()
                .iter()
                .map(|&field_def| field_def.type_of(args, ctxt))
                .collect(),
            Type::Tuple(fields) => fields.to_vec(),
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
