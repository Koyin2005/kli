use std::collections::{HashMap, HashSet};

use crate::{
    ast::BinaryOp,
    collect::TypeDefKind,
    def_ids::DefId,
    index_vec::IndexVec,
    resolved_ast::{self, BlockBody, Expr, ExprKind, FieldInit, FunctionDefId, Lambda, Var},
    src_loc::SrcLoc,
    typecheck::{
        coercion::Coercion,
        root::{FunctionCtxt, TypeCheck},
    },
    typed_ast::{self, Capture, FieldId, RecordFieldInit},
    types::{FieldName, FunctionSig, GenericArgs, IntegerSize, RecordField, Type, TypeKind},
};

impl<'root, 'ctxt> FunctionCtxt<'root, 'ctxt> {
    fn check_place(
        &self,
        place: &Expr,
        expected_ty: Option<Type<'ctxt>>,
    ) -> typed_ast::Place<'ctxt> {
        let (ty, kind) = match &place.kind {
            &ExprKind::Var(var) => (
                self.root().simplify_type(self.root().var_type(var.1)),
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
            &ExprKind::Field(ref receiver, field) => {
                let receiver = self.check_place(receiver, None);
                if let Some((id, named_info, field_ty)) =
                    self.check_field(place.loc, receiver.ty, field)
                {
                    if let Some(field_id) = named_info {
                        let _ = self.check_field_visibility(field_id, place.loc);
                    }
                    (
                        field_ty,
                        typed_ast::PlaceKind::Field(Box::new(receiver), id),
                    )
                } else {
                    (
                        Type::new_unknown(self.ctxt()),
                        typed_ast::PlaceKind::Invalid,
                    )
                }
            }
            ExprKind::Deref(boxed_value) => {
                let boxed_value = self.check_expr(
                    boxed_value,
                    expected_ty.map(|ty| Type::new_box(self.ctxt(), ty)),
                );
                let ty = if let Some(ty) = boxed_value.ty.as_box() {
                    ty
                } else {
                    self.ctxt().diag().add_diagnostic(
                        format!("Expected 'box' but got '{}'", boxed_value.ty),
                        place.loc,
                    );
                    Type::new_unknown(self.ctxt())
                };

                (ty, typed_ast::PlaceKind::Deref(Box::new(boxed_value)))
            }
            ExprKind::Index(reciever, index) => {
                let receiver = self.check_expr(reciever, None);
                let index = self.check_expr_coerces_to(
                    index,
                    Some(Type::new_uint(self.ctxt(), IntegerSize::Int64)),
                );
                let element_ty = receiver.ty.as_array().unwrap_or_else(|| {
                    self.ctxt().diag().add_diagnostic(
                        format!("Expected an array type but got '{}'", receiver.ty),
                        receiver.loc,
                    );
                    Type::new_unknown(self.ctxt())
                });
                (
                    element_ty,
                    typed_ast::PlaceKind::Index(Box::new(receiver), Box::new(index)),
                )
            }
            _ => {
                self.root()
                    .ctxt()
                    .diag()
                    .add_diagnostic("invalid place", place.loc);
                (
                    Type::new_unknown(self.ctxt()),
                    typed_ast::PlaceKind::Invalid,
                )
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
        reciever_ty: Type<'ctxt>,
        name: crate::ident::Ident,
    ) -> Option<(FieldId, Option<DefId>, Type<'ctxt>)> {
        let field_info = match reciever_ty.kind() {
            &TypeKind::Named(id, _, ref args) => {
                let ctxt = self.root().ctxt();
                match ctxt.type_def(id).kind {
                    TypeDefKind::Record(ref fields) => {
                        fields.iter_enumerated().find_map(|(index, field)| {
                            (field.name == name.symbol).then_some((
                                index,
                                Some(field.id),
                                field.type_of(args, ctxt),
                            ))
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
        for_expr: &resolved_ast::ForExpr,
    ) -> typed_ast::Expr<'ctxt> {
        let resolved_ast::ForExpr {
            iterator,
            pattern,
            body,
        } = for_expr;
        let iterator = self.check_expr(iterator, None);
        let element = self.root().iterator_element(iterator.ty);
        let (iterator_type, element) = match element {
            Ok((iterator_type, ty)) => (Some(iterator_type), ty),
            Err(_) => {
                self.root().ctxt().diag().add_diagnostic(
                    format!("Cannot use '{}' as an iterator", iterator.ty),
                    iterator.loc,
                );
                (None, Type::new_unknown(self.ctxt()))
            }
        };
        let pattern = self.check_pattern(pattern, element);
        let body = self.check_expr_coerces_to(body, Some(Type::new_unit(self.ctxt())));
        let Some(iterator_type) = iterator_type else {
            return typed_ast::Expr {
                ty: Type::new_unit(self.ctxt()),
                loc,
                kind: typed_ast::ExprKind::Err,
            };
        };
        typed_ast::Expr {
            ty: Type::new_unit(self.ctxt()),
            loc,
            kind: typed_ast::ExprKind::For {
                pattern: Box::new(pattern),
                iterator: Box::new(iterator),
                iterator_type,
                body: Box::new(body),
            },
        }
    }
    fn check_lambda(
        &self,
        loc: SrcLoc,
        id: DefId,
        lambda: &Lambda,
        hint: Option<Type<'ctxt>>,
    ) -> typed_ast::Expr<'ctxt> {
        let expected_sig = match hint.map(|ty| self.root().simplify_type(ty)).as_deref() {
            Some(TypeKind::Function(function)) => Some(function.clone()),
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
            lambda.param_tys.iter().enumerate().map(|(i, param)| {
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
            }),
            if let Some(ty) = expected_sig.as_ref().map(|sig| sig.return_type) {
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
            &mut FunctionCtxt::new(root, id, sig.return_type),
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
        let function = sig.clone().into_type(self.ctxt());
        if !captures.is_empty() {
            self.ctxt()
                .diag()
                .add_diagnostic("Cannot capture".to_string(), loc);
        }
        typed_ast::Expr {
            ty: function,
            loc,
            kind: typed_ast::ExprKind::Lambda(Box::new(typed_ast::Lambda {
                loc: lambda.loc,
                id,
                params,
                param_tys: sig.params,
                return_type: sig.return_type,
            })),
        }
    }
    fn check_call_sig(
        &self,
        callee_loc: SrcLoc,
        args: &[Expr],
        params: Vec<Type<'ctxt>>,
        return_type: Option<Type<'ctxt>>,
    ) -> (Type<'ctxt>, Vec<typed_ast::Expr<'ctxt>>) {
        if params.len() != args.len() {
            self.root().ctxt().diag().add_diagnostic(
                format!(
                    "Expected '{}' arguments but got '{}'",
                    params.len(),
                    args.len()
                ),
                callee_loc,
            );
        }

        let arg_map = |(arg, expected_ty)| self.check_expr(arg, expected_ty);
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
                .zip(params.iter().copied().map(Some))
                .map(arg_map)
                .collect::<Vec<_>>()
        };
        let ty = return_type.unwrap_or(Type::new_unknown(self.ctxt()));
        (ty, args)
    }
    fn check_call(
        &self,
        loc: SrcLoc,
        callee: &Expr,
        args: &[Expr],
        ty_hint: Option<Type<'ctxt>>,
    ) -> typed_ast::Expr<'ctxt> {
        if let ExprKind::Function(id, generic_args) = &callee.kind
            && let Some(builtin) = self.root().ctxt().builtin_for(id.0)
        {
            let ctxt = self.ctxt();
            let generic_args = self.root().lower_generic_args_for(id.0, loc, generic_args);
            let sig = ctxt.signature_of(id.0).bind(self.ctxt(), &generic_args);
            let FunctionSig {
                params,
                return_type,
            } = sig;

            let (ty, args) = self.check_call_sig(callee.loc, args, params, Some(return_type));
            typed_ast::Expr {
                ty,
                loc,
                kind: typed_ast::ExprKind::BuiltinCall(
                    id.0,
                    builtin,
                    generic_args,
                    args.into_boxed_slice(),
                ),
            }
        } else if let ExprKind::VariantCase(variant_id, generic_args) = &callee.kind
            && let root = self.root()
            && let ty_id = root.ctxt().expect_parent(*variant_id)
            && let cases = root.ctxt().type_def(ty_id).expect_cases()
            && let (index, case_def) = cases
                .iter_enumerated()
                .find_map(|(index, case)| (case.id == *variant_id).then_some((index, case)))
                .unwrap()
            && case_def.field.is_some()
            && args.len() == case_def.field.as_slice().len()
        {
            let generic_args = root.lower_generic_args_for(*variant_id, loc, generic_args);
            let ty = root
                .ctxt()
                .type_of(*variant_id)
                .bind(self.ctxt(), &generic_args);
            let FunctionSig {
                params,
                return_type,
            } = ty.as_function().unwrap();
            let (ty, args) =
                self.check_call_sig(callee.loc, args, params.clone(), Some(*return_type));
            let [arg] = args.try_into().unwrap();
            typed_ast::Expr {
                ty,
                loc,
                kind: typed_ast::ExprKind::VariantInit(
                    ty_id,
                    index,
                    generic_args,
                    Some(Box::new(arg)),
                ),
            }
        } else {
            let callee = self.check_expr(callee, None);
            let callee_type = self.root().simplify_type(callee.ty);
            let (params, return_type) = if let Some(function) = callee_type.as_function() {
                (function.params.clone(), Some(function.return_type))
            } else {
                self.root()
                    .expect_ty_error("function", callee_type, callee.loc);
                (Vec::new(), ty_hint)
            };
            let (ty, args) = self.check_call_sig(callee.loc, args, params, return_type);
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
        expected_ty: Option<Type<'ctxt>>,
    ) -> typed_ast::Expr<'ctxt> {
        let stmts = body
            .stmts
            .iter()
            .map(|stmt| self.check_stmt(stmt))
            .collect();
        let expr = self.check_expr_coerces_to(&body.expr, expected_ty);
        let ty: Type<'ctxt> = expr.ty;
        let body = typed_ast::BlockBody {
            stmts,
            expr: Box::new(expr),
        };
        typed_ast::Expr::<'ctxt> {
            ty,
            loc,
            kind: typed_ast::ExprKind::Block(body),
        }
    }
    fn check_record(&self, loc: SrcLoc, field_inits: &[FieldInit]) -> typed_ast::Expr<'ctxt> {
        let expected_fields: Option<IndexVec<FieldId, RecordField>> = None;
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
                        .and_then(|fields| field_id.map(|field| fields[field].ty)),
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
                    ty: field.value.ty,
                })
                .collect()
        };
        _ = record_fields;
        _ = expr_fields;
        todo!("remove me")
    }
    fn check_binary_op(
        &self,
        loc: SrcLoc,
        binary_op: BinaryOp,
        left: &Expr,
        right: &Expr,
        expected_ty: Option<Type<'ctxt>>,
    ) -> typed_ast::Expr<'ctxt> {
        let bool_ty = Type::new_bool(self.ctxt());
        let (left_ty, right_ty) = match binary_op {
            BinaryOp::Add | BinaryOp::Divide | BinaryOp::Multiply | BinaryOp::Subtract => {
                let operand_tys = expected_ty
                    .filter(|ty| ty.is_integer_or_int_var())
                    .map(|ty| self.root().simplify_type(ty));
                (operand_tys, operand_tys)
            }
            BinaryOp::Bor | BinaryOp::Band => {
                let operand_tys = expected_ty
                    .filter(|ty| ty.is_integer_or_int_var() || ty.is_bool())
                    .map(|ty| self.root().simplify_type(ty));
                (operand_tys, operand_tys)
            }
            BinaryOp::Equals | BinaryOp::Lesser | BinaryOp::Greater => (None, None),
            BinaryOp::And | BinaryOp::Or => (Some(bool_ty), Some(bool_ty)),
        };
        let left = self.check_expr(left, left_ty);
        let right = self.check_expr(right, right_ty);
        let result_ty = match binary_op {
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => self
                .root()
                .try_unify(left.ty, right.ty)
                .and_then(|result| result.is_integer_or_int_var().then_some(result)),
            BinaryOp::Equals | BinaryOp::Lesser | BinaryOp::Greater => {
                self.root().try_unify(left.ty, right.ty).and_then(|result| {
                    (result.is_int_var() || result.is_builtin_scalar()).then_some(bool_ty)
                })
            }
            BinaryOp::And | BinaryOp::Or => Some(bool_ty),
            BinaryOp::Bor | BinaryOp::Band => {
                self.root().try_unify(left.ty, right.ty).and_then(|result| {
                    (result.is_integer_or_int_var() || result.is_bool()).then_some(result)
                })
            }
        };
        let result = result_ty.unwrap_or_else(|| {
            self.ctxt().diag().add_diagnostic(
                format!(
                    "invalid operands '{}' and '{}' for '{binary_op}'",
                    left.ty, right.ty
                ),
                loc,
            );
            Type::new_unknown(self.ctxt())
        });
        let op = match binary_op {
            BinaryOp::Add => Ok(typed_ast::BinaryOp::Add),
            BinaryOp::Subtract => Ok(typed_ast::BinaryOp::Subtract),
            BinaryOp::Multiply => Ok(typed_ast::BinaryOp::Multiply),
            BinaryOp::Divide => Ok(typed_ast::BinaryOp::Divide),
            BinaryOp::Equals => Ok(typed_ast::BinaryOp::Equals),
            BinaryOp::Lesser => Ok(typed_ast::BinaryOp::Lesser),
            BinaryOp::Greater => Ok(typed_ast::BinaryOp::Greater),
            BinaryOp::Or => Err(typed_ast::LogicalOp::Or),
            BinaryOp::And => Err(typed_ast::LogicalOp::And),
            BinaryOp::Bor => Ok(typed_ast::BinaryOp::BitwiseOr),
            BinaryOp::Band => Ok(typed_ast::BinaryOp::BitwiseAnd),
        };
        typed_ast::Expr {
            loc,
            ty: result,
            kind: match op {
                Ok(op) => typed_ast::ExprKind::Binary(op, Box::new(left), Box::new(right)),
                Err(op) => typed_ast::ExprKind::Logic(op, Box::new(left), Box::new(right)),
            },
        }
    }
    pub(super) fn check_expr_coerces_to(
        &self,
        expr: &Expr,
        target: Option<Type<'ctxt>>,
    ) -> typed_ast::Expr<'ctxt> {
        let mut expr = self.check_expr_kind(expr, target);
        if let Some(target) = target {
            match self.unify_or_coerce(expr.loc, target, expr.ty) {
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
        expected_ty: Option<Type<'ctxt>>,
    ) -> typed_ast::Expr<'ctxt> {
        let &Expr { loc, ref kind } = expr;
        let make_expr =
            |ty, kind: typed_ast::ExprKind<'ctxt>, loc| typed_ast::Expr::<'ctxt> { ty, kind, loc };
        match kind {
            ExprKind::Unsafe(expr) => {
                let expr = self.check_expr_coerces_to(expr, expected_ty);
                make_expr(expr.ty, typed_ast::ExprKind::Unsafe(Box::new(expr)), loc)
            }
            ExprKind::Return(return_expr) => {
                let value = self.check_expr_coerces_to(return_expr, Some(self.return_type));
                make_expr(
                    Type::new_never(self.ctxt()),
                    typed_ast::ExprKind::Return(Box::new(value)),
                    loc,
                )
            }
            ExprKind::While(condition, body) => {
                let unit_ty = Type::new_unit(self.ctxt());
                let condition =
                    self.check_expr_coerces_to(condition, Some(Type::new_bool(self.ctxt())));
                let body = self.check_expr_coerces_to(body, Some(unit_ty));
                typed_ast::Expr {
                    ty: unit_ty,
                    loc,
                    kind: typed_ast::ExprKind::While(Box::new(condition), Box::new(body)),
                }
            }
            ExprKind::Tuple(fields) => {
                let expected_fields = expected_ty
                    .as_ref()
                    .and_then(|ty| match ty.kind() {
                        TypeKind::Tuple(fields) => Some(&**fields),
                        _ => None,
                    })
                    .unwrap_or(&[]);
                let fields = fields
                    .iter()
                    .enumerate()
                    .map(|(i, field)| self.check_expr(field, expected_fields.get(i).cloned()))
                    .collect::<Box<[_]>>();

                typed_ast::Expr {
                    ty: Type::tuple_from_iter(self.ctxt(), fields.iter().map(|field| field.ty)),
                    loc,
                    kind: typed_ast::ExprKind::Tuple(fields),
                }
            }
            ExprKind::Record(fields) => self.check_record(loc, fields),
            ExprKind::Block(block) => self.check_block(loc, block, expected_ty),
            ExprKind::Annotate(expr, ty) => self.check_expr(expr, Some(self.root().lower_type(ty))),
            ExprKind::Err => typed_ast::Expr {
                loc,
                ty: Type::new_unknown(self.ctxt()),
                kind: typed_ast::ExprKind::Err,
            },
            &ExprKind::Bool(value) => typed_ast::Expr {
                loc,
                ty: Type::new_bool(self.ctxt()),
                kind: typed_ast::ExprKind::Bool(value),
            },
            &ExprKind::Function(FunctionDefId(id), ref args) => {
                let ctxt = self.root().ctxt();
                let args = self.root().lower_generic_args_for(id, loc, args);
                make_expr(
                    self.root()
                        .ctxt()
                        .signature_of(id)
                        .bind(ctxt, &args)
                        .into_type(ctxt),
                    if let Some(builtin) = self.root().ctxt().builtin_for(id) {
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
                let ctxt = self.root().ctxt();
                let ty = self.root().ctxt().type_of(case_id).bind(ctxt, &args);
                let kind = if matches!(ty.kind(), TypeKind::Function(..)) {
                    typed_ast::ExprKind::Const(case_id, args)
                } else {
                    let ty_id = self.ctxt().expect_parent(case_id);
                    let (case_id, _) = self.ctxt().type_def(ty_id).case_with_id(case_id);
                    typed_ast::ExprKind::VariantInit(ty_id, case_id, args, None)
                };
                make_expr(ty, kind, loc)
            }
            &ExprKind::Char(char) => make_expr(
                Type::new_char(self.ctxt()),
                typed_ast::ExprKind::Char(char),
                loc,
            ),
            ExprKind::Var(_) | ExprKind::Field(..) | ExprKind::Index(..) | ExprKind::Deref(_) => {
                let place = self.check_place(expr, expected_ty);
                typed_ast::Expr {
                    ty: place.ty,
                    loc,
                    kind: typed_ast::ExprKind::Load(place),
                }
            }
            ExprKind::Unit => {
                let ctxt = self.root().ctxt();
                make_expr(Type::new_unit(ctxt), typed_ast::ExprKind::Unit, loc)
            }
            ExprKind::Int(value) => {
                let (ty, value) = self.root().check_int_lit(loc, expected_ty, *value);
                make_expr(ty, typed_ast::ExprKind::Int(value), loc)
            }
            ExprKind::String(value) => {
                let ctxt = self.root().ctxt();
                make_expr(
                    Type::new_string(ctxt),
                    typed_ast::ExprKind::String(value.clone()),
                    loc,
                )
            }
            ExprKind::Call(callee, args) => self.check_call(loc, callee, args, expected_ty),
            ExprKind::Panic => typed_ast::Expr {
                loc,
                kind: typed_ast::ExprKind::Panic,
                ty: Type::new_never(self.ctxt()),
            },
            ExprKind::Binary(binary_op, left, right) => {
                self.check_binary_op(loc, *binary_op, left, right, expected_ty)
            }
            ExprKind::Lambda(lambda) => self.check_lambda(loc, lambda.id, lambda, expected_ty),
            ExprKind::For(for_expr) => self.check_for_loop(loc, for_expr),
            ExprKind::Case(matched, case_arms) => {
                let matched = self.check_expr(matched, None);
                let mut patterns = Vec::with_capacity(case_arms.len());
                let mut coercion = Coercion::new(expected_ty, self);
                for arm in case_arms {
                    patterns.push(self.check_pattern(&arm.pattern, matched.ty));
                    coercion.check_expr(&arm.body);
                }
                let (combined_ty, bodies) = coercion.finish();
                let ty = if let Some(combined_ty) = combined_ty {
                    combined_ty
                } else if patterns.is_empty() {
                    Type::new_never(self.ctxt())
                } else {
                    self.root().type_annotations_needed(loc);
                    Type::new_unknown(self.ctxt())
                };
                let arms = patterns
                    .into_iter()
                    .zip(bodies)
                    .map(|(pat, body)| typed_ast::CaseArm { pattern: pat, body })
                    .collect();
                typed_ast::Expr {
                    ty,
                    loc,
                    kind: typed_ast::ExprKind::Case(Box::new(matched), arms),
                }
            }
            ExprKind::Assign(place, value) => {
                let place = self.check_place(place, None);
                let value = self.check_expr_coerces_to(value, Some(place.ty));
                typed_ast::Expr {
                    loc,
                    ty: Type::new_unit(self.ctxt()),
                    kind: typed_ast::ExprKind::Assign(Box::new(place), Box::new(value)),
                }
            }
            &ExprKind::NamedRecord(name, ref args, ref fields) => {
                let type_name = self.root().lower_type_name(loc, name, args);
                let (info, args) = match type_name.kind() {
                    &TypeKind::Named(id, name, ref args) => (Ok((id, name)), args.clone()),
                    _ => {
                        self.ctxt()
                            .diag()
                            .add_diagnostic(format!("Cannot construct '{}'", type_name), loc);
                        (Err(type_name), GenericArgs::new())
                    }
                };
                let field_info = if let Ok((ty_id, _)) = info {
                    if !self.ctxt().same_module(ty_id, self.id) && self.ctxt().is_opaque(ty_id) {
                        self.ctxt().diag().add_diagnostic(
                            format!(
                                "Cannot construct '{}' in this scope",
                                self.ctxt().display(ty_id)
                            ),
                            loc,
                        );
                    }
                    let ty_def = self.root().ctxt().type_def(ty_id);
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
                        ty: Type::named(self.ctxt(), id, name, args.iter().copied()),
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
            ExprKind::MethodCall(rcvr, method, args) => {
                let rcvr = self.check_expr(rcvr, None);
                let (sig, generic_args, id) =
                    match self.root().resolve_method(method.loc, rcvr.ty, *method) {
                        Ok((id, mut args)) => {
                            args = args.combine(self.root().lower_generic_args_for(
                                id,
                                method.loc,
                                &resolved_ast::GenericArgs::NONE,
                            ));
                            (
                                self.ctxt().signature_of(id).bind(self.ctxt(), &args),
                                args,
                                Some(id),
                            )
                        }
                        Err(_) => (
                            FunctionSig {
                                params: Vec::new(),
                                return_type: Type::new_unknown(self.ctxt()),
                            },
                            GenericArgs::new(),
                            None,
                        ),
                    };
                let sig_params = if !sig.params.is_empty() {
                    let mut params = sig.params.clone();
                    let ty = params.remove(0);
                    self.root().unify(ty, rcvr.ty, rcvr.loc);
                    params
                } else {
                    self.ctxt().diag().add_diagnostic("Cannot call method", loc);
                    sig.params.clone()
                };
                let (ty, mut args) =
                    self.check_call_sig(loc, args, sig_params, Some(sig.return_type));
                let function = make_expr(
                    Type::function_type(self.ctxt(), sig.params, sig.return_type),
                    if let Some(id) = id {
                        typed_ast::ExprKind::Function(id, generic_args)
                    } else {
                        typed_ast::ExprKind::Err
                    },
                    loc,
                );
                args.insert(0, rcvr);
                make_expr(ty, typed_ast::ExprKind::Call(Box::new(function), args), loc)
            }
            ExprKind::TypeRelativePath(ty_name, method, args) => {
                let ty =
                    self.root()
                        .lower_type_name(loc, *ty_name, &resolved_ast::GenericArgs::NONE);
                let Ok((id, base_args)) = self.root().resolve_method(loc, ty, *method) else {
                    return make_expr(
                        Type::new_unknown(self.ctxt()),
                        typed_ast::ExprKind::Err,
                        loc,
                    );
                };
                let args = base_args.combine(self.root().lower_generic_args_for(id, loc, args));
                let ctxt = self.root().ctxt();
                let sig = self.ctxt().signature_of(id).bind(ctxt, &args);
                make_expr(
                    sig.into_type(self.ctxt()),
                    typed_ast::ExprKind::Function(id, args),
                    loc,
                )
            }
            ExprKind::Array(fields) => {
                let element_ty = expected_ty.and_then(Type::as_array);
                let mut coercion = Coercion::new(element_ty, self);
                for field in fields {
                    coercion.check_expr(field);
                }
                let (combined_element_ty, elements) = coercion.finish();
                let element_ty = combined_element_ty.unwrap_or_else(|| {
                    self.root().type_annotations_needed(loc);
                    Type::new_unknown(self.ctxt())
                });
                let ty = Type::new_array(self.ctxt(), element_ty);
                make_expr(
                    ty,
                    typed_ast::ExprKind::Array(elements.into_boxed_slice()),
                    loc,
                )
            }
        }
    }
    pub(super) fn check_expr(
        &self,
        expr: &Expr,
        expected_ty: Option<Type<'ctxt>>,
    ) -> typed_ast::Expr<'ctxt> {
        let mut expr = self.check_expr_kind(expr, expected_ty);
        if let Some(expected) = expected_ty {
            expr.ty = self.root().unify(expected, expr.ty, expr.loc)
        };
        expr
    }
}
