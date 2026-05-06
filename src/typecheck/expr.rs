use crate::{
    resolved_ast::{BorrowExpr, Expr, ExprKind, Lambda, Pattern, Place, PlaceKind, Var},
    typecheck::root::TypeCheck,
    typed_ast,
    types::{FunctionType, Region, Type},
};

impl TypeCheck {
    fn check_place(&mut self, place: Place, expected_ty: Option<Type>) -> typed_ast::Place {
        let (ty, kind) = match place.kind {
            PlaceKind::Var(var) => (self.var_type(var.1).clone(), typed_ast::PlaceKind::Var(var)),
            PlaceKind::Deref(value) => {
                let expected_ty = expected_ty.as_ref().and_then(|ty| match ty {
                    Type::Imm(_, ty) | Type::Mut(_, ty) => Some((**ty).clone()),
                    _ => None,
                });
                let value = self.check_expr(*value, expected_ty);
                (
                    match value.ty.as_reference_type() {
                        Ok((_, _, ty)) => ty.clone(),
                        Err(ty) => {
                            self.diag
                                .borrow_mut()
                                .report(format!("Expected a reference but got '{ty}'"), value.line);
                            Type::Unknown
                        }
                    },
                    typed_ast::PlaceKind::Deref(Box::new(value)),
                )
            }
        };
        let ty = if let Some(expected) = expected_ty {
            self.unify(expected, ty, place.line)
        } else {
            ty
        };
        typed_ast::Place {
            ty,
            line: place.line,
            kind,
        }
    }
    fn check_for_loop(
        &mut self,
        line: usize,
        pattern: Pattern,
        iterator: Expr,
        body: Expr,
    ) -> typed_ast::Expr {
        let iterator = self.check_expr(iterator, None);
        let element = self.iterator_element(iterator.ty.clone());
        let element = match element {
            Ok(element) => element,
            Err(_) => {
                self.diag.borrow_mut().report(
                    format!("Cannot use '{}' as an iterator", iterator.ty),
                    iterator.line,
                );
                Type::Unknown
            }
        };
        let pattern = self.check_pattern(pattern, element, None);
        let body = self.check_expr(body, Some(Type::Unit));
        typed_ast::Expr {
            ty: Type::Unit,
            line,
            kind: typed_ast::ExprKind::For {
                pattern,
                iterator: Box::new(iterator),
                body: Box::new(body),
            },
        }
    }
    fn check_borrow(
        &mut self,
        line: usize,
        borrow: BorrowExpr,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let BorrowExpr {
            mutable,
            var_name,
            old_var,
            new_var,
            region_name,
            region,
            body,
        } = borrow;
        let var_ty = self.var_type(old_var).clone();
        let new_ty = var_ty
            .clone()
            .reference(mutable, Region::Local(region_name.content.clone(), region));
        self.declare_var(new_var, new_ty.clone());
        let body = self.check_expr(body, expected_ty);
        typed_ast::Expr {
            ty: body.ty.clone(),
            line,
            kind: typed_ast::ExprKind::Borrow {
                mutable,
                var_name,
                old_var,
                new_var,
                region_name,
                region,
                new_ty,
                body: Box::new(body),
            },
        }
    }
    fn check_lambda(&mut self, line: usize, lambda: Lambda, hint: Option<Type>) -> typed_ast::Expr {
        let expected_sig = match hint.clone().map(|ty| self.simplify_type(ty)) {
            Some(Type::Function(ref function)) => Some(function.clone()),
            _ => None,
        };
        let params = lambda
            .params
            .into_iter()
            .enumerate()
            .map(|(i, (name, var, ty))| {
                let ty = match ty {
                    Some(ty) => {
                        let ty = self.lower_type(ty);
                        if let Some(sig) = &expected_sig
                            && let Some(expect) = sig.params.get(i)
                        {
                            self.unify(expect.clone(), ty.clone(), name.line);
                        }
                        ty
                    }
                    None => expected_sig
                        .as_ref()
                        .and_then(|sig| sig.params.get(i).cloned())
                        .unwrap_or_else(|| self.fresh_ty(name.line)),
                };

                self.declare_var(var, ty.clone());
                (name, var, ty)
            })
            .collect::<Vec<_>>();
        let body = self.check_expr(
            lambda.body,
            expected_sig.as_ref().map(|sig| (*sig.return_type).clone()),
        );
        let function = Type::Function(FunctionType {
            resource: lambda.resource,
            params: params.iter().map(|(_, _, ty)| ty.clone()).collect(),
            return_type: Box::new(body.ty.clone()),
        });
        typed_ast::Expr {
            ty: function,
            line,
            kind: typed_ast::ExprKind::Lambda(Box::new(typed_ast::Lambda {
                is_resource: lambda.resource,
                params,
                return_type: body.ty.clone(),
                body,
            })),
        }
    }
    fn check_sequence(
        &mut self,
        first: Expr,
        rest: Expr,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let first = self.check_expr(first, None);
        let second = self.check_expr(rest, expected_ty);
        typed_ast::Expr {
            ty: second.ty.clone(),
            line: first.line,
            kind: typed_ast::ExprKind::Sequence(Box::new(first), Box::new(second)),
        }
    }
    fn check_call(
        &mut self,
        line: usize,
        callee: Expr,
        args: Vec<Expr>,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let callee = self.check_expr(callee, None);
        let callee_type = self.simplify_type(callee.ty.clone());
        let (params, return_type) = match callee_type {
            Type::Function(FunctionType {
                resource: _,
                params,
                return_type,
            }) => (params, Some(*return_type)),
            ty => {
                self.diag.borrow_mut().report(
                    format!("Expected a function type but got '{ty}'"),
                    callee.line,
                );
                (Vec::new(), None)
            }
        };
        if params.len() != args.len() {
            self.diag.borrow_mut().report(
                format!(
                    "Expected '{}' arguments but got '{}'",
                    params.len(),
                    args.len()
                ),
                callee.line,
            );
        }

        let arg_map = |(arg, expected_ty)| self.check_expr(arg, expected_ty);
        let args = if args.len() > params.len() {
            let diff = args.len() - params.len();
            args.into_iter()
                .zip(
                    params
                        .into_iter()
                        .map(Some)
                        .chain(std::iter::repeat_n(None, diff)),
                )
                .map(arg_map)
                .collect::<Vec<_>>()
        } else {
            args.into_iter()
                .zip(params.into_iter().map(Some))
                .map(arg_map)
                .collect::<Vec<_>>()
        };
        let ty = match (expected_ty, return_type) {
            (None, None) => Type::Unknown,
            (None, Some(ty)) | (Some(ty), None) => ty,
            (Some(ty), Some(return_type)) => self.unify(ty, return_type, callee.line),
        };
        typed_ast::Expr {
            ty,
            line,
            kind: typed_ast::ExprKind::Call(Box::new(callee), args),
        }
    }
    fn check_let(
        &mut self,
        line: usize,
        pattern: Pattern,
        ty: Option<Type>,
        value: Expr,
        body: Expr,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let binder = self.check_expr(value, ty);
        let pattern = self.check_pattern(pattern, binder.ty.clone(), None);
        let body = self.check_expr(body, expected_ty);
        typed_ast::Expr {
            ty: body.ty.clone(),
            line,
            kind: typed_ast::ExprKind::Let {
                pattern,
                binder: Box::new(binder),
                body: Box::new(body),
            },
        }
    }
    pub(super) fn check_expr(&mut self, expr: Expr, expected_ty: Option<Type>) -> typed_ast::Expr {
        let Expr { line, kind } = expr;
        let make_expr = move |ty, kind| typed_ast::Expr { ty, kind, line };
        let mut expr = match kind {
            ExprKind::Err => typed_ast::Expr {
                line,
                ty: Type::Unknown,
                kind: typed_ast::ExprKind::Err,
            },
            ExprKind::Bool(value) => typed_ast::Expr {
                line,
                ty: Type::Bool,
                kind: typed_ast::ExprKind::Bool(value),
            },
            ExprKind::Var(var, id) => make_expr(
                self.var_type(id).clone(),
                typed_ast::ExprKind::Load(typed_ast::Place {
                    ty: self.var_type(id).clone(),
                    line,
                    kind: typed_ast::PlaceKind::Var(Var(var, id)),
                }),
            ),
            ExprKind::Builtin(builtin) => {
                let args = self.instantiate_builtin_args(builtin, line);
                make_expr(
                    Type::Function(self.signature_of_builtin(builtin).bind(&args)),
                    typed_ast::ExprKind::Builtin(builtin, args),
                )
            }
            ExprKind::Function(name, function) => {
                let args = self.instantiate_function_args(function, line);
                make_expr(
                    Type::Function(self.signature_of_function(function).bind(&args)),
                    typed_ast::ExprKind::Function(name, function, args),
                )
            }
            ExprKind::None(ty) => {
                let given = ty.map(|ty| self.lower_type(ty));
                let ty = match (given, expected_ty.clone()) {
                    (None, None) => {
                        self.type_annotations_needed(expr.line);
                        Type::Unknown
                    }
                    (Some(ty), None) => ty,
                    (None, Some(expected)) => {
                        let expected = self.simplify_type(expected);
                        if let Type::Option(ty) = expected {
                            *ty
                        } else {
                            self.diag.borrow_mut().report(
                                format!("Expected option type but got '{}'", expected),
                                expr.line,
                            );
                            Type::Unknown
                        }
                    }
                    (Some(ty), Some(expected)) => {
                        if let Type::Option(expected) = expected {
                            self.unify(ty, *expected, expr.line)
                        } else {
                            Type::Unknown
                        }
                    }
                };
                make_expr(Type::Option(Box::new(ty)), typed_ast::ExprKind::None)
            }
            ExprKind::Some(value) => {
                let expected_inner = expected_ty.as_ref().and_then(|ty| match ty {
                    Type::Option(ty) => Some((**ty).clone()),
                    _ => None,
                });
                let value = self.check_expr(*value, expected_inner);
                make_expr(
                    Type::Option(Box::new(value.ty.clone())),
                    typed_ast::ExprKind::Some(Box::new(value)),
                )
            }
            ExprKind::Print(arg) => {
                let arg = arg.map(|arg| Box::new(self.check_expr(*arg, None)));
                make_expr(Type::Unit, typed_ast::ExprKind::Print(arg))
            }
            ExprKind::Unit => make_expr(Type::Unit, typed_ast::ExprKind::Unit),
            ExprKind::Int(value) => make_expr(Type::Int, typed_ast::ExprKind::Int(value)),
            ExprKind::String(value) => make_expr(Type::String, typed_ast::ExprKind::String(value)),
            ExprKind::Let(let_expr) => {
                return self.check_let(
                    expr.line,
                    let_expr.pattern,
                    let_expr.ty.map(|ty| self.lower_type(ty)),
                    let_expr.binder,
                    let_expr.body,
                    expected_ty,
                );
            }
            ExprKind::Call(callee, args) => {
                return self.check_call(expr.line, *callee, args, expected_ty);
            }
            ExprKind::Sequence(first, rest) => {
                return self.check_sequence(*first, *rest, expected_ty);
            }
            ExprKind::Panic(ty) => {
                let ty = match (ty.map(|ty| self.lower_type(ty)), expected_ty) {
                    (None, None) => {
                        self.type_annotations_needed(expr.line);
                        Type::Unknown
                    }
                    (Some(ty), None) | (None, Some(ty)) => ty,
                    (Some(given), Some(expected)) => self.unify(expected, given, expr.line),
                };
                return typed_ast::Expr {
                    line: expr.line,
                    ty,
                    kind: typed_ast::ExprKind::Panic,
                };
            }
            ExprKind::Binary(binary_op, left, right) => {
                let left = self.check_expr(*left, Some(Type::Int));
                let right = self.check_expr(*right, Some(Type::Int));
                typed_ast::Expr {
                    line: expr.line,
                    ty: Type::Int,
                    kind: typed_ast::ExprKind::Binary(binary_op, Box::new(left), Box::new(right)),
                }
            }
            ExprKind::List(elements) => {
                let mut expected_element = match &expected_ty {
                    &Some(Type::List(ref ty)) => Some((**ty).clone()),
                    _ => None,
                };
                let elements = elements
                    .into_iter()
                    .map(|element| {
                        let element = self.check_expr(element, expected_element.clone());
                        expected_element.get_or_insert_with(|| element.ty.clone());
                        element
                    })
                    .collect();
                let ty = Type::List(Box::new(expected_element.unwrap_or_else(|| {
                    self.type_annotations_needed(expr.line);
                    Type::Unknown
                })));
                typed_ast::Expr {
                    ty,
                    line: expr.line,
                    kind: typed_ast::ExprKind::List(elements),
                }
            }
            ExprKind::Deref(reference) => {
                let reference = self.check_expr(*reference, None);
                let pointee_ty = match reference.ty.as_reference_type() {
                    Ok((_, _, ty)) => ty.clone(),
                    Err(ty) => {
                        self.diag
                            .borrow_mut()
                            .report(format!("Cannot deref '{ty}'"), expr.line);
                        Type::Unknown
                    }
                };
                typed_ast::Expr {
                    ty: pointee_ty.clone(),
                    line,
                    kind: typed_ast::ExprKind::Load(typed_ast::Place {
                        ty: pointee_ty,
                        line: reference.line,
                        kind: typed_ast::PlaceKind::Deref(Box::new(reference)),
                    }),
                }
            }
            ExprKind::Lambda(lambda) => self.check_lambda(expr.line, *lambda, expected_ty.clone()),
            ExprKind::Borrow(borrow) => return self.check_borrow(expr.line, *borrow, expected_ty),
            ExprKind::For(pattern, iterator, body) => {
                self.check_for_loop(expr.line, pattern, *iterator, *body)
            }
            ExprKind::Case(matched, case_arms) => {
                let matched = self.check_expr(*matched, None);
                let arms = case_arms
                    .into_iter()
                    .map(|arm| {
                        let pattern = self.check_pattern(arm.pattern, matched.ty.clone(), None);
                        let body = self.check_expr(arm.body, expected_ty.clone());
                        typed_ast::CaseArm { pattern, body }
                    })
                    .collect();
                let ty = expected_ty.unwrap_or_else(|| {
                    self.type_annotations_needed(expr.line);
                    Type::Unknown
                });
                return typed_ast::Expr {
                    ty,
                    line: expr.line,
                    kind: typed_ast::ExprKind::Case(Box::new(matched), arms),
                };
            }
            ExprKind::Assign(place, value) => {
                let value = self.check_expr(*value, None);
                let place = self.check_place(place, Some(value.ty.clone()));
                typed_ast::Expr {
                    line: expr.line,
                    ty: Type::Unit,
                    kind: typed_ast::ExprKind::Assign(place, Box::new(value)),
                }
            }
        };
        if let Some(expected) = expected_ty {
            expr.ty = self.unify(expected, expr.ty, expr.line)
        };
        expr
    }
}
