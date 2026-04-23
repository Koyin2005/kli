use crate::{
    ast::{Expr, ExprKind, Ident, Lambda, Mutable, Pattern, Place},
    typecheck::{
        root::{Res, TypeCheck},
        types::{FunctionType, Region, Type},
    },
};

impl TypeCheck {
    fn check_place(&mut self, place: &Place, expected_ty: Option<Type>) -> Type {
        let (ty, line) = match place {
            Place::Ident(name) => (
                if let Some(res) = self.get_res(&name.content) {
                    match res {
                        Res::Var(var) => self.var_type(var).clone(),
                        Res::Function(_) => {
                            self.diag.borrow_mut().report(
                                format!("Cannot re-assign constant '{}'", name.content),
                                name.line,
                            );
                            Type::Unknown
                        }
                        _ => {
                            self.diag
                                .borrow_mut()
                                .report(format!("Cannot assign '{}'", name.content), name.line);
                            Type::Unknown
                        }
                    }
                } else {
                    self.diag
                        .borrow_mut()
                        .report(format!("'{}' not in scope", name.content), name.line);
                    Type::Unknown
                },
                name.line,
            ),
            &Place::Deref(ref place, line) => {
                let expected_ty = expected_ty.as_ref().and_then(|ty| match ty {
                    Type::Imm(_, ty) | Type::Mut(_, ty) => Some((**ty).clone()),
                    _ => None,
                });
                let place_ty = self.check_place(place, expected_ty);
                (
                    if let Type::Mut(.., ty) | Type::Imm(.., ty) = place_ty {
                        *ty
                    } else {
                        self.diag
                            .borrow_mut()
                            .report(format!("Expected a reference but got '{place_ty}'"), line);
                        Type::Unknown
                    },
                    line,
                )
            }
        };
        if let Some(expected) = expected_ty {
            self.unify(expected, ty, line)
        } else {
            ty
        }
    }
    fn check_for_loop(
        &mut self,
        pattern: &Pattern,
        iterator: &Expr,
        body: &Expr,
        expected_ty: Option<Type>,
    ) -> Type {
        let iterator_ty = self.check_expr(iterator, None);
        let element = self.iterator_element(iterator_ty.clone());
        self.in_scope(|this| {
            let ty = match element {
                Some(ty) => ty,
                None => {
                    this.diag.borrow_mut().report(
                        format!("Cannot use '{}' as an iterator", iterator_ty),
                        iterator.line,
                    );
                    Type::Unknown
                }
            };
            let region = if let Type::Imm(region, _) | Type::Mut(region, _) = &iterator_ty {
                Some(region.clone())
            } else {
                None
            };
            this.check_pattern(pattern, ty.clone(), region);
            let body_ty = this.check_expr(body, Some(Type::Unit));
            if let Some(ty) = expected_ty {
                this.unify(ty, body_ty, body.line)
            } else {
                body_ty
            }
        })
    }
    fn check_borrow(
        &mut self,
        mutable: Mutable,
        name: &Ident,
        region_name: &Ident,
        body: &Expr,
        expected_ty: Option<Type>,
    ) -> Type {
        self.in_scope(|this| {
            let var = match this.get_res(&name.content) {
                Some(Res::Var(var)) => Some(var),
                Some(_) => {
                    this.diag.borrow_mut().report(
                        format!("Cannot use '{}' as variable", name.content),
                        name.line,
                    );
                    None
                }
                None => {
                    this.diag
                        .borrow_mut()
                        .report(format!("'{}' is not in scope", name.content), name.line);
                    None
                }
            };
            let ty = var
                .map(|var| this.var_type(var).clone())
                .unwrap_or(Type::Unknown);
            let region = this.declare_region(&region_name.content);
            this.declare_var(
                Mutable::Immutable,
                &name.content,
                match mutable {
                    Mutable::Immutable => Type::Imm(
                        Region::Local(region_name.content.clone(), region),
                        Box::new(ty),
                    ),
                    Mutable::Mutable => Type::Mut(
                        Region::Local(region_name.content.clone(), region),
                        Box::new(ty),
                    ),
                },
            );
            this.check_expr(body, expected_ty)
        })
    }
    fn check_lambda(&mut self, line: usize, lambda: &Lambda, expected_ty: Option<Type>) -> Type {
        let expected_sig = match expected_ty.clone().map(|ty| self.simplify(ty)) {
            Some(Type::Function(ref function)) => Some(function.clone()),
            _ => None,
        };

        let function = self.in_scope(|this| {
            let params = lambda
                .params
                .iter()
                .enumerate()
                .map(|(i, (name, ty))| {
                    let ty = match ty {
                        Some(ty) => {
                            let ty = this.lower_type(ty);
                            if let Some(sig) = &expected_sig
                                && let Some(expect) = sig.params.get(i)
                            {
                                this.unify(expect.clone(), ty.clone(), name.line);
                            }
                            ty
                        }
                        None => expected_sig
                            .as_ref()
                            .and_then(|sig| sig.params.get(i).cloned())
                            .unwrap_or_else(|| this.fresh_ty(name.line)),
                    };

                    this.declare_var(Mutable::Immutable, &name.content, ty.clone());
                    ty
                })
                .collect::<Vec<_>>();
            let return_type = this.check_expr(
                &lambda.body,
                match lambda.return_type {
                    Some(ref ty) => Some(this.lower_type(ty)),
                    None => expected_sig.as_ref().map(|sig| (*sig.return_type).clone()),
                },
            );
            Type::Function(FunctionType {
                params,
                return_type: Box::new(return_type),
            })
        });
        if let Some(ty) = expected_ty {
            self.unify(ty, function, line)
        } else {
            function
        }
    }
    fn check_sequence(&mut self, first: &Expr, rest: &Expr, expected_ty: Option<Type>) -> Type {
        let _ = self.check_expr(first, None);
        self.check_expr(rest, expected_ty)
    }
    fn check_ident(&mut self, ident: &Ident, expected_ty: Option<Type>) -> Type {
        let Some(res) = self.get_res(&ident.content) else {
            self.diag
                .borrow_mut()
                .report(format!("'{}' not in scope", ident.content), ident.line);
            return expected_ty.unwrap_or(Type::Unknown);
        };
        let ty = match res {
            Res::Builtin(builtin) => {
                let args = self.instantiate_builtin_args(builtin, ident.line);
                Type::Function(self.signature_of_builtin(builtin).bind(&args))
            }
            Res::Function(function) => {
                let args = self.instantiate_function_args(function, ident.line);
                Type::Function(self.signature_of_function(function).bind(&args))
            }
            Res::Var(var) => self.var_type(var).clone(),
            Res::Param(_) | Res::LocalRegion(_) => {
                self.diag.borrow_mut().report(
                    format!("Cannot use '{}' as value", ident.content),
                    ident.line,
                );
                return expected_ty.unwrap_or(Type::Unknown);
            }
        };
        if let Some(expected) = expected_ty {
            self.unify(expected, ty, ident.line)
        } else {
            ty
        }
    }
    fn check_call(&mut self, callee: &Expr, args: &[Expr], expected_ty: Option<Type>) -> Type {
        let callee_type = self.check_expr(callee, None);
        let callee_type = self.simplify(callee_type);
        let Type::Function(function) = callee_type else {
            self.diag.borrow_mut().report(
                format!("Expected a function type but got '{callee_type}'"),
                callee.line,
            );
            for arg in args {
                self.check_expr(arg, None);
            }
            return expected_ty.unwrap_or(Type::Unknown);
        };
        let FunctionType {
            params,
            return_type,
        } = function;
        if params.len() != args.len() {
            self.diag.borrow_mut().report(
                format!(
                    "Expected '{}' arguments but got '{}'",
                    params.len(),
                    args.len()
                ),
                callee.line,
            );
            for (i, arg) in args.iter().enumerate() {
                let arg = self.check_expr(arg, params.get(i).cloned());
            }
            return match expected_ty {
                Some(ty) => self.unify(ty, *return_type, callee.line),
                None => *return_type,
            };
        }
        for (arg, ty) in args.iter().zip(params) {
            self.check_expr(arg, Some(ty));
        }
        match expected_ty {
            Some(ty) => self.unify(ty, *return_type, callee.line),
            None => *return_type,
        }
    }
    fn check_let(
        &mut self,
        name: &Ident,
        mutable: Mutable,
        ty: Option<Type>,
        value: &Expr,
        body: &Expr,
        expected_ty: Option<Type>,
    ) -> Type {
        let value = self.check_expr(value, ty);
        self.declare_var(mutable, &name.content, value);
        self.check_expr(body, expected_ty)
    }
    pub(super) fn check_expr(&mut self, expr: &Expr, expected_ty: Option<Type>) -> Type {
        match &expr.kind {
            ExprKind::None(ty) => {
                let given = ty.as_ref().map(|ty| self.lower_type(ty));
                match (given, expected_ty) {
                    (None, None) => {
                        self.type_annotations_needed(expr.line);
                        Type::Option(Box::new(Type::Unknown))
                    }
                    (Some(ty), None) => Type::Option(Box::new(ty)),
                    (None, Some(expected)) => {
                        let expected = self.simplify(expected);
                        if let Type::Option(ty) = expected {
                            Type::Option(ty)
                        } else {
                            self.diag.borrow_mut().report(
                                format!("Expected option type but got '{}'", expected),
                                expr.line,
                            );
                            Type::Option(Box::new(Type::Unknown))
                        }
                    }
                    (Some(ty), Some(expected)) => {
                        if let Type::Option(expected) = expected {
                            self.unify(ty, *expected, expr.line)
                        } else {
                            self.unify(expected, Type::Option(Box::new(ty)), expr.line)
                        }
                    }
                }
            }
            ExprKind::Some(value) => match expected_ty {
                None => {
                    let value = self.check_expr(value, None);
                    Type::Option(Box::new(value))
                }
                Some(Type::Option(ty)) => {
                    let value = self.check_expr(value, Some(*ty));
                    Type::Option(Box::new(value))
                }
                Some(ty) => {
                    let value = self.check_expr(value, None);
                    self.unify(ty, Type::Option(Box::new(value)), expr.line)
                }
            },
            ExprKind::Print(arg) => {
                if let Some(expr) = arg {
                    self.check_expr(expr, None);
                }
                if let Some(ty) = expected_ty {
                    self.unify(ty, Type::Unit, expr.line)
                } else {
                    Type::Unit
                }
            }
            ExprKind::Unit => {
                if let Some(ty) = expected_ty {
                    self.unify(ty, Type::Unit, expr.line)
                } else {
                    Type::Unit
                }
            }
            ExprKind::Number(_) => {
                if let Some(ty) = expected_ty {
                    self.unify(ty, Type::Int, expr.line)
                } else {
                    Type::Int
                }
            }
            ExprKind::String(_) => {
                if let Some(ty) = expected_ty {
                    self.unify(ty, Type::String, expr.line)
                } else {
                    Type::String
                }
            }
            &ExprKind::Let(mutable, ref name, ref value, ref ty, ref body) => self.check_let(
                name,
                mutable,
                ty.as_ref().map(|ty| self.lower_type(ty)),
                value,
                body,
                expected_ty,
            ),
            ExprKind::Call(callee, args) => self.check_call(callee, args, expected_ty),
            ExprKind::Ident(name) => self.check_ident(name, expected_ty),
            ExprKind::Sequence(first, rest) => self.check_sequence(first, rest, expected_ty),
            ExprKind::Panic(ty) => match (ty.as_ref().map(|ty| self.lower_type(ty)), expected_ty) {
                (None, None) => {
                    self.type_annotations_needed(expr.line);
                    Type::Unknown
                }
                (Some(ty), None) | (None, Some(ty)) => ty,
                (Some(given), Some(expected)) => self.unify(expected, given, expr.line),
            },
            ExprKind::Binary(binary_op, left, right) => {
                let _ = binary_op;
                let _ = self.check_expr(left, Some(Type::Int));
                let _ = self.check_expr(right, Some(Type::Int));
                if let Some(expected) = expected_ty {
                    self.unify(expected, Type::Int, expr.line)
                } else {
                    Type::Int
                }
            }
            ExprKind::List(elements) => {
                let mut expected_element = match &expected_ty {
                    &Some(Type::List(ref ty)) => Some((**ty).clone()),
                    _ => None,
                };
                for element in elements {
                    let element_ty = self.check_expr(element, expected_element.clone());
                    if expected_element.is_none() {
                        expected_element = Some(element_ty);
                    }
                }
                let ty = Type::List(Box::new(expected_element.unwrap_or_else(|| {
                    self.type_annotations_needed(expr.line);
                    Type::Unknown
                })));
                if let Some(expected) = expected_ty {
                    self.unify(expected, ty, expr.line)
                } else {
                    ty
                }
            }
            ExprKind::Deref(reference) => {
                let ty = self.check_expr(reference, None);
                let ty = if let Type::Mut(_, ty) | Type::Imm(_, ty) = ty {
                    *ty
                } else {
                    self.diag
                        .borrow_mut()
                        .report(format!("Cannot deref '{ty}'"), expr.line);
                    Type::Unknown
                };
                if let Some(expected) = expected_ty {
                    self.unify(expected, ty, expr.line)
                } else {
                    ty
                }
            }
            ExprKind::Lambda(lambda) => self.check_lambda(expr.line, lambda, expected_ty),
            ExprKind::Borrow(mutable, name, region_name, body) => {
                self.check_borrow(*mutable, name, region_name, body, expected_ty)
            }
            ExprKind::For(pattern, iterator, body) => {
                self.check_for_loop(pattern, iterator, body, expected_ty)
            }
            ExprKind::Case(matched, case_arms) => {
                let matched = self.check_expr(matched, None);
                let mut combined_type = expected_ty;
                for arm in case_arms {
                    self.in_scope(|this| {
                        let region = match &matched {
                            Type::Imm(region, _) | Type::Mut(region, _) => Some(region.clone()),
                            _ => None,
                        };
                        this.check_pattern(&arm.pat, matched.clone(), region);
                        let ty = this.check_expr(&arm.body, combined_type.clone());
                        if combined_type.is_none() {
                            combined_type = Some(ty);
                        }
                    })
                }
                combined_type.unwrap_or_else(|| {
                    self.type_annotations_needed(expr.line);
                    Type::Unknown
                })
            }
            ExprKind::Assign(place, value) => {
                let value = self.check_expr(value, None);
                let _ = self.check_place(place, Some(value));
                if let Some(ty) = expected_ty {
                    self.unify(ty, Type::Unit, expr.line)
                } else {
                    Type::Unit
                }
            }
        }
    }
}
