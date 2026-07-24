use std::borrow::Cow;

use crate::{
    collect::{CtxtRef, TypeDefKind},
    diagnostics::emit_fatal_diagnostic,
    mir::{
        BinaryOp, Body, CastKind, Location, Stmt,
        StmtKind, TerminatorKind,
        visitor::{PlaceCtxt, Visit},
    },
    src_loc::SrcLoc,
    types::{FunctionType, Type},
    unsafety,
};
pub struct WellFormed<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
    body: &'ctxt Body,
}
impl<'ctxt> WellFormed<'ctxt> {
    pub fn new(body: &'ctxt Body, ctxt: CtxtRef<'ctxt>) -> Self {
        Self { ctxt, body }
    }
    #[track_caller]
    fn assert<S: Into<Cow<'static, str>>>(
        &mut self,
        condition: bool,
        msg: impl FnOnce() -> S,
        loc: SrcLoc,
    ) {
        if !condition {
            emit_fatal_diagnostic(loc, msg());
        }
    }
    #[track_caller]
    fn assert_with_some<T, U, S: Into<Cow<'static, str>>>(
        &mut self,
        value: T,
        f: impl FnOnce(T) -> Option<U>,
        msg: impl FnOnce() -> S,
        loc: SrcLoc,
    ) -> U {
        let Some(value) = f(value) else {
            emit_fatal_diagnostic(loc, msg().into());
        };
        value
    }
}
impl Visit for WellFormed<'_> {
    fn visit_place(&mut self, _: PlaceCtxt, loc: Location, place: &super::Place) {
        let mut ty = place
            .base
            .type_of(&self.body.locals, &self.body.return_type);
        for proj in &place.projections {
            let loc = self.body.src_info(loc);
            match proj {
                super::PlaceProjection::CaseDowncast(index, _) => {
                    ty = if let Type::Named(id, _, ref args) = ty {
                        self.ctxt
                            .type_def(id)
                            .case(*index)
                            .payload_type(args, self.ctxt)
                    } else {
                        emit_fatal_diagnostic(loc, format!("Cannot get inner value of '{}'", ty))
                    };
                }
                super::PlaceProjection::Field(field_id) => {
                    let field_ty = ty.field_info(*field_id, self.ctxt);
                    (ty, _) = self.assert_with_some(
                        &ty,
                        |_| field_ty,
                        || format!("Cannot take a field of '{}'", ty),
                        loc,
                    )
                }
                super::PlaceProjection::ConstantIndex(_) | super::PlaceProjection::Index(_) => {
                    ty = self.assert_with_some(
                        ty,
                        |ty| match ty {
                            Type::Array(ty) => Some(*ty),
                            _ => None,
                        },
                        || "Cannot take an index for non-array",
                        loc,
                    )
                }
            }
        }
    }

    fn visit_rvalue(&mut self, loc: Location, rvalue: &super::Rvalue) {
        self.super_visit_rvalue(loc, rvalue);
        let loc = self.body.src_info(loc);
        match rvalue {
            super::Rvalue::Discriminant(place) => {
                self.assert(
                    if let Type::Named(id, _, _) =
                        place.type_of(self.ctxt, &self.body.locals, &self.body.return_type)
                        && let TypeDefKind::Variant(_) = self.ctxt.type_def(id).kind
                    {
                        true
                    } else {
                        false
                    },
                    || "type does not have a discriminant",
                    loc,
                );
            }
            super::Rvalue::AllocateArray(element, fields) => {
                for field in fields {
                    let field_ty =
                        field.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                    self.assert(
                        *element == field_ty,
                        || format!("array elements should have same type '{}'", element),
                        loc,
                    );
                }
            }
            super::Rvalue::Aggregate(aggregate_kind, fields) => match aggregate_kind {
                super::AggregateKind::Record { field_names } => self.assert(
                    fields.len() == field_names.len(),
                    || "Field names should be same length as fields",
                    loc,
                ),
                super::AggregateKind::NamedRecord(id, args) => {
                    let type_def = self.ctxt.type_def(*id);
                    let field_info = type_def.fields();
                    self.assert(
                        fields.len() == field_info.len(),
                        || "should have fields for each field def",
                        loc,
                    );
                    for (field, operand) in field_info.iter().zip(fields) {
                        let field_ty = field.type_of(args, self.ctxt);
                        self.assert(
                            field_ty
                                == operand.type_of(
                                    self.ctxt,
                                    &self.body.locals,
                                    &self.body.return_type,
                                ),
                            || format!("Field of '{}' should have type '{}'", field.name, field_ty),
                            loc,
                        );
                    }
                }
                super::AggregateKind::Closure(..) => {
                    let (env, code) = self.assert_with_some(
                        fields.as_slice(),
                        |fields| match fields {
                            [env, code] => Some((env, code)),
                            _ => None,
                        },
                        || "closure should have two fields",
                        loc,
                    );
                    let env_ty = env.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                    self.assert(
                        env_ty.as_pointer().is_some_and(|ty| *ty == Type::Byte),
                        || "env should be byte pointer",
                        loc,
                    );
                    let code = code.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                    self.assert(
                        matches!(code, Type::Function(FunctionType { .. })),
                        || "code should be function pointer",
                        loc,
                    );
                }
                super::AggregateKind::Variant(id, index, args) => {
                    let type_def = self.ctxt.type_def(*id);
                    let case_def = type_def.case(*index);
                    let field = case_def.expect_field();
                    let field_ty = field.type_of(args, self.ctxt);

                    let field = self.assert_with_some(
                        fields.as_slice(),
                        |fields| {
                            if let [field] = fields {
                                Some(field)
                            } else {
                                None
                            }
                        },
                        || {
                            format!(
                                "Variants can only have at most 1 inner field not {}",
                                fields.len()
                            )
                        },
                        loc,
                    );
                    let operand_ty =
                        field.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                    self.assert(
                        field_ty == operand_ty,
                        || format!("{field_ty} and {operand_ty} should be same types"),
                        loc,
                    );
                }
                super::AggregateKind::Tuple => (),
            },
            super::Rvalue::Use(_) => (),
            super::Rvalue::AddrOf(place) => {
                self.assert(
                    matches!(
                        place.type_of(self.ctxt, &self.body.locals, &self.body.return_type),
                        Type::Array(_)
                    ),
                    || "Expected an array".to_string(),
                    loc,
                );
            }
            super::Rvalue::Call(operand, operands) => {
                let callee = operand.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                let FunctionType { params, .. } = self.assert_with_some(
                    callee,
                    |ty| match ty {
                        Type::Function(function_type) => Some(function_type),
                        _ => None,
                    },
                    || "Can only call function types",
                    loc,
                );
                let operand_tys = operands
                    .iter()
                    .map(|operand| {
                        operand.type_of(self.ctxt, &self.body.locals, &self.body.return_type)
                    })
                    .collect::<Vec<_>>();
                self.assert(
                    operand_tys == params,
                    || format!("Expected '{:?}' but got '{:?}'", params, operand_tys),
                    loc,
                );
            }
            super::Rvalue::Binary(binary_op, left_and_right) => {
                let (left, right) = left_and_right.as_ref();
                match (
                    binary_op,
                    left.type_of(self.ctxt, &self.body.locals, &self.body.return_type),
                    right.type_of(self.ctxt, &self.body.locals, &self.body.return_type),
                ) {
                    (
                        BinaryOp::BitwiseAnd
                        | BinaryOp::Divide
                        | BinaryOp::Overflow(_)
                        | BinaryOp::Unchecked(_)
                        | BinaryOp::Wrapping(_)
                        | BinaryOp::Lesser
                        | BinaryOp::Greater,
                        left,
                        right,
                    ) if left == right && left.is_integer() && right.is_integer() => (),
                    (BinaryOp::BitwiseAnd, Type::Bool, Type::Bool) => (),
                    (BinaryOp::Equals, left, right) => self.assert(
                        left == right,
                        || format!("Cannot equate '{}' and '{}'", left, right),
                        loc,
                    ),
                    (op, left, right) => self.assert(
                        false,
                        || format!("invalid '{op:?}'  with operands {} and {}", left, right),
                        loc,
                    ),
                }
            }
            super::Rvalue::Cast(cast_kind, operand) => match cast_kind {
                CastKind::Transmute(to) => {
                    let from =
                        operand.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                    self.assert(
                        unsafety::transmutable(self.ctxt, &from, to),
                        || format!("Cannot transmute {} into {}", from, to),
                        loc,
                    );
                }
            },
            super::Rvalue::Len(place) => {
                let ty = place.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                self.assert(
                    matches!(ty, Type::Array(..)),
                    || "Expected an array type",
                    loc,
                );
            }
        }
    }
    fn visit_terminator(&mut self, loc: Location, terminator: &super::Terminator) {
        self.super_visit_terminator(loc, terminator);
        if let TerminatorKind::Assert(operand, ..) = &terminator.kind {
            let condition_ty =
                operand.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
            self.assert(
                condition_ty == Type::Bool,
                || format!("Can only assert on bools not {}", condition_ty),
                terminator.src_info,
            );
        }
    }
    fn visit_stmt(&mut self, loc: Location, stmt: &Stmt) {
        self.super_visit_stmt(loc, stmt);
        match &stmt.kind {
            StmtKind::Assign(lhs, rhs) => {
                let lhs_ty = lhs.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                let rhs_ty = rhs.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                self.assert(
                    lhs_ty == rhs_ty,
                    || {
                        format!(
                            "Cannot assign non equal types {} and {} for {:?} {:?}",
                            lhs_ty, rhs_ty, lhs, rhs
                        )
                    },
                    stmt.loc,
                );
            }
            StmtKind::Noop => (),
            StmtKind::Print(_) => {}
        }
    }
}
