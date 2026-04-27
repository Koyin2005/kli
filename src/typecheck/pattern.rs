use crate::{
    ast::Mutable,
    resolved_ast::{Pattern, PatternKind, Var},
    typed_ast,
    types::{Region, Type},
};

use super::root::TypeCheck;

impl TypeCheck {
    pub fn check_pattern(
        &mut self,
        pattern: Pattern,
        expected_type: Type,
        region: Option<Region>,
    ) -> typed_ast::Pattern {
        let expected_type = self.simplify_type(expected_type);
        match pattern.kind {
            PatternKind::Deref(derefed_pattern) => {
                let (derefed_pattern, mutable, region) = match expected_type.as_reference_type() {
                    Ok((mutable, expected_region, ty)) => {
                        let region = match region {
                            Some(region) => {
                                self.unify_region(region, expected_region, pattern.line)
                            }
                            None => expected_region,
                        };
                        (
                            self.check_pattern(*derefed_pattern, ty, Some(region.clone())),
                            mutable,
                            region,
                        )
                    }
                    Err(ty) => {
                        self.diag.borrow_mut().report(
                            format!("Expected a reference type '{}' but got", ty),
                            pattern.line,
                        );
                        (
                            self.check_pattern(*derefed_pattern, Type::Unknown, region),
                            Mutable::Immutable,
                            Region::Unknown,
                        )
                    }
                };
                typed_ast::Pattern {
                    ty: Type::reference(derefed_pattern.ty.clone(), mutable, region),
                    line: pattern.line,
                    kind: typed_ast::PatternKind::Deref(Box::new(derefed_pattern)),
                }
            }
            PatternKind::None => {
                let inner_ty = match expected_type {
                    Type::Option(ty) => *ty,
                    expected_type => {
                        self.diag.borrow_mut().report(
                            format!("Expected an option type but got '{}'", expected_type),
                            pattern.line,
                        );
                        Type::Unknown
                    }
                };
                typed_ast::Pattern {
                    ty: Type::Option(Box::new(inner_ty)),
                    line: pattern.line,
                    kind: typed_ast::PatternKind::None,
                }
            }
            PatternKind::Some(inner) => {
                let inner = match expected_type {
                    Type::Option(ty) => self.check_pattern(*inner, *ty, region),
                    expected_type => {
                        self.diag.borrow_mut().report(
                            format!("Expected an option type but got '{}'", expected_type),
                            pattern.line,
                        );
                        self.check_pattern(*inner, Type::Unknown, region)
                    }
                };
                typed_ast::Pattern {
                    ty: Type::Option(Box::new(inner.ty.clone())),
                    line: pattern.line,
                    kind: typed_ast::PatternKind::Some(Box::new(inner)),
                }
            }
            PatternKind::Binding(mutable, ident, var, borrow_region) => {
                let borrow_region = borrow_region.map(|region| self.lower_region(region));
                let name = ident.content.clone();
                let var_ty = match (borrow_region, region) {
                    (None, None) => expected_type.clone(),
                    (None, Some(_)) => expected_type.clone(),
                    (Some(region), None) => {
                        self.diag
                            .borrow_mut()
                            .report("Cant borrow without region".to_string(), pattern.line);
                        Type::reference(expected_type.clone(), mutable, region)
                    }
                    (Some(borrow_region), Some(expected)) => {
                        let region = self.unify_region(borrow_region, expected, pattern.line);
                        Type::reference(expected_type.clone(), mutable, region)
                    }
                };
                self.declare_var(var, var_ty.clone());
                typed_ast::Pattern {
                    ty: expected_type,
                    line: pattern.line,
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
