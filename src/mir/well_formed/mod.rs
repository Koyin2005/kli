use std::borrow::Cow;

use crate::{
    collect::{CtxtRef, TypeDefKind},
    diagnostics::emit_fatal_diagnostic,
    mir::{
        BinaryOp, BitwiseOp, Body, Location, Operation, Stmt, StmtKind, TerminatorKind,
        visitor::{PlaceCtxt, Visit},
    },
    src_loc::SrcLoc,
    types::{FunctionSig, Type, TypeKind},
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
    fn visit_place(&mut self, _: PlaceCtxt, loc: Location, place: &super::Place<'ctxt>) {
        let mut ty = place
            .base
            .type_of(self.ctxt, &self.body.locals, &self.body.registers);
        for proj in &place.projections {
            let loc = self.body.src_info(loc);
            match proj {
                super::PlaceProjection::CaseDowncast(index, _) => {
                    ty = if let Some((id, _, args)) = ty.as_named() {
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
                        |ty| ty.as_array(),
                        || "Cannot take an index for non-array",
                        loc,
                    )
                }
                super::PlaceProjection::Deref => {
                    ty = self.assert_with_some(
                        ty,
                        |ty| ty.as_box().or(ty.as_raw_ptr()),
                        || format!("Cannot deref non box or ptr type {}", ty),
                        loc,
                    )
                }
            }
        }
    }

    fn visit_rvalue(&mut self, loc: Location, rvalue: &super::Rvalue<'ctxt>) {
        self.super_visit_rvalue(loc, rvalue);
        let loc = self.body.src_info(loc);
        match rvalue {
            super::Rvalue::AllocArray(ty, elements) => {
                for element in elements {
                    let element =
                        element.type_of(self.ctxt(), &self.body.locals, &self.body.registers);
                    self.assert(
                        element == *ty,
                        || format!("Array elements should have type '{}'", ty),
                        loc,
                    );
                }
            }
            super::Rvalue::ReadLine => (),
            super::Rvalue::Discriminant(place) => {
                self.assert(
                    if let Some((id, _, _)) = place
                        .type_of(self.ctxt, &self.body.locals, &self.body.registers)
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
            super::Rvalue::Aggregate(aggregate_kind, fields) => match aggregate_kind {
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
                                    &self.body.registers,
                                ),
                            || format!("Field of '{}' should have type '{}'", field.name, field_ty),
                            loc,
                        );
                    }
                }
                super::AggregateKind::Variant(id, index, args) => {
                    let type_def = self.ctxt.type_def(*id);
                    let case_def = type_def.case(*index);

                    let field = case_def.field;
                    let field_ty = field.map(|field| field.type_of(args, self.ctxt));
                    if let Some(field_ty) = field_ty {
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
                            field.type_of(self.ctxt, &self.body.locals, &self.body.registers);
                        self.assert(
                            field_ty == operand_ty,
                            || format!("{field_ty} and {operand_ty} should be same types"),
                            loc,
                        );
                    } else {
                        self.assert(
                            fields.is_empty(),
                            || format!("{} should have no fields", case_def.name),
                            loc,
                        );
                    }
                }
                super::AggregateKind::Tuple => (),
            },
            super::Rvalue::Use(_) => (),
            super::Rvalue::Call(operand, operands) => {
                let callee = operand.type_of(self.ctxt, &self.body.locals, &self.body.registers);
                let FunctionSig { params, .. } = self.assert_with_some(
                    &callee,
                    |ty| ty.as_function(),
                    || "Can only call function types",
                    loc,
                );
                let operand_tys = operands
                    .iter()
                    .map(|operand| {
                        operand.type_of(self.ctxt, &self.body.locals, &self.body.registers)
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
                    left.type_of(self.ctxt, &self.body.locals, &self.body.registers),
                    right.type_of(self.ctxt, &self.body.locals, &self.body.registers),
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
                        if left.as_raw_ptr().is_some_and(|_| right.is_integer()) => {}
                    (op, left, right) => self.assert(
                        false,
                        || format!("invalid '{op:?}' with operands {} and {}", left, right),
                        loc,
                    ),
                }
            }
            super::Rvalue::Len(place) => {
                let ty = place.type_of(self.ctxt, &self.body.locals, &self.body.registers);
                self.assert(
                    ty.as_array().is_some() || matches!(ty.kind(), TypeKind::String),
                    || "Expected an array or string type",
                    loc,
                );
            }
        }
    }
    fn visit_terminator(&mut self, loc: Location, terminator: &super::Terminator<'ctxt>) {
        self.super_visit_terminator(loc, terminator);
        match &terminator.kind {
            TerminatorKind::Goto(block, args) => {
                let block_regs = &self.body.block_info.blocks()[*block].args;
                self.assert(
                    args.len() == block_regs.len(),
                    || {
                        format!(
                            "expected {} args for bb{} but got {}",
                            block_regs.len(),
                            block.into_usize(),
                            args.len()
                        )
                    },
                    terminator.src_info,
                );
                for (arg, block_reg) in args.iter().zip(block_regs) {
                    let arg_ty = arg.type_of(self.ctxt, &self.body.registers);
                    let block_reg_ty = self.body.registers[*block_reg].ty;
                    self.assert(
                        arg_ty == block_reg_ty,
                        || format!("Expected {} but got {}", block_reg_ty, arg_ty),
                        terminator.src_info,
                    );
                }
            }
            TerminatorKind::Switch(value, targets) => {
                let value_ty = value.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    value_ty.is_builtin_scalar(),
                    || format!("should be a scalar ty but got '{}'", value_ty),
                    terminator.src_info,
                );
                for target in targets.succesors_iter() {
                    self.assert(
                        self.body.block_info.blocks()[target].args.is_empty(),
                        || format!("bb{} should have no args", target.into_usize()),
                        terminator.src_info,
                    );
                }
            }
            TerminatorKind::Return(value) => {
                let value_ty = value.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    value_ty == self.body.return_type,
                    || {
                        format!(
                            "return value should have type '{}' not '{}'",
                            self.body.return_type, value_ty
                        )
                    },
                    terminator.src_info,
                );
            }
            _ => (),
        }
    }
    fn visit_operation(&mut self, loc: Location, operation: &super::Operation<'ctxt>) {
        self.super_visit_operation(loc, operation);
        match operation {
            Operation::ReadLine => (),
            Operation::Bitwise(op, left, right) => {
                let left_ty = left.type_of(self.ctxt, &self.body.registers);
                let right_ty = right.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    left_ty == right_ty,
                    || format!("{op:?} should have operands {left_ty} and {right_ty}"),
                    self.body.src_info(loc),
                );
                match op {
                    BitwiseOp::And | BitwiseOp::Or => {
                        self.assert(
                            left_ty.is_bool() || left_ty.is_integer(),
                            || format!("expected int"),
                            self.body.src_info(loc),
                        );
                    }
                    BitwiseOp::ShiftLeft | BitwiseOp::ShiftRight => {
                        self.assert(
                            left_ty.is_integer(),
                            || format!("expected int"),
                            self.body.src_info(loc),
                        );
                    }
                }
            }
            Operation::Not(value) => {
                let ty = value.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    ty.is_bool(),
                    || format!("Should be a bool '{}'", ty),
                    self.body.src_info(loc),
                );
            }
            Operation::Discriminant(value) => {
                let loc = self.body.src_info(loc);
                self.assert(
                    if let Some((id, _, _)) =
                        value.type_of(self.ctxt, &self.body.registers).as_named()
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
            Operation::InBounds(base, index) => {
                let ty = base.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    ty.as_array().is_some(),
                    || format!("Should be an array '{}'", ty),
                    self.body.src_info(loc),
                );

                let ty = index.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    ty.is_integer(),
                    || format!("Should be an integer '{ty}'"),
                    self.body.src_info(loc),
                );
            }
            Operation::Len(value) => {
                let ty = value.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    ty.as_array().is_some(),
                    || format!("Should be an array '{}'", ty),
                    self.body.src_info(loc),
                );
            }
            Operation::Load(_) => (),
            Operation::AllocArray(ty, elements) => {
                let loc = self.body.src_info(loc);
                for element in elements {
                    let element = element.type_of(self.ctxt(), &self.body.registers);
                    self.assert(
                        element == *ty,
                        || format!("Array elements should have type '{}'", ty),
                        loc,
                    );
                }
            }
            Operation::Cmp(_, left, right) => {
                let lhs_ty = left.type_of(self.ctxt, &self.body.registers);
                let rhs_ty = right.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    lhs_ty == rhs_ty && lhs_ty.is_builtin_scalar(),
                    || format!("{} and {} should be scalars", lhs_ty, rhs_ty),
                    self.body.src_info(loc),
                );
            }
            Operation::Arith(_, left, right) => {
                let lhs_ty = left.type_of(self.ctxt, &self.body.registers);
                let rhs_ty = right.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    lhs_ty == rhs_ty && lhs_ty.is_builtin_scalar(),
                    || format!("{} and {} should be scalars", lhs_ty, rhs_ty),
                    self.body.src_info(loc),
                );
            }
            Operation::ExtractPayload(value, case_id) => {
                let loc = self.body.src_info(loc);
                let ty = value.type_of(self.ctxt, &self.body.registers);
                let (id, ..) = self.assert_with_some(
                    &ty,
                    |ty| ty.as_named(),
                    || format!("Cannot get inner value of '{}'", ty),
                    loc,
                );

                let type_def = self.ctxt.type_def(id);
                let cases = self.assert_with_some(
                    &type_def,
                    |type_def| type_def.cases(),
                    || format!("'{ty}' should have cases"),
                    loc,
                );
                let _ = self.assert_with_some(
                    cases,
                    |cases| cases.get(*case_id),
                    || format!("not enough cases"),
                    loc,
                );
            }
            Operation::ExtractField(value, field) => {
                let ty = value.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    ty.field_info(*field, self.ctxt()).is_some(),
                    || format!("{ty} does not have a field {field:?}"),
                    self.body.src_info(loc),
                );
            }
            Operation::ExtractElement(array, index) => {
                let loc = self.body.src_info(loc);
                let ty = array.type_of(self.ctxt, &self.body.registers);
                let _ = self.assert_with_some(
                    ty,
                    |ty| ty.as_array(),
                    || "Cannot take an index for non-array",
                    loc,
                );
                let ty = index.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    ty.is_integer(),
                    || format!("Index should be an int not '{ty}'"),
                    loc,
                );
            }
            Operation::Call(callee, args) => {
                let loc = self.body.src_info(loc);
                let callee = callee.type_of(self.ctxt, &self.body.registers);
                let FunctionSig { params, .. } = self.assert_with_some(
                    &callee,
                    |ty| ty.as_function(),
                    || "Can only call function types",
                    loc,
                );
                let operand_tys = args
                    .iter()
                    .map(|operand| operand.type_of(self.ctxt, &self.body.registers))
                    .collect::<Vec<_>>();
                self.assert(
                    operand_tys == *params,
                    || format!("Expected '{:?}' but got '{:?}'", params, operand_tys),
                    loc,
                );
            }
            Operation::Aggregate(kind, fields) => {
                let loc = self.body.src_info(loc);
                match kind {
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
                                field_ty == operand.type_of(self.ctxt, &self.body.registers),
                                || {
                                    format!(
                                        "Field of '{}' should have type '{}'",
                                        field.name, field_ty
                                    )
                                },
                                loc,
                            );
                        }
                    }
                    super::AggregateKind::Variant(id, index, args) => {
                        let type_def = self.ctxt.type_def(*id);
                        let case_def = type_def.case(*index);

                        let field = case_def.field;
                        let field_ty = field.map(|field| field.type_of(args, self.ctxt));
                        if let Some(field_ty) = field_ty {
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
                            let operand_ty = field.type_of(self.ctxt, &self.body.registers);
                            self.assert(
                                field_ty == operand_ty,
                                || format!("{field_ty} and {operand_ty} should be same types"),
                                loc,
                            );
                        } else {
                            self.assert(
                                fields.is_empty(),
                                || format!("{} should have no fields", case_def.name),
                                loc,
                            );
                        }
                    }
                    super::AggregateKind::Tuple => (),
                }
            }
        }
    }
    fn visit_stmt(&mut self, loc: Location, stmt: &Stmt<'ctxt>) {
        self.super_visit_stmt(loc, stmt);
        match &stmt.kind {
            StmtKind::Store(place, value) => {
                let lhs_ty = place.type_of(self.ctxt, &self.body.locals, &self.body.registers);
                let rhs_ty = value.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    lhs_ty == rhs_ty,
                    || {
                        format!(
                            "Cannot assign non equal types {} and {} for {:?} {:?}",
                            lhs_ty, rhs_ty, place, value
                        )
                    },
                    stmt.loc,
                );
            }
            StmtKind::PanicIf(value) => {
                let ty = value.type_of(self.ctxt, &self.body.registers);
                self.assert(
                    ty.is_bool(),
                    || format!("PanicIf requires a bool '{}'", ty),
                    stmt.loc,
                );
            }
            StmtKind::Assign(dst, operation) => {
                let lhs_ty = self.body.registers[*dst].ty;
                let rhs_ty =
                    operation.result_type(self.ctxt, &self.body.registers, &self.body.locals);
                self.assert(
                    lhs_ty == rhs_ty,
                    || format!("Cannot assign non equal types {} and {}", lhs_ty, rhs_ty),
                    stmt.loc,
                );
            }
            StmtKind::OldStore(lhs, rhs) => {
                let lhs_ty = lhs.type_of(self.ctxt, &self.body.locals, &self.body.registers);
                let rhs_ty = rhs.type_of(self.ctxt, &self.body.locals, &self.body.registers);
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
            StmtKind::Print { value, err: _ } => {
                self.assert(
                    value.type_of(self.ctxt, &self.body.registers) == Type::new_string(self.ctxt),
                    || "cannot print non string",
                    stmt.loc,
                );
            }
        }
    }
}
