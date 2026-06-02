use std::collections::{HashMap, HashSet};

use crate::{
    ast::Mutable,
    resolved_ast::{Pattern, PatternField, PatternKind, Var},
    typed_ast::{self, FieldId},
    types::{RecordField, Region, Type},
};

use super::root::TypeCheck;

impl TypeCheck {
    pub fn check_pattern(
        &mut self,
        pattern: Pattern,
        expected_type: Type,
        binding_mode: Option<(Region, Mutable)>,
    ) -> typed_ast::Pattern {
        let expected_type = self.simplify_type(expected_type);
        if let Ok((mutable, region, ty)) = expected_type.as_reference_type() {
            let region = region.clone();
            let ty = ty.clone();
            return self.check_pattern(pattern, ty, Some((region, mutable)));
        }
        match pattern.kind {
            PatternKind::Record(fields) => {
                let expected_fields = match self.simplify_type(expected_type) {
                    Type::Record(fields) => Some(fields),
                    ref ty => {
                        self.diag.borrow_mut().add_diagnostic(
                            format!("Expected 'record' type but got '{}'", ty),
                            pattern.loc.clone(),
                        );
                        None
                    }
                };
                let field_names = expected_fields
                    .iter()
                    .flatten()
                    .enumerate()
                    .map(|(i, field)| (field.name.clone(), i))
                    .collect::<HashMap<_, _>>();
                let mut seen_fields = HashSet::new();
                let fields = fields
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, PatternField { name, pattern })| {
                        let field_id = field_names.get(&name.content).copied();
                        let pattern = self.check_pattern(
                            pattern,
                            field_id
                                .and_then(|field| {
                                    expected_fields
                                        .as_ref()
                                        .map(|fields| fields[field].ty.clone())
                                })
                                .unwrap_or(Type::Unknown),
                            binding_mode.clone(),
                        );
                        if expected_fields.is_some() && !seen_fields.insert(name.content.clone()) {
                            self.diag.borrow_mut().add_diagnostic(
                                format!("Repeated field '{}'", name.content),
                                name.loc.clone(),
                            );
                            return None;
                        }

                        let field_id = if let Some(field_id) = field_id {
                            field_id
                        } else if expected_fields.is_some() {
                            self.diag.borrow_mut().add_diagnostic(
                                format!("'record' has no field '{}'", name.content),
                                name.loc,
                            );
                            return None;
                        } else {
                            i
                        };
                        Some(typed_ast::PatternField {
                            name,
                            pattern,
                            index: FieldId::new(field_id),
                        })
                    })
                    .collect::<Vec<_>>();
                let record_fields = if let Some(fields) = expected_fields {
                    let mut field_names = field_names;
                    for field in &fields {
                        if !seen_fields.contains(&field.name)
                            && field_names.remove(&field.name).is_some()
                        {
                            self.diag.borrow_mut().add_diagnostic(
                                format!("Missing field '{}'", field.name),
                                pattern.loc.clone(),
                            );
                        }
                    }
                    fields
                } else {
                    fields
                        .iter()
                        .map(|field| RecordField {
                            name: field.name.content.clone(),
                            ty: field.pattern.ty.clone(),
                        })
                        .collect()
                };
                let ty = Type::Record(record_fields);
                typed_ast::Pattern {
                    ty,
                    loc: pattern.loc,
                    kind: typed_ast::PatternKind::Record(fields),
                }
            }
            PatternKind::Bool(value) => {
                self.unify(expected_type, Type::Bool, pattern.loc.clone());
                typed_ast::Pattern {
                    loc: pattern.loc,
                    ty: Type::Bool,
                    kind: typed_ast::PatternKind::Bool(value),
                }
            }
            PatternKind::None => {
                let inner_ty = match expected_type {
                    Type::Option(ty) => *ty,
                    expected_type => {
                        self.diag.borrow_mut().add_diagnostic(
                            format!("Expected an option type but got '{}'", expected_type),
                            pattern.loc.clone(),
                        );
                        Type::Unknown
                    }
                };
                typed_ast::Pattern {
                    ty: Type::Option(Box::new(inner_ty)),
                    loc: pattern.loc,
                    kind: typed_ast::PatternKind::None,
                }
            }
            PatternKind::Some(inner) => {
                let inner = match expected_type {
                    Type::Option(ty) => self.check_pattern(*inner, *ty, binding_mode),
                    expected_type => {
                        self.diag.borrow_mut().add_diagnostic(
                            format!("Expected an option type but got '{}'", expected_type),
                            pattern.loc.clone(),
                        );
                        self.check_pattern(*inner, Type::Unknown, binding_mode)
                    }
                };
                typed_ast::Pattern {
                    ty: Type::Option(Box::new(inner.ty.clone())),
                    loc: pattern.loc,
                    kind: typed_ast::PatternKind::Some(Box::new(inner)),
                }
            }
            PatternKind::Binding(mutable, ident, var) => {
                let name = ident.content.clone();
                let var_ty = if let Some((region, mutable)) = binding_mode {
                    Type::reference(expected_type.clone(), mutable, region)
                } else {
                    expected_type.clone()
                };
                self.declare_var(var, var_ty.clone());
                typed_ast::Pattern {
                    ty: expected_type,
                    loc: pattern.loc,
                    kind: typed_ast::PatternKind::Binding(
                        mutable,
                        Var(name.clone(), var),
                        Box::new(var_ty),
                    ),
                }
            }
        }
    }
}
