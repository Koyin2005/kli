use std::collections::{HashMap, HashSet};

use crate::{
    resolved_ast::{
        self, BlockBody, BorrowExpr, Expr, ExprKind, FieldInit, Lambda, LocalRegionId, Pattern,
        Place, PlaceKind, Var,
    },
    src_loc::SrcLoc,
    typecheck::root::TypeCheck,
    typed_ast::{self, FieldId, RecordFieldInit},
    types::{FunctionType, GenericArg, PointerType, RecordField, Type},
};

impl TypeCheck {
    fn check_place(&mut self, place: Place, expected_ty: Option<Type>) -> typed_ast::Place {
        let (ty, kind) = match place.kind {
            PlaceKind::Var(var) => (
                self.var_type(var.1).clone(),
                if self.capture(var.1) {
                    typed_ast::PlaceKind::Upvar(var)
                } else {
                    typed_ast::PlaceKind::Var(var)
                },
            ),
            PlaceKind::Deref(value) => {
                let value = self.check_expr(*value, None);
                (
                    match self.simplify_type(value.ty.clone()).as_pointer_type() {
                        Ok((PointerType::Raw | PointerType::Reference(..), ty)) => ty.clone(),
                        Ok((p, ty)) => {
                            self.non_deref_error(Type::pointer_type(p, ty), value.loc.clone())
                        }
                        Err(ty) => self.non_deref_error(ty, value.loc.clone()),
                    },
                    typed_ast::PlaceKind::Deref(Box::new(value)),
                )
            }
        };
        let ty = if let Some(expected) = expected_ty {
            self.unify(expected, ty, place.loc.clone())
        } else {
            ty
        };
        typed_ast::Place {
            ty,
            loc: place.loc,
            kind,
        }
    }
    fn check_for_loop(
        &mut self,
        loc: SrcLoc,
        pattern: Pattern,
        iterator: Expr,
        body: Expr,
    ) -> typed_ast::Expr {
        let iterator = self.check_expr(iterator, None);
        let element = self.iterator_element(iterator.ty.clone());
        let (iterator_type, element) = match element {
            Ok((iterator_type, ty)) => (Some(iterator_type), ty),
            Err(_) => {
                self.diag.borrow_mut().add_diagnostic(
                    format!("Cannot use '{}' as an iterator", iterator.ty),
                    iterator.loc.clone(),
                );
                (None, Type::Unknown)
            }
        };
        let pattern = self.check_pattern(pattern, element, None);
        let body = self.check_expr(body, Some(Type::Unit));
        let Some(iterator_type) = iterator_type else {
            return typed_ast::Expr {
                ty: Type::Unit,
                loc,
                kind: typed_ast::ExprKind::Err,
            };
        };
        typed_ast::Expr {
            ty: Type::Unit,
            loc,
            kind: typed_ast::ExprKind::For {
                pattern,
                iterator: Box::new(iterator),
                iterator_type,
                body: Box::new(body),
            },
        }
    }
    fn check_builtin(
        &mut self,
        loc: SrcLoc,
        builtin: crate::resolved_ast::Builtin,
        args: Option<Vec<resolved_ast::Type>>,
    ) -> (FunctionType, Vec<GenericArg>) {
        let args = if let Some(args) = args {
            let arg_count = self.generic_arg_count_of_builtin(builtin);
            if arg_count != args.len() {
                self.diag.borrow_mut().add_diagnostic(
                    format!(
                        "Expected '{}' generic args but got '{}'",
                        arg_count,
                        args.len()
                    ),
                    loc.clone(),
                );
            }
            let remaining = args.len().abs_diff(arg_count);
            args.into_iter()
                .map(|arg| self.lower_type(arg))
                .chain(std::iter::repeat_n(Type::Unknown, remaining))
                .map(GenericArg::Type)
                .collect()
        } else {
            self.instantiate_builtin_args(builtin, loc.clone())
        };
        (self.signature_of_builtin(builtin).bind(&args), args)
    }
    fn check_borrow(
        &mut self,
        loc: SrcLoc,
        borrow: BorrowExpr,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let BorrowExpr {
            mutable,
            place,
            region,
        } = borrow;
        let (ty_mutable, ty_region, expected) = if let Some(ref expected) = expected_ty
            && let Ok((ty_mutable, ty_region, ty)) = expected.as_reference_type()
        {
            (Some(ty_mutable), Some(ty_region.clone()), Some(ty.clone()))
        } else {
            (None, None, None)
        };
        let place = self.check_place(place, expected);
        let region = {
            let region = self.lower_region(region);
            match ty_region {
                Some(expected) => self.unify_region(expected, region, loc.clone()),
                None => region,
            }
        };
        if let Some(ty_mutable) = ty_mutable
            && ty_mutable != mutable
        {
            self.diag.borrow_mut().add_diagnostic(
                format!("Expected a '{}' but got '{}'", ty_mutable, mutable),
                loc.clone(),
            );
        }
        if let Some(expected) = expected_ty
            && expected.as_reference_type().is_err()
        {
            self.diag.borrow_mut().add_diagnostic(
                format!("Expected a reference but got '{}'", expected),
                loc.clone(),
            );
        }
        typed_ast::Expr {
            ty: Type::reference(place.ty.clone(), mutable, region.clone()),
            loc,
            kind: typed_ast::ExprKind::Borrow {
                mutable,
                region,
                place,
            },
        }
    }
    fn check_lambda(&mut self, loc: SrcLoc, lambda: Lambda, hint: Option<Type>) -> typed_ast::Expr {
        let id = lambda.id;
        let expected_sig = match hint.clone().map(|ty| self.simplify_type(ty)) {
            Some(Type::Function(ref function)) => Some(function.clone()),
            _ => None,
        };
        let (captures, (params, body)) = self.with_capture_scope(|this| {
            let params = lambda
                .params
                .into_iter()
                .enumerate()
                .map(|(i, (name, var, ty))| {
                    let ty = match ty {
                        Some(ty) => {
                            let ty = this.lower_type(ty);
                            if let Some(sig) = &expected_sig
                                && let Some(expect) = sig.params.get(i)
                            {
                                this.unify(expect.clone(), ty.clone(), name.loc.clone());
                            }
                            ty
                        }
                        None => expected_sig
                            .as_ref()
                            .and_then(|sig| sig.params.get(i).cloned())
                            .unwrap_or_else(|| this.fresh_ty(name.loc.clone())),
                    };

                    this.declare_var(var, ty.clone(), name.content.clone());
                    typed_ast::Param { name, var, ty }
                })
                .collect::<Vec<_>>();
            let body = this.check_expr(
                lambda.body,
                expected_sig.as_ref().map(|sig| (*sig.return_type).clone()),
            );
            (params, body)
        });
        let function = Type::Function(FunctionType {
            resource: lambda.resource,
            params: params
                .iter()
                .map(|typed_ast::Param { ty, .. }| ty.clone())
                .collect(),
            return_type: Box::new(body.ty.clone()),
        });
        let captures = captures
            .into_iter()
            .map(|capture| {
                (
                    Var(self.var_name(capture), capture),
                    self.var_type(capture).clone(),
                )
            })
            .collect();
        typed_ast::Expr {
            ty: function,
            loc,
            kind: typed_ast::ExprKind::Lambda(Box::new(typed_ast::Lambda {
                id,
                captures,
                is_resource: lambda.resource,
                params,
                return_type: body.ty.clone(),
                body,
            })),
        }
    }
    fn check_call(
        &mut self,
        loc: SrcLoc,
        callee: Expr,
        args: Vec<Expr>,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        fn check_call_sig(
            this: &mut TypeCheck,
            callee_loc: SrcLoc,
            args: Vec<Expr>,
            params: Vec<Type>,
            return_type: Option<Type>,
            expected_ty: Option<Type>,
        ) -> (Type, Vec<typed_ast::Expr>) {
            if params.len() != args.len() {
                this.diag.borrow_mut().add_diagnostic(
                    format!(
                        "Expected '{}' arguments but got '{}'",
                        params.len(),
                        args.len()
                    ),
                    callee_loc.clone(),
                );
            }

            let arg_map = |(arg, expected_ty)| this.check_expr(arg, expected_ty);
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
                (Some(ty), Some(return_type)) => this.unify(ty, return_type, callee_loc),
            };
            (ty, args)
        }
        if let ExprKind::Builtin(id, generic_args) = callee.kind {
            let (
                FunctionType {
                    resource: _,
                    params,
                    return_type,
                },
                generic_args,
            ) = self.check_builtin(callee.loc.clone(), id, generic_args);
            let (ty, args) = check_call_sig(
                self,
                callee.loc,
                args,
                params,
                Some(*return_type),
                expected_ty,
            );
            typed_ast::Expr {
                ty,
                loc,
                kind: typed_ast::ExprKind::BuiltinCall(id, generic_args, args),
            }
        } else {
            let callee = self.check_expr(callee, None);
            let callee_type = self.simplify_type(callee.ty.clone());
            let (params, return_type) = match callee_type {
                Type::Function(FunctionType {
                    resource: _,
                    params,
                    return_type,
                }) => (params, Some(*return_type)),
                ty => {
                    self.diag.borrow_mut().add_diagnostic(
                        format!("Expected a function type but got '{ty}'"),
                        callee.loc.clone(),
                    );
                    (Vec::new(), None)
                }
            };
            let (ty, args) = check_call_sig(
                self,
                callee.loc.clone(),
                args,
                params,
                return_type,
                expected_ty,
            );
            typed_ast::Expr {
                ty,
                loc,
                kind: typed_ast::ExprKind::Call(Box::new(callee), args),
            }
        }
    }
    fn check_block(
        &mut self,
        loc: SrcLoc,
        body: BlockBody,
        region: Option<LocalRegionId>,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let stmts = body
            .stmts
            .into_iter()
            .map(|stmt| self.check_stmt(stmt))
            .collect();
        let expr = self.check_expr(*body.expr, expected_ty);
        let ty = expr.ty.clone();
        let body = typed_ast::BlockBody {
            stmts,
            expr: Box::new(expr),
        };
        typed_ast::Expr {
            ty,
            loc,
            kind: typed_ast::ExprKind::Block(body, region),
        }
    }
    fn check_record(
        &mut self,
        loc: SrcLoc,
        field_inits: Vec<FieldInit>,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let expected_fields = match expected_ty.map(|ty| self.simplify_type(ty)) {
            Some(Type::Record(fields)) => Some(fields),
            _ => None,
        };
        let mut fields = Vec::new();
        let mut seen_fields = HashSet::new();
        let field_names = expected_fields
            .iter()
            .flatten()
            .enumerate()
            .map(|(i, field)| (field.name.clone(), i))
            .collect::<HashMap<_, _>>();

        for (i, FieldInit { name, value }) in field_inits.into_iter().enumerate() {
            let field_id = field_names.get(&name.content).copied().map(FieldId::new);
            let value = self.check_expr(
                value,
                expected_fields
                    .as_ref()
                    .and_then(|fields| field_id.map(|field| fields[field].ty.clone())),
            );
            if expected_fields.is_some() && !seen_fields.insert(name.content.clone()) {
                self.diag.borrow_mut().add_diagnostic(
                    format!("Repeated field '{}'", name.content),
                    name.loc.clone(),
                );
                continue;
            }

            let field_id = if let Some(field_id) = field_id {
                field_id
            } else if expected_fields.is_some() {
                self.diag.borrow_mut().add_diagnostic(
                    format!("'record' has no field '{}'", name.content),
                    name.loc,
                );
                continue;
            } else {
                FieldId::new(i)
            };
            fields.push(RecordFieldInit {
                index: field_id,
                name,
                value,
            });
        }
        let record_fields = if let Some(fields) = expected_fields {
            let mut field_names = field_names;
            for field in &fields {
                if !seen_fields.contains(&field.name) && field_names.remove(&field.name).is_some() {
                    self.diag
                        .borrow_mut()
                        .add_diagnostic(format!("Missing field '{}'", field.name), loc.clone());
                }
            }
            fields
        } else {
            fields
                .iter()
                .map(|field| RecordField {
                    name: field.name.content.clone(),
                    ty: field.value.ty.clone(),
                })
                .collect()
        };
        typed_ast::Expr {
            ty: Type::Record(record_fields),
            loc,
            kind: typed_ast::ExprKind::Record(fields),
        }
    }
    pub(super) fn check_expr(&mut self, expr: Expr, expected_ty: Option<Type>) -> typed_ast::Expr {
        let Expr { loc, kind } = expr;
        let make_expr = |ty, kind, loc| typed_ast::Expr { ty, kind, loc };
        let mut expr = match kind {
            ExprKind::Record(fields) => self.check_record(loc, fields, expected_ty.clone()),
            ExprKind::Block(block, region) => {
                return self.check_block(loc, block, region, expected_ty);
            }
            ExprKind::Annotate(expr, ty) => self.check_expr(*expr, Some(self.lower_type(*ty))),
            ExprKind::Err => typed_ast::Expr {
                loc,
                ty: Type::Unknown,
                kind: typed_ast::ExprKind::Err,
            },
            ExprKind::Bool(value) => typed_ast::Expr {
                loc,
                ty: Type::Bool,
                kind: typed_ast::ExprKind::Bool(value),
            },
            ExprKind::Var(var, id) => {
                let captured = self.capture(id);
                make_expr(
                    self.var_type(id).clone(),
                    typed_ast::ExprKind::Load(typed_ast::Place {
                        ty: self.var_type(id).clone(),
                        loc: loc.clone(),
                        kind: if !captured {
                            typed_ast::PlaceKind::Var(Var(var, id))
                        } else {
                            typed_ast::PlaceKind::Upvar(Var(var, id))
                        },
                    }),
                    loc,
                )
            }
            ExprKind::Builtin(builtin, args) => {
                let (ty, _) = self.check_builtin(loc.clone(), builtin, args);
                self.diag.borrow_mut().add_diagnostic(
                    format!(
                        "Cannot use builtin '{}' in non-call position",
                        builtin.name()
                    ),
                    loc.clone(),
                );
                make_expr(Type::Function(ty), typed_ast::ExprKind::Err, loc)
            }
            ExprKind::Function(name, function, args) => {
                let args = if let Some(args) = args {
                    let arg_count = self.generic_arg_count_of_function(function);
                    if arg_count != args.len() {
                        self.diag.borrow_mut().add_diagnostic(
                            format!(
                                "Expected '{}' generic args but got '{}'",
                                arg_count,
                                args.len()
                            ),
                            loc.clone(),
                        );
                    }
                    let remaining = args.len().abs_diff(arg_count);
                    args.into_iter()
                        .map(|arg| self.lower_type(arg))
                        .chain(std::iter::repeat_n(Type::Unknown, remaining))
                        .map(GenericArg::Type)
                        .collect()
                } else {
                    self.instantiate_function_args(function, loc.clone())
                };
                make_expr(
                    Type::Function(self.signature_of_function(function).bind(&args)),
                    typed_ast::ExprKind::Function(name, function, args),
                    loc,
                )
            }
            ExprKind::VariantCase(name, case, args) => {
                let args = if let Some(args) = args {
                    let arg_count = todo!("type def generic args");
                    if arg_count != args.len() {
                        self.diag.borrow_mut().add_diagnostic(
                            format!(
                                "Expected '{}' generic args but got '{}'",
                                arg_count,
                                args.len()
                            ),
                            loc.clone(),
                        );
                    };
                    let remaining = args.len().abs_diff(arg_count);
                    args.into_iter()
                        .map(|arg| self.lower_type(arg))
                        .chain(std::iter::repeat_n(Type::Unknown, remaining))
                        .map(GenericArg::Type)
                        .collect::<Vec<_>>()
                } else {
                    todo!("type def generic args")
                };
                todo!()
            }
            ExprKind::None(ty) => {
                let given = ty.map(|ty| self.lower_type(ty));
                let ty = match (given, expected_ty.clone()) {
                    (None, None) => {
                        self.type_annotations_needed(loc.clone());
                        Type::Unknown
                    }
                    (Some(ty), None) => ty,
                    (None, Some(expected)) => {
                        let expected = self.simplify_type(expected);
                        if let Type::Option(ty) = expected {
                            *ty
                        } else {
                            self.diag.borrow_mut().add_diagnostic(
                                format!("Expected option type but got '{}'", expected),
                                loc.clone(),
                            );
                            Type::Unknown
                        }
                    }
                    (Some(ty), Some(expected)) => {
                        if let Type::Option(ty) =
                            self.unify(Type::Option(Box::new(ty)), expected.clone(), loc.clone())
                        {
                            *ty
                        } else {
                            Type::Unknown
                        }
                    }
                };
                make_expr(Type::Option(Box::new(ty)), typed_ast::ExprKind::None, loc)
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
                    loc,
                )
            }
            ExprKind::Print(arg) => {
                let arg = arg.map(|arg| Box::new(self.check_expr(*arg, None)));
                make_expr(Type::Unit, typed_ast::ExprKind::Print(arg), loc)
            }
            ExprKind::Unit => make_expr(Type::Unit, typed_ast::ExprKind::Unit, loc),
            ExprKind::Int(value) => make_expr(Type::Int, typed_ast::ExprKind::Int(value), loc),
            ExprKind::String(value) => {
                make_expr(Type::String, typed_ast::ExprKind::String(value), loc)
            }
            ExprKind::Call(callee, args) => {
                return self.check_call(loc, *callee, args, expected_ty);
            }
            ExprKind::Panic(ty) => {
                let ty = match (ty.map(|ty| self.lower_type(ty)), expected_ty) {
                    (None, None) => {
                        self.type_annotations_needed(loc.clone());
                        Type::Unknown
                    }
                    (Some(ty), None) | (None, Some(ty)) => ty,
                    (Some(given), Some(expected)) => self.unify(expected, given, loc.clone()),
                };
                return typed_ast::Expr {
                    loc,
                    ty,
                    kind: typed_ast::ExprKind::Panic,
                };
            }
            ExprKind::Binary(binary_op, left, right) => {
                let left = self.check_expr(*left, Some(Type::Int));
                let right = self.check_expr(*right, Some(Type::Int));
                typed_ast::Expr {
                    loc,
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
                    self.type_annotations_needed(loc.clone());
                    Type::Unknown
                })));
                typed_ast::Expr {
                    ty,
                    loc,
                    kind: typed_ast::ExprKind::List(elements),
                }
            }
            ExprKind::Deref(reference) => {
                let reference = self.check_expr(*reference, None);
                let pointee_ty = match reference.ty.clone().as_pointer_type() {
                    Ok((PointerType::Raw | PointerType::Reference(..), ty)) => ty.clone(),
                    Ok((p, ty)) => self.non_deref_error(Type::pointer_type(p, ty), loc.clone()),
                    Err(ty) => self.non_deref_error(ty, loc.clone()),
                };
                typed_ast::Expr {
                    ty: pointee_ty.clone(),
                    loc,
                    kind: typed_ast::ExprKind::Load(typed_ast::Place {
                        ty: pointee_ty,
                        loc: reference.loc.clone(),
                        kind: typed_ast::PlaceKind::Deref(Box::new(reference)),
                    }),
                }
            }
            ExprKind::Lambda(lambda) => self.check_lambda(loc, *lambda, expected_ty.clone()),
            ExprKind::Borrow(borrow) => return self.check_borrow(loc, *borrow, expected_ty),
            ExprKind::For(pattern, iterator, body) => {
                self.check_for_loop(loc, pattern, *iterator, *body)
            }
            ExprKind::Case(matched, case_arms) => {
                let matched = self.check_expr(*matched, None);
                let mut prev_ty = None::<Type>;
                let arms = case_arms
                    .into_iter()
                    .map(|arm| {
                        let pattern = self.check_pattern(arm.pattern, matched.ty.clone(), None);
                        let body = self.check_expr(arm.body, expected_ty.clone());
                        if expected_ty.is_none() {
                            if let Some(ref prev_ty) = prev_ty {
                                self.unify(prev_ty.clone(), body.ty.clone(), body.loc.clone());
                            } else {
                                prev_ty = Some(body.ty.clone());
                            }
                        }
                        typed_ast::CaseArm { pattern, body }
                    })
                    .collect();
                let ty = expected_ty.or(prev_ty).unwrap_or_else(|| {
                    self.type_annotations_needed(loc.clone());
                    Type::Unknown
                });
                return typed_ast::Expr {
                    ty,
                    loc,
                    kind: typed_ast::ExprKind::Case(Box::new(matched), arms),
                };
            }
            ExprKind::Assign(place, value) => {
                let value = self.check_expr(*value, None);
                let place = self.check_place(place, Some(value.ty.clone()));
                typed_ast::Expr {
                    loc,
                    ty: Type::Unit,
                    kind: typed_ast::ExprKind::Assign(place, Box::new(value)),
                }
            }
        };
        if let Some(expected) = expected_ty {
            expr.ty = self.unify(expected, expr.ty, expr.loc.clone())
        };
        expr
    }
}
