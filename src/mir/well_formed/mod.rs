use crate::{
    ast::IsResource,
    collect::{CtxtRef, TypeDefKind},
    diagnostics::emit_fatal_diagnostic,
    mir::{
        BinaryOp, Body, CastKind, CopyNonOverlapping, DropInPlace, Location, PointerCast, Stmt,
        StmtKind, TerminatorKind,
        visitor::{PlaceCtxt, Visit},
    },
    src_loc::SrcLoc,
    types::{FunctionType, PointerType, Type},
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
    fn assert(&mut self, condition: bool, msg: impl FnOnce() -> String, loc: SrcLoc) {
        if !condition {
            emit_fatal_diagnostic(loc, msg());
        }
    }
    #[track_caller]
    fn assert_with_some<T, U>(
        &mut self,
        value: T,
        f: impl FnOnce(T) -> Option<U>,
        msg: impl FnOnce() -> String,
        loc: SrcLoc,
    ) -> U {
        let Some(value) = f(value) else {
            emit_fatal_diagnostic(loc, msg());
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
                            Type::Array(ty, _) => Some(*ty),
                            _ => None,
                        },
                        || "Cannot take an index for non-array".to_string(),
                        loc,
                    )
                }
                super::PlaceProjection::Deref => {
                    ty = self.assert_with_some(
                        ty,
                        |ty| match ty {
                            Type::RawPointer(ty) => Some(*ty),
                            Type::Imm(_, ty) | Type::Mut(_, ty) => Some(*ty),
                            _ => None,
                        },
                        || "Cannot deref non pointer or non ref".to_string(),
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
            super::Rvalue::DanglingPtr(_) => {}
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
                    || "type does not have a discriminant".to_string(),
                    loc,
                );
            }
            super::Rvalue::Aggregate(aggregate_kind, fields) => match aggregate_kind {
                super::AggregateKind::Record { field_names } => self.assert(
                    fields.len() == field_names.len(),
                    || "Field names should be same length as fields".to_string(),
                    loc,
                ),
                super::AggregateKind::NamedRecord(id, args) => {
                    let type_def = self.ctxt.type_def(*id);
                    let field_info = type_def.fields();
                    self.assert(
                        fields.len() == field_info.len(),
                        || "should have fields for each field def".to_string(),
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
                        || "closure should have two fields".to_string(),
                        loc,
                    );
                    let env_ty = env.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                    self.assert(
                        env_ty.as_pointer().is_some_and(|ty| *ty == Type::Byte),
                        || "env should be byte pointer".to_string(),
                        loc,
                    );
                    let code = code.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                    self.assert(
                        matches!(
                            code,
                            Type::Function(FunctionType {
                                resource: IsResource::Data,
                                ..
                            })
                        ),
                        || "code should be function pointer".to_string(),
                        loc,
                    );
                }
                super::AggregateKind::Array(ty, count) => {
                    self.assert(
                        fields.len() == (*count).try_into().unwrap(),
                        || format!("array requires '{}' fields", count),
                        loc,
                    );
                    for field in fields {
                        let field_ty =
                            field.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                        self.assert(
                            field_ty == *ty,
                            || "array field must have same type as array".to_string(),
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
            super::Rvalue::RawPtrTo(_) => {}
            super::Rvalue::Call(operand, operands) => {
                let callee = operand.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                let FunctionType {
                    resource, params, ..
                } = self.assert_with_some(
                    callee,
                    |ty| match ty {
                        Type::Function(function_type) => Some(function_type),
                        _ => None,
                    },
                    || "Can only call function types".to_string(),
                    loc,
                );
                self.assert(
                    resource == IsResource::Data,
                    || "Can only call data functions".to_string(),
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
                    (BinaryOp::BitwiseAnd, Type::Bool, Type::Bool)
                    | (BinaryOp::Offset, Type::RawPointer(_), Type::INT) => (),
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
            super::Rvalue::Ref(..) => (),
            super::Rvalue::Allocate { .. } => (),
            super::Rvalue::Cast(cast_kind, operand) => match cast_kind {
                CastKind::PointerCast(pointer_cast) => {
                    let ctxt = self.ctxt;
                    let (pointer_type, _) = self.assert_with_some(
                        operand.type_of(self.ctxt, &self.body.locals, &self.body.return_type),
                        |ty| ty.into_pointer_type(ctxt).ok(),
                        || "Cannot take a non pointer type".to_string(),
                        loc,
                    );
                    match (pointer_cast, pointer_type) {
                        (PointerCast::RawToRaw(_), PointerType::Raw) => (),
                        (cast, pointer_type) => {
                            self.assert(
                                false,
                                || format!("Invalid pointer cast {cast:?} for {pointer_type:?}"),
                                loc,
                            );
                        }
                    }
                }
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
                    || "Expected an array type".to_string(),
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
            StmtKind::DropInPlace(drop_in_place) => {
                let DropInPlace { pointer_to_place } = drop_in_place.as_ref();
                let pointer_ty =
                    pointer_to_place.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                self.assert(
                    pointer_ty.as_pointer().is_some(),
                    || format!("pointer to place should be pointer not {}", pointer_ty),
                    stmt.loc,
                );
            }
            StmtKind::CopyNonOverlapping(copy) => {
                let CopyNonOverlapping { dst, src, count } = copy.as_ref();
                let dst_ty = dst.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                let src_ty = src.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                let count_ty = count.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                self.assert(
                    dst_ty == src_ty,
                    || format!("src and dst have types {} and {}", dst_ty, src_ty),
                    stmt.loc,
                );
                self.assert(
                    count_ty == Type::UINT,
                    || format!("count should be int not '{}'", count_ty),
                    stmt.loc,
                );
                self.assert(
                    dst_ty.as_pointer() == src_ty.as_pointer() && dst_ty.as_pointer().is_some(),
                    || {
                        format!(
                            "dst and src should be pointers, not {} and {}",
                            dst_ty, src_ty
                        )
                    },
                    stmt.loc,
                );
            }
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
            StmtKind::Print(operand) => {
                if let Some(operand) = operand {
                    let ty = operand.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                    self.assert(
                        !ty.is_resource(self.ctxt),
                        || format!("Cannot print resource {}", ty),
                        stmt.loc,
                    );
                }
            }
            StmtKind::Deallocate(operand) => {
                let pointer = operand.type_of(self.ctxt, &self.body.locals, &self.body.return_type);
                self.assert(
                    pointer.as_pointer().is_some(),
                    || format!("Cannot deallocate {}", pointer),
                    stmt.loc,
                );
            }
        }
    }
}
