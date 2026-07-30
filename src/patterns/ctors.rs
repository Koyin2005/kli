use crate::{
    Symbol,
    collect::{CtxtRef, TypeDefKind},
    def_ids::DefId,
    index_vec::IndexVec,
    types::{CaseId, TypeKind},
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
pub fn constructors_of_ty(from: DefId, ctxt: CtxtRef<'_>, ty: &TypeKind) -> ConstructorSet {
    match ty {
        TypeKind::Bool => ConstructorSet::Bool,
        TypeKind::Never => ConstructorSet::Never,
        TypeKind::Char
        | TypeKind::Unknown
        | TypeKind::Param(..)
        | TypeKind::Int(_)
        | TypeKind::Function(..)
        | TypeKind::Byte
        | TypeKind::Array(_)
        | TypeKind::String
        | TypeKind::Box(_) => ConstructorSet::NonExhaustive,
        TypeKind::Record(_) | TypeKind::Tuple(_) => ConstructorSet::Record,
        TypeKind::Infer(_) => unreachable!("Cannot have infer here"),
        TypeKind::Named(id, _, args) => {
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

pub fn fields_of(ty: &TypeKind, constructor: Constructor, ctxt: CtxtRef<'_>) -> Vec<TypeKind> {
    match constructor {
        Constructor::Int(_)
        | Constructor::Bool(_)
        | Constructor::NonExhaustive
        | Constructor::Wildcard
        | Constructor::Missing => Vec::new(),
        Constructor::Record => match ty {
            TypeKind::Record(fields) => fields.iter().map(|field| field.ty.clone()).collect(),
            TypeKind::Named(id, _, args) => ctxt
                .type_def(*id)
                .fields()
                .iter()
                .map(|&field_def| field_def.type_of(args, ctxt))
                .collect(),
            TypeKind::Tuple(fields) => fields.to_vec(),
            _ => unreachable!("should be a record type"),
        },
        Constructor::Case(name) => {
            let TypeKind::Named(ty_id, .., args) = ty else {
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
