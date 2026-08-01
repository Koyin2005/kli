use std::collections::{HashMap, HashSet};

use crate::{
    collect::TypeDefKind, index_vec::IndexVec, resolved_ast::{Pattern, PatternField, PatternKind, Var}, typecheck::root::FunctionCtxt, typed_ast::{self, FieldId}, types::{self, FieldName, RecordField, Type, TypeKind},
};
impl<'ctxt> FunctionCtxt<'_, 'ctxt> {
    pub fn check_pattern(
        &self,
        pattern: &Pattern,
        expected_type: Type<'ctxt>,
    ) -> typed_ast::Pattern<'ctxt> {
        let loc = pattern.loc;
        let root = self.root();
        let expected_type = root.simplify_type(expected_type);
        match pattern.kind {
            PatternKind::Int(value) => {
                let (ty, value) = self.root().check_int_lit(loc, Some(expected_type), value);
                let _ = root.unify(expected_type, ty, pattern.loc);
                typed_ast::Pattern {
                    ty,
                    loc,
                    kind: typed_ast::PatternKind::Int(value),
                }
            }
            PatternKind::Unit => {
                let _ = root.unify(expected_type, Type::new_unit(self.ctxt()), pattern.loc);
                typed_ast::Pattern {
                    ty: Type::new_unit(self.ctxt()),
                    loc,
                    kind: typed_ast::PatternKind::Unit,
                }
            }
            PatternKind::Tuple(ref fields) => {
                let expected_fields = match expected_type.kind() {
                    TypeKind::Tuple(field_tys) => &**field_tys,
                    _ => {
                        self.root().expect_ty_error("tuple", expected_type, loc);
                        &[]
                    }
                };
                if expected_fields.len() != fields.len() {
                    self.ctxt().diag().add_diagnostic(
                        format!(
                            "Expected '{}' fields but got '{}'",
                            expected_fields.len(),
                            fields.len()
                        ),
                        pattern.loc,
                    );
                }
                let fields = fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| {
                        let ty = expected_fields
                            .get(i)
                            .cloned()
                            .unwrap_or(Type::new_unknown(self.ctxt()));
                        typed_ast::PatternField {
                            index: FieldId::new(i),
                            pattern: self.check_pattern(field, ty),
                        }
                    })
                    .collect();
                typed_ast::Pattern {
                    ty: Type::tuple_from_iter(self.ctxt(), expected_fields.iter().copied()),
                    loc,
                    kind: typed_ast::PatternKind::Record(fields),
                }
            }
            PatternKind::Case(name, ref inner) => {
                let (id, ty_name, args) = match expected_type.kind() {
                    &TypeKind::Named(id, ty_name, ref args) => (id, ty_name, args),
                    _ => {
                        root.expect_ty_error("variant type", expected_type, loc);
                        if let Some(inner) = inner {
                            let _ = self.check_pattern(inner, Type::new_unknown(self.ctxt()));
                        }
                        return typed_ast::Pattern {
                            ty: expected_type,
                            loc,
                            kind: typed_ast::PatternKind::Err,
                        };
                    }
                };
                let ctxt = root.ctxt();
                let type_def = ctxt.type_def(id);
                let cases = match type_def.kind {
                    TypeDefKind::Variant(ref variant_def) => variant_def,
                    _ => {
                        root.ctxt()
                            .diag()
                            .add_diagnostic("expected 'variant' type but got 'record'", loc);
                        if let Some(inner) = inner {
                            let _ = self.check_pattern(inner, Type::new_unknown(self.ctxt()));
                        }
                        return typed_ast::Pattern {
                            ty: Type::named(self.ctxt(), id, ty_name, args.iter().copied()),
                            loc,
                            kind: typed_ast::PatternKind::Err,
                        };
                    }
                };
                let Some((i, &case_def)) = cases
                    .iter_enumerated()
                    .find(|(_, case_def)| case_def.name == name.symbol)
                else {
                    root.ctxt().diag().add_diagnostic(
                        format!("'{}' has no case '{}'", ty_name, name.symbol),
                        name.loc,
                    );
                    if let Some(inner) = inner {
                        let _ = self.check_pattern(inner, Type::new_unknown(self.ctxt()));
                    }
                    return typed_ast::Pattern {
                        ty: Type::named(self.ctxt(), id, ty_name, args.iter().copied()),
                        loc,
                        kind: typed_ast::PatternKind::Err,
                    };
                };
                let case_id = case_def.id;
                let inner = match (
                    case_def.field.map(|field| field.type_of(args, ctxt)),
                    inner,
                ) {
                    (None, None) => None,
                    (Some(inner_ty), Some(inner)) => {
                        Some(Box::new(self.check_pattern(inner, inner_ty)))
                    }
                    (None, Some(inner)) => {
                        root.ctxt().diag().add_diagnostic(
                            format!("'{}' has no inner fields", name.symbol),
                            name.loc,
                        );
                        Some(Box::new(
                            self.check_pattern(inner, Type::new_unknown(self.ctxt())),
                        ))
                    }
                    (Some(ty), None) => {
                        root.ctxt().diag().add_diagnostic(
                            format!("'{}' has inner fields", name.symbol),
                            name.loc,
                        );
                        Some(Box::new(typed_ast::Pattern {
                            ty,
                            loc,
                            kind: typed_ast::PatternKind::Err,
                        }))
                    }
                };
                typed_ast::Pattern {
                    ty: Type::named(self.ctxt(), id, ty_name, args.iter().copied()),
                    loc,
                    kind: typed_ast::PatternKind::Case(case_id, args.clone(), i, inner),
                }
            }
            PatternKind::Record(ref pat_fields) => {
                let expected_type = root.simplify_type(expected_type);
                let (ty, expected_fields):(_,Option<IndexVec<FieldId,RecordField>>) = match expected_type.kind() {
                    &TypeKind::Named(id, _, ref args)
                        if let TypeDefKind::Record(fields) = self.ctxt().type_def(id).kind =>
                    {
                        let fields = fields
                            .into_iter()
                            .map(|field| types::RecordField {
                                name: FieldName::Named(field.name),
                                ty: field.type_of(args, self.ctxt()),
                            })
                            .collect();
                        (expected_type, Some(fields))
                    }
                    _ => {
                        root.expect_ty_error("record", expected_type, pattern.loc);
                        (Type::new_unknown(self.ctxt()), None)
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
                                        .map(|fields| fields[field].ty)
                                })
                                .unwrap_or(Type::new_unknown(self.ctxt())),
                        );
                        if expected_fields.is_some() && !seen_fields.insert(name.symbol) {
                            root.ctxt().diag().add_diagnostic(
                                format!("Repeated field '{}'", name.symbol),
                                name.loc,
                            );
                            return None;
                        }

                        let field_id = if let Some(field_id) = field_id {
                            field_id
                        } else if expected_fields.is_some() {
                            root.ctxt().diag().add_diagnostic(
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

                let _ = self.root().check_missing_fields(
                    pattern.loc,
                    seen_fields,
                    expected_fields.iter().flatten().map(|field| field.name),
                );
                if let Some((id, _, _)) = ty.as_named()
                    && let type_def = self.ctxt().type_def(id)
                {
                    let ty_fields = type_def.fields();
                    for field in &fields {
                        let _ = self
                            .check_field_visibility(ty_fields[field.index].id, field.pattern.loc);
                    }
                }
                typed_ast::Pattern {
                    ty,
                    loc,
                    kind: typed_ast::PatternKind::Record(fields),
                }
            }
            PatternKind::Bool(value) => {
                let bool = Type::new_bool(self.ctxt());
                root.unify(expected_type, bool, pattern.loc);
                typed_ast::Pattern {
                    loc,
                    ty: bool,
                    kind: typed_ast::PatternKind::Bool(value),
                }
            }
            PatternKind::Binding(mutable, ref ident, var) => {
                let name = ident.symbol;

                let var_ty = expected_type;
                root.declare_var(var, var_ty, name);
                typed_ast::Pattern {
                    ty: expected_type,
                    loc,
                    kind: typed_ast::PatternKind::Binding(mutable, Var(name, var), var_ty),
                }
            }
        }
    }
}
