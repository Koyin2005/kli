use std::collections::{HashMap, HashSet};

use crate::{
    ast::Mutable,
    resolved_ast::{Pattern, PatternField, PatternKind, Var},
    typed_ast::{self, FieldId},
    types::{FieldName, RecordField, Region, Type},
};

use super::root::TypeCheck;

impl TypeCheck<'_> {
    pub fn check_pattern(
        &self,
        pattern: &Pattern,
        expected_type: Type,
        binding_mode: Option<(Region, Mutable)>,
    ) -> typed_ast::Pattern {
        let loc = pattern.loc;
        let expected_type = self.simplify_type(expected_type);
        match pattern.kind {
            PatternKind::Int(value) => {
                let _ = self.unify(expected_type, Type::Int, pattern.loc);
                typed_ast::Pattern {
                    ty: Type::Int,
                    loc,
                    kind: typed_ast::PatternKind::Int(value),
                }
            }
            PatternKind::Ref(ref inner) => {
                let (mutable, region, ty) =
                    if let Ok((mutable, region, ty)) = expected_type.as_reference_type() {
                        (mutable, region, ty.clone())
                    } else {
                        self.expect_ty_error("reference", &expected_type, pattern.loc);
                        (Mutable::Mutable, Region::Unknown, Type::Unknown)
                    };
                let inner = self.check_pattern(inner, ty.clone(), Some((region, mutable)));
                typed_ast::Pattern {
                    ty: Type::reference(inner.ty.clone(), mutable, region),
                    loc,
                    kind: typed_ast::PatternKind::Ref(Box::new(inner)),
                }
            }
            PatternKind::Record(ref pat_fields) => {
                let expected_fields = match self.simplify_type(expected_type) {
                    Type::Record(fields) => Some(fields),
                    ref ty => {
                        self.expect_ty_error("record", ty, pattern.loc);
                        None
                    }
                };
                let field_names = expected_fields
                    .iter()
                    .flatten()
                    .enumerate()
                    .map(|(i, field)| (field.name, i))
                    .collect::<HashMap<_, _>>();
                let mut seen_fields = HashSet::new();
                let fields = pat_fields
                    .iter()
                    .enumerate()
                    .filter_map(|(i, PatternField { name, pattern })| {
                        let field_id = field_names
                            .get(&FieldName::Named(name.symbol))
                            .copied()
                            .map(FieldId::new);
                        let pattern = self.check_pattern(
                            pattern,
                            field_id
                                .and_then(|field| {
                                    expected_fields
                                        .as_ref()
                                        .map(|fields| fields[field].ty.clone())
                                })
                                .unwrap_or(Type::Unknown),
                            binding_mode,
                        );
                        if expected_fields.is_some() && !seen_fields.insert(name.symbol) {
                            self.ctxt().diag().add_diagnostic(
                                format!("Repeated field '{}'", name.symbol),
                                name.loc,
                            );
                            return None;
                        }

                        let field_id = if let Some(field_id) = field_id {
                            field_id
                        } else if expected_fields.is_some() {
                            self.ctxt().diag().add_diagnostic(
                                format!("'record' has no field '{}'", name.symbol),
                                name.loc,
                            );
                            return None;
                        } else {
                            FieldId::new(i)
                        };
                        Some(typed_ast::PatternField {
                            pattern,
                            index: field_id,
                        })
                    })
                    .collect::<Vec<_>>();
                let record_fields = if let Some(fields) = expected_fields {
                    let _ = self.check_missing_fields(
                        pattern.loc,
                        seen_fields,
                        fields.iter().map(|field| field.name),
                    );
                    fields
                } else {
                    fields
                        .iter()
                        .zip(pat_fields)
                        .map(|(field, pat_field)| RecordField {
                            name: FieldName::Named(pat_field.name.symbol),
                            ty: field.pattern.ty.clone(),
                        })
                        .collect()
                };
                let ty = Type::Record(record_fields);
                typed_ast::Pattern {
                    ty,
                    loc,
                    kind: typed_ast::PatternKind::Record(fields),
                }
            }
            PatternKind::Bool(value) => {
                self.unify(expected_type, Type::Bool, pattern.loc);
                typed_ast::Pattern {
                    loc,
                    ty: Type::Bool,
                    kind: typed_ast::PatternKind::Bool(value),
                }
            }
            PatternKind::Binding(borrow, mutable, ref ident, var) => {
                let name = ident.symbol;

                let (borrow, var_ty) = match (borrow, binding_mode) {
                    (None, _) => (None, expected_type.clone()),
                    (Some(_), None) => {
                        self.ctxt().diag().add_diagnostic(
                            format!("Cannot create borrow binding '{}'", ident.symbol),
                            ident.loc,
                        );
                        (None, expected_type.clone())
                    }
                    (Some(borrow), Some((region, mutable))) => {
                        if !mutable.usable_as(borrow) {
                            self.ctxt().diag().add_diagnostic(
                                format!("Cannot create borrow binding '{}'", ident.symbol),
                                ident.loc,
                            );
                        }
                        (
                            Some((mutable, region)),
                            Type::reference(expected_type.clone(), borrow, region),
                        )
                    }
                };
                self.declare_var(var, var_ty.clone(), name);
                typed_ast::Pattern {
                    ty: expected_type,
                    loc,
                    kind: typed_ast::PatternKind::Binding(
                        borrow,
                        mutable,
                        Var(name, var),
                        Box::new(var_ty),
                    ),
                }
            }
        }
    }
}
