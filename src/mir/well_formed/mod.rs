use std::borrow::Cow;

use crate::{
    collect::{CtxtRef, TypeDefKind},
    diagnostics::emit_fatal_diagnostic,
    mir::{
        BinaryOp, Body, CastKind, IntegerCast, Location, Stmt, StmtKind, TerminatorKind,
        visitor::{PlaceCtxt, Visit},
    },
    src_loc::SrcLoc,
    types::{FunctionSig, IntegerKind, IntegerSize, SimpleScalar, Type},
    unsafety,
};
pub struct WellFormed<'ctxt, 'body> {
    ctxt: CtxtRef<'ctxt>,
    body: &'body Body<'ctxt>,
}
impl<'ctxt, 'body> WellFormed<'ctxt, 'body> {
    pub fn new(body: &'body Body<'ctxt>, ctxt: CtxtRef<'ctxt>) -> Self {
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
impl<'ctxt> Visit<'ctxt> for WellFormed<'ctxt, '_> {
    fn ctxt(&self) -> CtxtRef<'ctxt> {
        self.ctxt
    }
    fn visit_place(&mut self, _: PlaceCtxt, loc: Location, place: &super::Place) {
        let mut ty = place.base.type_of(&self.body.locals, self.body.return_type);
        for proj in &place.projections {
            let loc = self.body.src_info(loc);
            match *proj {
            }
        }
    }

    fn visit_rvalue(&mut self, loc: Location, rvalue: &super::Rvalue<'ctxt>) {
        self.super_visit_rvalue(loc, rvalue);
        let loc = self.body.src_info(loc);
        match rvalue {
            super::Rvalue::Unbox(place) => {
                let ty = place.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                self.assert(
                        ty.as_box().is_some(),
                        || "Cannot deref non box or ptr",
                        loc,
                    );
            }
            super::Rvalue::LoadPayload(place, case) => {
                let ty = place.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let (id, name, _) = self.assert_with_some(
                    &ty,
                    |ty| ty.as_named(),
                    || format!("Should be a named type but got '{}'", ty),
                    loc,
                );
                let type_def = self.ctxt.type_def(id);
                let cases = self.assert_with_some(
                    type_def.cases(),
                    std::convert::identity,
                    || format!("should be a type with cases for '{name}'"),
                    loc,
                );
                self.assert(
                    cases.len() >= case.into_usize(),
                    || format!("Case id {} too high", case.into_usize()),
                    loc,
                );
            }
            super::Rvalue::LoadField(place, field) => {
                let ty = place.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let field_ty = ty.field_info(*field, self.ctxt);
                self.assert_with_some(
                    field_ty,
                    |ty| ty,
                    || format!("Cannot take a field of '{}'", ty),
                    loc,
                );
            }
            super::Rvalue::LoadIndex(place, index) => {
                let array_ty = place.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let index_ty = index.type_of(self.ctxt, &self.body.locals, self.body.return_type);

                self.assert(
                    array_ty.as_array().is_some(),
                    || "array type should be an array",
                    loc,
                );
                self.assert(
                    index_ty.is_integer_kind(IntegerKind::Unsigned(IntegerSize::Int64)),
                    || format!("index should be a uint not '{}'", index_ty),
                    loc,
                );
            }
            super::Rvalue::GcAlloc(_, count) => {
                let count_ty = count.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                self.assert(
                    count_ty.is_integer_kind(IntegerKind::Unsigned(IntegerSize::Int64)),
                    || format!("count should be a uint not '{}'", count_ty),
                    loc,
                );
            }
            super::Rvalue::UninitZeroed(_) | super::Rvalue::ReadLine => (),
            super::Rvalue::AllocateBox(ty, operand) => {
                self.assert(
                    *ty == operand.type_of(self.ctxt, &self.body.locals, self.body.return_type),
                    || "Same type",
                    loc,
                );
            }
            super::Rvalue::Discriminant(place) => {
                self.assert(
                    if let Some((id, _, _)) = place
                        .type_of(self.ctxt, &self.body.locals, self.body.return_type)
                        .as_named()
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
            super::Rvalue::AllocateRawArray { count, .. } => {
                let count_ty = count.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                self.assert(
                    count_ty.is_integer_kind(IntegerKind::Unsigned(IntegerSize::Int64)),
                    || format!("count should be a uint not '{}'", count_ty),
                    loc,
                );
            }
            super::Rvalue::AllocateArray(element, fields) => {
                for field in fields {
                    let field_ty =
                        field.type_of(self.ctxt, &self.body.locals, self.body.return_type);
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
                                    self.body.return_type,
                                ),
                            || format!("Field of '{}' should have type '{}'", field.name, field_ty),
                            loc,
                        );
                    }
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
                        field.type_of(self.ctxt, &self.body.locals, self.body.return_type);
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
                    place
                        .type_of(self.ctxt, &self.body.locals, self.body.return_type)
                        .as_array()
                        .is_some(),
                    || "Expected an array".to_string(),
                    loc,
                );
            }
            super::Rvalue::Call(operand, operands) => {
                let callee = operand.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let FunctionSig { params, .. } = self.assert_with_some(
                    &callee,
                    |ty| ty.as_function(),
                    || "Can only call function types",
                    loc,
                );
                let operand_tys = operands
                    .iter()
                    .map(|operand| {
                        operand.type_of(self.ctxt, &self.body.locals, self.body.return_type)
                    })
                    .collect::<Vec<_>>();
                self.assert(
                    operand_tys == *params,
                    || format!("Expected '{:?}' but got '{:?}'", params, operand_tys),
                    loc,
                );
            }
            super::Rvalue::Binary(binary_op, left_and_right) => {
                let (left, right) = left_and_right.as_ref();
                match (
                    binary_op,
                    left.type_of(self.ctxt, &self.body.locals, self.body.return_type),
                    right.type_of(self.ctxt, &self.body.locals, self.body.return_type),
                ) {
                    (
                        BinaryOp::Divide | BinaryOp::Overflow(_) | BinaryOp::Wrapping(_),
                        left,
                        right,
                    ) if left == right && left.is_integer() => (),
                    (BinaryOp::Lesser | BinaryOp::Greater, left, right)
                        if left == right && left.is_builtin_scalar() => {}
                    (BinaryOp::BitwiseAnd | BinaryOp::BitwiseOr, left, right)
                        if left == right && (left.is_integer() || left.is_bool()) => {}
                    (BinaryOp::ShiftLeft | BinaryOp::ShiftRight, left, right)
                        if left == right && left.is_integer() => {}
                    (BinaryOp::Equals, left, right) => self.assert(
                        left == right,
                        || format!("Cannot equate '{}' and '{}'", left, right),
                        loc,
                    ),
                    (BinaryOp::Offset, left, right)
                        if left
                            .as_raw_ptr()
                            .is_some_and(|_| right.is_uint(IntegerSize::Int64)) => {}
                    (op, left, right) => self.assert(
                        false,
                        || format!("invalid '{op:?}' with operands {} and {}", left, right),
                        loc,
                    ),
                }
            }
            &super::Rvalue::Cast(cast_kind, ref operand, to_ty) => match cast_kind {
                CastKind::Transmute => {
                    let from = operand.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                    self.assert(
                        unsafety::transmutable(self.ctxt, from, to_ty),
                        || format!("Cannot transmute {} into {}", from, to_ty),
                        loc,
                    );
                }
                CastKind::IntegerCast(kind) => match kind {
                    IntegerCast::ZeroExtend(to) => {
                        let from_ty =
                            operand.type_of(self.ctxt, &self.body.locals, self.body.return_type);

                        let from = self.assert_with_some(
                            from_ty,
                            |from| from.as_integer().map(IntegerKind::size),
                            || "Should be an integer",
                            loc,
                        );

                        self.assert(
                            from.bit_width() < to.bit_width(),
                            || {
                                format!(
                                    "Cannot extend {} into {}",
                                    from_ty,
                                    IntegerKind::Unsigned(to)
                                )
                            },
                            loc,
                        );
                    }
                    IntegerCast::SignExtend(to) => {
                        let from_ty =
                            operand.type_of(self.ctxt, &self.body.locals, self.body.return_type);

                        let from = self
                            .assert_with_some(
                                from_ty,
                                |from| from.as_simple_scalar().map(SimpleScalar::as_integer),
                                || "Should be an integer",
                                loc,
                            )
                            .size();

                        self.assert(
                            from.bit_width() <= to.bit_width(),
                            || {
                                format!(
                                    "Cannot extend {} into {}",
                                    from_ty,
                                    IntegerKind::Signed(to)
                                )
                            },
                            loc,
                        );
                    }
                    IntegerCast::Truncate(to) => {
                        let from_ty =
                            operand.type_of(self.ctxt, &self.body.locals, self.body.return_type);

                        let from = self
                            .assert_with_some(
                                from_ty,
                                |from| from.as_simple_scalar().map(SimpleScalar::as_integer),
                                || "Should be an integer",
                                loc,
                            )
                            .size();

                        let to = to.size();
                        self.assert(
                            from.bit_width() >= to.bit_width(),
                            || {
                                format!(
                                    "Cannot truncate {} into {}",
                                    from_ty,
                                    IntegerKind::Signed(to)
                                )
                            },
                            loc,
                        );
                    }
                },
            },
            super::Rvalue::Len(place) => {
                let ty = place.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                self.assert(ty.as_array().is_some(), || "Expected an array type", loc);
            }
        }
    }
    fn visit_terminator(&mut self, loc: Location, terminator: &super::Terminator<'ctxt>) {
        self.super_visit_terminator(loc, terminator);
        if let TerminatorKind::Assert(operand, ..) = &terminator.kind {
            let condition_ty = operand.type_of(self.ctxt, &self.body.locals, self.body.return_type);
            self.assert(
                condition_ty.is_bool(),
                || format!("Can only assert on bools not {}", condition_ty),
                terminator.src_info,
            );
        }
    }
    fn visit_stmt(&mut self, loc: Location, stmt: &Stmt<'ctxt>) {
        self.super_visit_stmt(loc, stmt);
        match &stmt.kind {
            StmtKind::AssignField(place, field, rhs) => {
                let receiver = place.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let rhs_ty = rhs.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let (lhs_ty, _) = self.assert_with_some(
                    receiver.field_info(*field, self.ctxt()),
                    |ty| ty,
                    || {
                        format!(
                            "'{}' does not have a field '{}'",
                            receiver,
                            field.into_usize()
                        )
                    },
                    stmt.loc,
                );
                self.assert(
                    lhs_ty == rhs_ty,
                    || format!("Cannot assign non equal types {} and {}", lhs_ty, rhs_ty),
                    stmt.loc,
                );
            }
            StmtKind::AssignIndex(array, index, rhs) => {
                let array = array.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let index = index.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let rhs_ty = rhs.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let element_ty = self.assert_with_some(
                    array,
                    |ty| ty.as_array(),
                    || format!("should be an array but got '{}'", array),
                    stmt.loc,
                );
                self.assert(
                    index.is_uint(IntegerSize::Int64),
                    || format!("should be index type but got '{}'", index),
                    stmt.loc,
                );
                self.assert(
                    element_ty == rhs_ty,
                    || {
                        format!(
                            "Cannot assign non equal types {} and {}",
                            element_ty, rhs_ty
                        )
                    },
                    stmt.loc,
                );
            }
            StmtKind::Assign(lhs, rhs) => {
                let lhs_ty = lhs.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let rhs_ty = rhs.type_of(self.ctxt, &self.body.locals, self.body.return_type);
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
            StmtKind::Copy { dst, src, count } => {
                let lhs_ty = dst.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let rhs_ty = src.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                let (lhs_ty, rhs_ty) = self.assert_with_some(
                    lhs_ty.as_raw_ptr().zip(rhs_ty.as_raw_ptr()),
                    |tys| tys,
                    || "Expected pointer types",
                    stmt.loc,
                );
                self.assert(
                    lhs_ty == rhs_ty,
                    || {
                        format!(
                            "Cannot copy non equal pointee types {} and {} for {:?} {:?}",
                            lhs_ty, rhs_ty, dst, src
                        )
                    },
                    stmt.loc,
                );
                let count_ty = count.type_of(self.ctxt, &self.body.locals, self.body.return_type);
                self.assert(
                    count_ty.is_integer_kind(IntegerKind::Unsigned(IntegerSize::Int64)),
                    || format!("count should be a uint not '{}'", count_ty),
                    stmt.loc,
                );
            }
            StmtKind::Noop => (),
            StmtKind::Print { value, err: _ } => {
                self.assert(
                    value.type_of(self.ctxt, &self.body.locals, self.body.return_type)
                        == Type::new_string(self.ctxt),
                    || "cannot print non string",
                    stmt.loc,
                );
            }
        }
    }
}
