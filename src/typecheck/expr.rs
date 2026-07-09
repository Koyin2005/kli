use std::collections::{HashMap, HashSet};

use crate::{
    ast::BinaryOp,
    collect::TypeDefKind,
    def_ids::DefId,
    index_vec::IndexVec,
    resolved_ast::{
        BlockBody, BorrowExpr, Expr, ExprKind, FieldInit, FunctionDefId, Lambda, LocalRegionId,
        Pattern, Var,
    },
    src_loc::SrcLoc,
    typecheck::root::{FunctionCtxt, TypeCheck},
    typed_ast::{self, Capture, FieldId, RecordFieldInit},
    types::{FieldName, FunctionSig, FunctionType, GenericArgs, PointerType, RecordField, Type},
};

impl FunctionCtxt<'_> {
    fn check_place(&self, place: &Expr, expected_ty: Option<Type>) -> typed_ast::Place {
        let (ty, kind) = match &place.kind {
            &ExprKind::Var(var) => (
                self.root().var_type(var.1).clone(),
                if self
                    .root()
                    .ctxt()
                    .captures(self.id)
                    .is_some_and(|captures| captures.captured(var.1))
                {
                    typed_ast::PlaceKind::Upvar(self.id, var)
                } else {
                    typed_ast::PlaceKind::Var(var)
                },
            ),
            ExprKind::Deref(value) => {
                let value = self.check_expr(value, None);
                (
                    match self
                        .root()
                        .simplify_type(value.ty.clone())
                        .into_pointer_type(self.root().ctxt())
                    {
                        Ok((PointerType::Raw | PointerType::Reference(..), ty)) => ty.clone(),
                        Ok((p, ty)) => self.root().non_deref_error(
                            &Type::pointer_type(p, ty, self.root().ctxt()),
                            value.loc,
                        ),
                        Err(ty) => self.root().non_deref_error(&ty, value.loc),
                    },
                    typed_ast::PlaceKind::Deref(Box::new(value)),
                )
            }
            ExprKind::Field(receiver, field) => {
                let receiver = self.check_place(receiver, None);
                if let Some((id, named_info, field_ty)) =
                    self.check_field(place.loc, &receiver.ty, *field)
                {
                    if let Some(field_id) = named_info {
                        let _ = self.check_field_visibility(field_id, place.loc);
                    }
                    (
                        field_ty,
                        typed_ast::PlaceKind::Field(Box::new(receiver), id),
                    )
                } else {
                    (Type::Unknown, typed_ast::PlaceKind::Invalid)
                }
            }
            _ => {
                self.root()
                    .ctxt()
                    .diag()
                    .add_diagnostic("invalid place".to_string(), place.loc);
                (Type::Unknown, typed_ast::PlaceKind::Invalid)
            }
        };
        let ty = if let Some(expected) = expected_ty {
            self.root().unify(expected, ty, place.loc)
        } else {
            ty
        };
        typed_ast::Place {
            ty,
            loc: place.loc,
            kind,
        }
    }
    fn check_field(
        &self,
        loc: SrcLoc,
        reciever_ty: &Type,
        name: crate::ident::Ident,
    ) -> Option<(FieldId, Option<DefId>, Type)> {
        let field_info = match reciever_ty {
            Type::Record(fields) => fields.iter_enumerated().find_map(|(index, field)| {
                (field.name == FieldName::Named(name.symbol))
                    .then(|| (index, None, field.ty.clone()))
            }),
            &Type::Named(id, _, ref args) => {
                let ctxt = self.root().ctxt();
                match ctxt.type_def(id).kind {
                    TypeDefKind::Record(ref fields) => {
                        fields.iter_enumerated().find_map(|(index, field)| {
                            (field.name == name.symbol)
                                .then(|| (index, Some(field.id), field.type_of(args, ctxt)))
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        };
        if field_info.is_none() {
            self.root().ctxt().diag().add_diagnostic(
                format!("'{}' does not have field '{}'", reciever_ty, name.symbol),
                loc,
            );
        }
        field_info
    }

    fn check_for_loop(
        &self,
        loc: SrcLoc,
        pattern: &Pattern,
        iterator: &Expr,
        body: &Expr,
    ) -> typed_ast::Expr {
        let iterator = self.check_expr(iterator, None);
        let element = self.root().iterator_element(iterator.ty.clone());
        let (iterator_type, element) = match element {
            Ok((iterator_type, ty)) => (Some(iterator_type), ty),
            Err(_) => {
                self.root().ctxt().diag().add_diagnostic(
                    format!("Cannot use '{}' as an iterator", iterator.ty),
                    iterator.loc,
                );
                (None, Type::Unknown)
            }
        };
        let pattern = self.check_pattern(pattern, element, None);
        let body = self.check_expr_coerces_to(body, Some(Type::Unit));
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
                pattern: Box::new(pattern),
                iterator: Box::new(iterator),
                iterator_type,
                body: Box::new(body),
            },
        }
    }
    fn check_borrow(
        &self,
        loc: SrcLoc,
        borrow: &BorrowExpr,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let &BorrowExpr {
            mutable,
            ref place,
            ref region,
        } = borrow;
        let (ty_mutable, ty_region, expected) = if let Some(ref expected) = expected_ty
            && let Ok((ty_mutable, ty_region, ty)) = expected.as_reference_type()
        {
            (Some(ty_mutable), Some(ty_region), Some(ty.clone()))
        } else {
            (None, None, None)
        };
        let place = self.check_place(place, expected);
        let region = {
            let region = self.root().lower_region(region);
            match ty_region {
                Some(expected) => self.root().unify_region(expected, region, loc),
                None => region,
            }
        };
        if let Some(ty_mutable) = ty_mutable
            && ty_mutable != mutable
        {
            self.root().ctxt().diag().add_diagnostic(
                format!("Expected a '{}' but got '{}'", ty_mutable, mutable),
                loc,
            );
        }
        if let Some(expected) = expected_ty
            && expected.as_reference_type().is_err()
        {
            self.root().expect_ty_error("reference", &expected, loc);
        }
        typed_ast::Expr {
            ty: Type::reference(place.ty.clone(), mutable, region),
            loc,
            kind: typed_ast::ExprKind::Borrow {
                mutable,
                region,
                place,
            },
        }
    }
    fn check_lambda(
        &self,
        loc: SrcLoc,
        id: DefId,
        lambda: &Lambda,
        hint: Option<Type>,
    ) -> typed_ast::Expr {
        let expected_sig = match hint.clone().map(|ty| self.root().simplify_type(ty)) {
            Some(Type::Function(function)) => Some(function),
            _ => None,
        };
        let params = lambda
            .params
            .iter()
            .map(|param| typed_ast::LambdaParam {
                loc: param.loc,
                var: param.var,
            })
            .collect::<Vec<_>>();
        let root = self.root();
        let sig = FunctionSig::new(
            lambda
                .param_tys
                .iter()
                .enumerate()
                .map(|(i, param)| {
                    let param = param.as_ref().map(|param| root.lower_type(param));
                    let param_ty = expected_sig
                        .as_ref()
                        .and_then(|sig| sig.params.get(i))
                        .cloned();
                    let loc = lambda.params[i].loc;
                    match (param, param_ty) {
                        (None, None) => root.fresh_ty(loc),
                        (Some(ty), None) | (None, Some(ty)) => ty,
                        (Some(ty), Some(expected)) => root.unify(expected, ty, loc),
                    }
                })
                .collect(),
            if let Some(ty) = expected_sig.as_ref().map(|sig| &*sig.return_type).cloned() {
                ty
            } else {
                root.fresh_ty(lambda.body.loc)
            },
        );
        let capture_map = self.root().ctxt().captures(id).unwrap_or_default();
        let captures = capture_map
            .into_vars()
            .into_iter()
            .map(|capture| Capture {
                var: Var(self.root().var_name(capture), capture),
                ty: self.root().var_type(capture),
            })
            .collect::<Vec<_>>();
        TypeCheck::check_function(
            &mut FunctionCtxt::new(root, id, sig.return_type.clone()),
            captures
                .iter()
                .map(|capture| {
                    (
                        capture.var.ident(lambda.loc),
                        self.root().var_type(capture.var.1),
                    )
                })
                .collect(),
            sig.clone(),
            &lambda.params,
            Some(&lambda.body),
        );
        let function = Type::Function(FunctionType {
            resource: lambda.resource,
            params: sig.params.clone(),
            return_type: Box::new(sig.return_type.clone()),
        });
        typed_ast::Expr {
            ty: function,
            loc,
            kind: typed_ast::ExprKind::Lambda(Box::new(typed_ast::Lambda {
                loc: lambda.loc,
                id,
                captures,
                is_resource: lambda.resource,
                params,
                param_tys: sig.params,
                return_type: Box::new(sig.return_type),
            })),
        }
    }
    fn check_call(
        &self,
        loc: SrcLoc,
        callee: &Expr,
        args: &[Expr],
        ty_hint: Option<Type>,
    ) -> typed_ast::Expr {
        fn check_call_sig(
            this: &FunctionCtxt,
            callee_loc: SrcLoc,
            args: &[Expr],
            params: Vec<Type>,
            return_type: Option<Type>,
        ) -> (Type, Vec<typed_ast::Expr>) {
            if params.len() != args.len() {
                this.root().ctxt().diag().add_diagnostic(
                    format!(
                        "Expected '{}' arguments but got '{}'",
                        params.len(),
                        args.len()
                    ),
                    callee_loc,
                );
            }

            let arg_map = |(arg, expected_ty)| this.check_expr(arg, expected_ty);
            let args = if args.len() > params.len() {
                let diff = args.len() - params.len();
                args.iter()
                    .zip(
                        params
                            .into_iter()
                            .map(Some)
                            .chain(std::iter::repeat_n(None, diff)),
                    )
                    .map(arg_map)
                    .collect::<Vec<_>>()
            } else {
                args.iter()
                    .zip(params.into_iter().map(Some))
                    .map(arg_map)
                    .collect::<Vec<_>>()
            };
            let ty = return_type.unwrap_or(Type::Unknown);
            (ty, args)
        }
        if let ExprKind::Function(id, generic_args) = &callee.kind
            && let Some(builtin) = self.root().ctxt().builtins().builtin_for(id.0)
        {
            let generic_args = self.root().lower_generic_args_for(id.0, loc, generic_args);
            let FunctionSig {
                params,
                return_type,
            } = self.root().ctxt().signature_of(id.0).bind(&generic_args);
            let (ty, args) = check_call_sig(self, callee.loc, args, params, Some(return_type));
            typed_ast::Expr {
                ty,
                loc,
                kind: typed_ast::ExprKind::BuiltinCall(
                    builtin,
                    generic_args,
                    args.into_boxed_slice(),
                ),
            }
        } else if let ExprKind::VariantCase(variant_id, generic_args) = &callee.kind
            && let root = self.root()
            && let ctxt = root.ctxt()
            && let ty_id = ctxt.expect_parent(*variant_id)
            && let cases = ctxt.type_def(ty_id).cases()
            && let (index, case_def) = cases
                .iter_enumerated()
                .find_map(|(index, case)| (case.id == *variant_id).then_some((index, case)))
                .unwrap()
            && case_def.field.is_some()
            && args.len() == case_def.field.as_slice().len()
        {
            let generic_args = root.lower_generic_args_for(*variant_id, loc, generic_args);
            let Type::Function(FunctionType {
                resource: _,
                params,
                return_type,
            }) = ctxt.type_of(*variant_id).bind(&generic_args)
            else {
                unreachable!("Should be a function")
            };
            let (ty, args) = check_call_sig(self, callee.loc, args, params, Some(*return_type));
            let [arg] = args.try_into().unwrap();
            typed_ast::Expr {
                ty,
                loc,
                kind: typed_ast::ExprKind::VariantInit(ty_id, index, generic_args, Box::new(arg)),
            }
        } else {
            let callee = self.check_expr(callee, None);
            let callee_type = self.root().simplify_type(callee.ty.clone());
            let (params, return_type) = match callee_type {
                Type::Function(FunctionType {
                    resource: _,
                    params,
                    return_type,
                }) => (params, Some(*return_type)),
                ty => {
                    self.root().expect_ty_error("function", &ty, callee.loc);
                    (Vec::new(), ty_hint)
                }
            };
            let (ty, args) = check_call_sig(self, callee.loc, args, params, return_type);
            typed_ast::Expr {
                ty,
                loc,
                kind: typed_ast::ExprKind::Call(Box::new(callee), args),
            }
        }
    }
    fn check_block(
        &self,
        loc: SrcLoc,
        body: &BlockBody,
        region: Option<LocalRegionId>,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let stmts = body
            .stmts
            .iter()
            .map(|stmt| self.check_stmt(stmt))
            .collect();
        let expr = self.check_expr_coerces_to(&body.expr, expected_ty);
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
        &self,
        loc: SrcLoc,
        field_inits: &[FieldInit],
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let expected_fields = match expected_ty.map(|ty| self.root().simplify_type(ty)) {
            Some(Type::Record(fields)) => Some(fields),
            _ => None,
        };
        let mut seen_fields = HashSet::new();
        let field_names = expected_fields
            .iter()
            .flatten()
            .enumerate()
            .map(|(i, field)| (field.name, i))
            .collect::<HashMap<_, _>>();

        let expr_fields = field_inits
            .iter()
            .enumerate()
            .filter_map(|(i, &FieldInit { name, ref value })| {
                let field_id = field_names
                    .get(&FieldName::Named(name.symbol))
                    .copied()
                    .map(FieldId::new);
                let value = self.check_expr_coerces_to(
                    value,
                    expected_fields
                        .as_ref()
                        .and_then(|fields| field_id.map(|field| fields[field].ty.clone())),
                );
                if expected_fields.is_some() && !seen_fields.insert(name.symbol) {
                    self.root()
                        .ctxt()
                        .diag()
                        .add_diagnostic(format!("Repeated field '{}'", name.symbol), name.loc);
                    return None;
                }
                let field_id = if let Some(field_id) = field_id {
                    field_id
                } else if expected_fields.is_some() {
                    self.root().ctxt().diag().add_diagnostic(
                        format!("'record' has no field '{}'", name.symbol),
                        name.loc,
                    );
                    return None;
                } else {
                    FieldId::new(i)
                };
                Some(RecordFieldInit {
                    index: field_id,
                    value,
                })
            })
            .collect::<Box<[_]>>();
        let record_fields = if let Some(fields) = expected_fields {
            let _ = self.root().check_missing_fields(
                loc,
                seen_fields,
                fields.iter().map(|field| field.name),
            );
            fields
        } else {
            expr_fields
                .iter()
                .zip(field_inits.iter().map(|field| field.name.symbol))
                .map(|(field, name)| RecordField {
                    name: FieldName::Named(name),
                    ty: field.value.ty.clone(),
                })
                .collect()
        };
        typed_ast::Expr {
            ty: Type::Record(record_fields),
            loc,
            kind: typed_ast::ExprKind::Record(expr_fields),
        }
    }
    pub(super) fn check_expr_coerces_to(
        &self,
        expr: &Expr,
        target: Option<Type>,
    ) -> typed_ast::Expr {
        let mut expr = self.check_expr_kind(expr, target.clone());
        if let Some(target) = target {
            match self.unify_or_coerce(expr.loc, target.clone(), expr.ty.clone()) {
                Ok(coercion) => expr = self.apply_coercion(coercion, expr),
                Err(_) => {
                    expr = typed_ast::Expr {
                        ty: target,
                        loc: expr.loc,
                        kind: typed_ast::ExprKind::Err,
                    };
                }
            }
        }
        expr
    }
    pub(super) fn check_expr_kind(
        &self,
        expr: &Expr,
        expected_ty: Option<Type>,
    ) -> typed_ast::Expr {
        let &Expr { loc, ref kind } = expr;
        let make_expr = |ty, kind, loc| typed_ast::Expr { ty, kind, loc };
        match kind {
            ExprKind::Return(return_expr) => {
                let value = self.check_expr_coerces_to(return_expr, Some(self.return_type.clone()));
                make_expr(
                    Type::Never,
                    typed_ast::ExprKind::Return(Box::new(value)),
                    loc,
                )
            }
            ExprKind::While(condition, body) => {
                let condition = self.check_expr_coerces_to(condition, Some(Type::Bool));
                let body = self.check_expr_coerces_to(body, Some(Type::Unit));
                typed_ast::Expr {
                    ty: Type::Unit,
                    loc,
                    kind: typed_ast::ExprKind::While(Box::new(condition), Box::new(body)),
                }
            }
            ExprKind::Record(fields) => self.check_record(loc, fields, expected_ty.clone()),
            ExprKind::Block(block, region) => self.check_block(loc, block, *region, expected_ty),
            ExprKind::Annotate(expr, ty) => self.check_expr(expr, Some(self.root().lower_type(ty))),
            ExprKind::Err => typed_ast::Expr {
                loc,
                ty: Type::Unknown,
                kind: typed_ast::ExprKind::Err,
            },
            &ExprKind::Bool(value) => typed_ast::Expr {
                loc,
                ty: Type::Bool,
                kind: typed_ast::ExprKind::Bool(value),
            },
            &ExprKind::Function(FunctionDefId(id), ref args) => {
                let args = self.root().lower_generic_args_for(id, loc, args);
                make_expr(
                    Type::Function(
                        self.root()
                            .ctxt()
                            .signature_of(id)
                            .bind(&args)
                            .into_function_type(),
                    ),
                    if let Some(builtin) = self.root().ctxt().builtins().builtin_for(id) {
                        self.root().ctxt().diag().add_diagnostic(
                            format!("cannot use builtin '{}' here", builtin.name()),
                            loc,
                        );
                        typed_ast::ExprKind::Err
                    } else {
                        typed_ast::ExprKind::Function(id, args)
                    },
                    loc,
                )
            }
            &ExprKind::VariantCase(case_id, ref args) => {
                let args = self.root().lower_generic_args_for(case_id, loc, args);
                let ty = self.root().ctxt().type_of(case_id).bind(&args);
                typed_ast::Expr {
                    ty,
                    loc,
                    kind: typed_ast::ExprKind::Const(case_id, args),
                }
            }
            ExprKind::Var(_) | ExprKind::Deref(_) | ExprKind::Field(..) => {
                let place = self.check_place(expr, expected_ty);
                typed_ast::Expr {
                    ty: place.ty.clone(),
                    loc,
                    kind: typed_ast::ExprKind::Load(place),
                }
            }
            ExprKind::Print(arg) => {
                let arg = arg.as_ref().map(|arg| Box::new(self.check_expr(arg, None)));
                make_expr(Type::Unit, typed_ast::ExprKind::Print(arg), loc)
            }
            ExprKind::Unit => make_expr(Type::Unit, typed_ast::ExprKind::Unit, loc),
            ExprKind::Int(value) => make_expr(Type::Int, typed_ast::ExprKind::Int(*value), loc),
            ExprKind::String(value) => make_expr(
                Type::String,
                typed_ast::ExprKind::String(value.clone()),
                loc,
            ),
            ExprKind::Call(callee, args) => self.check_call(loc, callee, args, expected_ty),
            ExprKind::Panic => typed_ast::Expr {
                loc,
                kind: typed_ast::ExprKind::Panic,
                ty: Type::Never,
            },
            ExprKind::AddressOf(place) => {
                let place = self.check_place(
                    place,
                    if let Some(ref ty) = expected_ty
                        && let Type::RawPointer(ty) = ty
                    {
                        Some((**ty).clone())
                    } else {
                        None
                    },
                );
                typed_ast::Expr {
                    ty: Type::pointer(place.ty.clone()),
                    loc,
                    kind: typed_ast::ExprKind::AddressOf(Box::new(place)),
                }
            }
            ExprKind::Binary(binary_op, left, right) => {
                let ((left_ty, right_ty), result) = match binary_op {
                    BinaryOp::Add | BinaryOp::Divide | BinaryOp::Multiply | BinaryOp::Subtract => {
                        ((Some(Type::Int), Some(Type::Int)), Type::Int)
                    }
                    BinaryOp::Equals => ((None, None), Type::Bool),
                    BinaryOp::Lesser | BinaryOp::Greater => {
                        ((Some(Type::Int), Some(Type::Int)), Type::Bool)
                    }
                    BinaryOp::And => ((Some(Type::Bool), Some(Type::Bool)), Type::Bool),
                };
                let left = self.check_expr(left, left_ty);
                let right = self.check_expr(right, right_ty);
                match (binary_op, &left.ty, &right.ty) {
                    (
                        BinaryOp::Add
                        | BinaryOp::Divide
                        | BinaryOp::Subtract
                        | BinaryOp::Multiply
                        | BinaryOp::Greater
                        | BinaryOp::Lesser,
                        Type::Int,
                        Type::Int,
                    )
                    | (BinaryOp::And, Type::Bool, Type::Bool)
                    | (BinaryOp::Equals, Type::Int, Type::Int)
                    | (BinaryOp::Equals, Type::Byte, Type::Byte)
                    | (BinaryOp::Equals, Type::Char, Type::Char) => (),
                    (BinaryOp::Equals, Type::RawPointer(ty1), Type::RawPointer(ty2)) => {
                        let _ = self.root().unify((**ty1).clone(), (**ty2).clone(), loc);
                    }
                    (_, left, right) => {
                        self.ctxt().diag().add_diagnostic(
                            format!(
                                "'{left}' and '{right}' are invalid operands for '{binary_op}'"
                            ),
                            loc,
                        );
                    }
                }
                fn binary_kind(
                    op: typed_ast::BinaryOp,
                    left: typed_ast::Expr,
                    right: typed_ast::Expr,
                ) -> typed_ast::ExprKind {
                    typed_ast::ExprKind::Binary(op, Box::new(left), Box::new(right))
                }
                typed_ast::Expr {
                    loc,
                    ty: result,
                    kind: match *binary_op {
                        BinaryOp::Add => binary_kind(typed_ast::BinaryOp::Add, left, right),
                        BinaryOp::Subtract => {
                            binary_kind(typed_ast::BinaryOp::Subtract, left, right)
                        }
                        BinaryOp::Multiply => {
                            binary_kind(typed_ast::BinaryOp::Multiply, left, right)
                        }
                        BinaryOp::Divide => binary_kind(typed_ast::BinaryOp::Divide, left, right),
                        BinaryOp::Equals => binary_kind(typed_ast::BinaryOp::Equals, left, right),
                        BinaryOp::Lesser => binary_kind(typed_ast::BinaryOp::Lesser, left, right),
                        BinaryOp::Greater => binary_kind(typed_ast::BinaryOp::Greater, left, right),
                        BinaryOp::And => typed_ast::ExprKind::Logic(
                            typed_ast::LogicalOp::And,
                            Box::new(left),
                            Box::new(right),
                        ),
                    },
                }
            }
            ExprKind::Lambda(lambda) => {
                self.check_lambda(loc, lambda.id, lambda, expected_ty.clone())
            }
            ExprKind::Borrow(borrow) => self.check_borrow(loc, borrow, expected_ty),
            ExprKind::For(for_expr) => {
                self.check_for_loop(loc, &for_expr.pattern, &for_expr.iterator, &for_expr.body)
            }
            ExprKind::Case(matched, case_arms) => {
                let matched = self.check_expr(matched, None);
                let mut arms = case_arms
                    .iter()
                    .map(|arm| {
                        let pattern = self.check_pattern(&arm.pattern, matched.ty.clone(), None);
                        let body = self.check_expr_coerces_to(&arm.body, expected_ty.clone());
                        typed_ast::CaseArm { pattern, body }
                    })
                    .collect::<Vec<_>>();
                let combined_ty = self.merge_ty(arms.iter().map(|arm| arm.body.ty.clone()));
                let ty = if let Some(combined_ty) = combined_ty {
                    arms = arms
                        .into_iter()
                        .map(|mut arm| {
                            let Ok(coercion) = self.unify_or_coerce(
                                arm.pattern.loc,
                                combined_ty.clone(),
                                arm.body.ty.clone(),
                            ) else {
                                return arm;
                            };
                            arm.body = self.apply_coercion(coercion, arm.body);
                            arm
                        })
                        .collect();
                    combined_ty
                } else if arms.is_empty() {
                    Type::Never
                } else {
                    self.root().type_annotations_needed(loc);
                    Type::Unknown
                };
                typed_ast::Expr {
                    ty,
                    loc,
                    kind: typed_ast::ExprKind::Case(Box::new(matched), arms),
                }
            }
            ExprKind::Assign(place, value) => {
                let place = self.check_place(place, None);
                let value = self.check_expr_coerces_to(value, Some(place.ty.clone()));
                typed_ast::Expr {
                    loc,
                    ty: Type::Unit,
                    kind: typed_ast::ExprKind::Assign(Box::new(place), Box::new(value)),
                }
            }
            &ExprKind::NamedRecord(name, ref args, ref fields) => {
                let (info, args) = match self.root().lower_type_name(loc, name, args) {
                    Type::Named(id, name, args) => (Ok((id, name)), args),
                    ty => {
                        self.ctxt()
                            .diag()
                            .add_diagnostic(format!("Cannot construct '{}'", ty), loc);
                        (Err(ty), GenericArgs::new())
                    }
                };
                let field_info = if let Ok((id, _)) = info {
                    if !self.ctxt().same_module(id, self.id) && !self.ctxt().is_opaque(self.id) {
                        self.ctxt().diag().add_diagnostic(
                            format!(
                                "Cannot construct '{}' in this scope",
                                self.ctxt().display(id)
                            ),
                            loc,
                        );
                    }
                    let ty_def = self.root().ctxt().type_def(id);
                    match ty_def.kind {
                        TypeDefKind::Record(fields) => fields,
                        TypeDefKind::Variant(_) => {
                            self.root().ctxt().diag().add_diagnostic(
                                format!("Cannot record init 'variant {}'", ty_def.name),
                                loc,
                            );
                            IndexVec::new()
                        }
                    }
                } else {
                    IndexVec::new()
                };

                let field_map = field_info
                    .iter_enumerated()
                    .map(|(i, &field)| (field.name, (i, field)))
                    .collect::<HashMap<_, _>>();
                let mut seen_fields = HashSet::new();
                let fields = fields
                    .iter()
                    .filter_map(|field| {
                        let field_info = field_map.get(&field.name.symbol).copied();
                        if !seen_fields.insert(field.name.symbol) {
                            self.root().ctxt().diag().add_diagnostic(
                                format!("Repeated field '{}'", field.name.symbol),
                                field.name.loc,
                            );
                        }
                        let value = self.check_expr_coerces_to(
                            &field.value,
                            field_info
                                .as_ref()
                                .map(|(_, field)| field.type_of(&args, self.root().ctxt())),
                        );
                        let (id, _) = if let Some(field_info) = field_info {
                            field_info
                        } else {
                            self.ctxt().diag().add_diagnostic(
                                format!("Unknown field '{}'", field.name.symbol),
                                field.name.loc,
                            );
                            return None;
                        };
                        Some(RecordFieldInit { index: id, value })
                    })
                    .collect();
                let _ = self.root().check_missing_fields(
                    expr.loc,
                    seen_fields,
                    field_info.iter().map(|field| FieldName::Named(field.name)),
                );
                match info {
                    Ok((id, name)) => typed_ast::Expr {
                        ty: Type::Named(id, name, args.clone()),
                        loc,
                        kind: typed_ast::ExprKind::NamedRecord(id, args, fields),
                    },
                    Err(ty) => typed_ast::Expr {
                        ty,
                        loc,
                        kind: typed_ast::ExprKind::Err,
                    },
                }
            }
        }
    }
    pub(super) fn check_expr(&self, expr: &Expr, expected_ty: Option<Type>) -> typed_ast::Expr {
        let mut expr = self.check_expr_kind(expr, expected_ty.clone());
        if let Some(expected) = expected_ty {
            expr.ty = self.root().unify(expected, expr.ty, expr.loc)
        };
        expr
    }
}
