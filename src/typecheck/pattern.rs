use std::collections::{HashMap, HashSet};

use crate::{
    ast::Mutable,
    collect::TypeDefKind,
    resolved_ast::{Pattern, PatternField, PatternKind, Var},
    typecheck::root::FunctionCtxt,
    typed_ast::{self, FieldId},
    types::{self, FieldName, Region, Type},
};
impl FunctionCtxt<'_> {
    pub fn check_pattern(
        &self,
        pattern: &Pattern,
        expected_type: Type,
        binding_mode: Option<(Region, Mutable)>,
    ) -> typed_ast::Pattern {
        let loc = pattern.loc;
        let root = self.root();
        let expected_type = root.simplify_type(expected_type);
        match pattern.kind {
            PatternKind::Int(value) => {
                let (ty, value) = self.root().check_int_lit(loc, Some(&expected_type), value);
                let _ = root.unify(expected_type, ty.clone(), pattern.loc);
                typed_ast::Pattern {
                    ty,
                    loc,
                    kind: typed_ast::PatternKind::Int(value),
                }
            }
            PatternKind::Unit => {
                let _ = root.unify(expected_type, Type::UNIT, pattern.loc);
                typed_ast::Pattern {
                    ty: Type::UNIT,
                    loc,
                    kind: typed_ast::PatternKind::Unit,
                }
            }
            PatternKind::Tuple(ref fields) => {
                let expected_fields = match expected_type {
                    Type::Tuple(field_tys) => field_tys,
                    _ => Vec::new(),
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
                        let ty = expected_fields.get(i).cloned().unwrap_or(Type::Unknown);
                        typed_ast::PatternField {
                            index: FieldId::new(i),
                            pattern: self.check_pattern(field, ty, binding_mode),
                        }
                    })
                    .collect();
                typed_ast::Pattern {
                    ty: Type::Tuple(expected_fields),
                    loc,
                    kind: typed_ast::PatternKind::Record(fields),
                }
            }
            PatternKind::Case(name, ref inner) => {
                let (id, ty_name, args) = match expected_type {
                    Type::Named(id, ty_name, args) => (id, ty_name, args),
                    ty => {
                        root.expect_ty_error("variant type", &ty, loc);
                        if let Some(inner) = inner {
                            let _ = self.check_pattern(inner, Type::Unknown, binding_mode);
                        }
                        return typed_ast::Pattern {
                            ty,
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
                            let _ = self.check_pattern(inner, Type::Unknown, binding_mode);
                        }
                        return typed_ast::Pattern {
                            ty: Type::Named(id, ty_name, args),
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
                        let _ = self.check_pattern(inner, Type::Unknown, binding_mode);
                    }
                    return typed_ast::Pattern {
                        ty: Type::Named(id, ty_name, args),
                        loc,
                        kind: typed_ast::PatternKind::Err,
                    };
                };
                let case_id = case_def.id;
                let inner = match (
                    case_def.field.map(|field| field.type_of(&args, ctxt)),
                    inner,
                ) {
                    (None, None) => None,
                    (Some(inner_ty), Some(inner)) => {
                        Some(Box::new(self.check_pattern(inner, inner_ty, binding_mode)))
                    }
                    (None, Some(inner)) => {
                        root.ctxt().diag().add_diagnostic(
                            format!("'{}' has no inner fields", name.symbol),
                            name.loc,
                        );
                        Some(Box::new(self.check_pattern(
                            inner,
                            Type::Unknown,
                            binding_mode,
                        )))
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
                    ty: Type::Named(id, ty_name, args.clone()),
                    loc,
                    kind: typed_ast::PatternKind::Case(case_id, args, i, inner),
                }
            }
            PatternKind::Record(ref pat_fields) => {
                let (ty, expected_fields) = match root.simplify_type(expected_type) {
                    Type::Record(fields) => (Type::Record(fields.clone()), Some(fields)),
                    Type::Named(id, name, args)
                        if let TypeDefKind::Record(fields) = self.ctxt().type_def(id).kind =>
                    {
                        let fields = fields
                            .into_iter()
                            .map(|field| types::RecordField {
                                name: FieldName::Named(field.name),
                                ty: field.type_of(&args, self.ctxt()),
                            })
                            .collect();
                        (Type::Named(id, name, args), Some(fields))
                    }
                    ref ty => {
                        root.expect_ty_error("record", ty, pattern.loc);
                        (Type::Unknown, None)
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
                if let Type::Named(id, _, _) = ty
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
                root.unify(expected_type, Type::Bool, pattern.loc);
                typed_ast::Pattern {
                    loc,
                    ty: Type::Bool,
                    kind: typed_ast::PatternKind::Bool(value),
                }
            }
            PatternKind::Binding(mutable, ref ident, var) => {
                let name = ident.symbol;

                let var_ty = expected_type.clone();
                root.declare_var(var, var_ty.clone(), name);
                typed_ast::Pattern {
                    ty: expected_type,
                    loc,
                    kind: typed_ast::PatternKind::Binding(
                        mutable,
                        Var(name, var),
                        Box::new(var_ty),
                    ),
                }
            }
        }
    }
}
