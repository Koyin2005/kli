use crate::{
    ast::{IsResource, Mutable},
    collect::CtxtRef,
    collect::TypeDefKind,
    diagnostics::emit_fatal_diagnostic,
    mir::{BinaryOp, Body, CastKind, Location, PointerCast, Stmt, StmtKind, visitor::Visit},
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
    fn visit_place(&mut self, loc: Location, place: &super::Place) {
        let mut ty = self.body.type_of_base(&place.base);
        for proj in &place.projections {
            let loc = self.body.src_info(loc);
            match proj {
                super::PlaceProjection::CaseDowncast(index, _) => {
                    ty = if let Type::Named(id, _, ref args) = ty {
                        self.ctxt
                            .type_def(id)
                            .case(*index)
                            .expect_field()
                            .type_of(args, self.ctxt)
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
            super::Rvalue::Discriminant(place) => {
                self.assert(
                    if let Type::Named(id, _, _) = self.body.type_of_place(place, self.ctxt)
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
                    let env_ty = self.body.type_of_operand(env, self.ctxt);
                    self.assert(
                        env_ty.as_pointer().is_some_and(|ty| *ty == Type::Byte),
                        || "env should be byte pointer".to_string(),
                        loc,
                    );
                    let code = self.body.type_of_operand(code, self.ctxt);
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
                super::AggregateKind::ArrayList(ty) => {
                    let [ptr, cap, len] = self.assert_with_some(
                        fields.as_slice(),
                        |fields| {
                            if let [ptr, cap, len] = fields {
                                Some([ptr, cap, len])
                            } else {
                                None
                            }
                        },
                        || "ArrayList must have 3 fields".to_string(),
                        loc,
                    );
                    let ptr_ty = self.body.type_of_operand(ptr, self.ctxt);
                    self.assert(
                        ptr_ty == Type::pointer(ty.clone()),
                        || "ptr should point to same type".to_string(),
                        loc,
                    );
                    let cap_ty = self.body.type_of_operand(cap, self.ctxt);
                    self.assert(cap_ty == Type::Int, || "cap should be int".to_string(), loc);
                    let len_ty = self.body.type_of_operand(len, self.ctxt);
                    self.assert(len_ty == Type::Int, || "len should be int".to_string(), loc);
                }
                super::AggregateKind::Array(ty, count) => {
                    self.assert(
                        fields.len() == (*count).try_into().unwrap(),
                        || format!("array requires '{}' fields", count),
                        loc,
                    );
                    for field in fields {
                        let field_ty = self.body.type_of_operand(field, self.ctxt);
                        self.assert(
                            field_ty == *ty,
                            || "array field must have same type as array".to_string(),
                            loc,
                        );
                    }
                }
                super::AggregateKind::String => {
                    let [ptr, cap, len] = self.assert_with_some(
                        fields.as_slice(),
                        |fields| {
                            if let [ptr, cap, len] = fields {
                                Some([ptr, cap, len])
                            } else {
                                None
                            }
                        },
                        || "String must have 3 fields".to_string(),
                        loc,
                    );
                    let ptr_ty = self.body.type_of_operand(ptr, self.ctxt);
                    self.assert(
                        ptr_ty == Type::pointer(Type::Byte),
                        || "ptr should be a byte pointer".to_string(),
                        loc,
                    );
                    let cap_ty = self.body.type_of_operand(cap, self.ctxt);
                    self.assert(cap_ty == Type::Int, || "cap should be int".to_string(), loc);
                    let len_ty = self.body.type_of_operand(len, self.ctxt);
                    self.assert(len_ty == Type::Int, || "len should be int".to_string(), loc);
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
                    let operand_ty = self.body.type_of_operand(field, self.ctxt);
                    self.assert(
                        field_ty == operand_ty,
                        || format!("{field_ty} and {operand_ty} should be same types"),
                        loc,
                    );
                }
            },
            super::Rvalue::Use(_) => (),
            super::Rvalue::RawPtrTo(_) => {}
            super::Rvalue::Call(operand, operands) => {
                let callee = self.body.type_of_operand(operand, self.ctxt);
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
                    .map(|operand| self.body.type_of_operand(operand, self.ctxt))
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
                    self.body.type_of_operand(left, self.ctxt),
                    self.body.type_of_operand(right, self.ctxt),
                ) {
                    (
                        BinaryOp::BitwiseAnd
                        | BinaryOp::Divide
                        | BinaryOp::Overflow(_)
                        | BinaryOp::Unchecked(_)
                        | BinaryOp::Wrapping(_)
                        | BinaryOp::Lesser,
                        Type::Int,
                        Type::Int,
                    ) => (),
                    (BinaryOp::Offset, Type::RawPointer(_), Type::Int) => (),
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
                    let (pointer_type, _) = self.assert_with_some(
                        self.body.type_of_operand(operand, self.ctxt),
                        |ty| ty.as_pointer_type().ok(),
                        || "Cannot take a non pointer type".to_string(),
                        loc,
                    );
                    match (pointer_cast, pointer_type) {
                        (PointerCast::BoxToRaw, PointerType::Box) => (),
                        (PointerCast::Freeze, PointerType::Reference(_, Mutable::Mutable)) => (),
                        (
                            PointerCast::RawToRaw(_)
                            | PointerCast::RawToBox
                            | PointerCast::RawToRef(..),
                            PointerType::Raw,
                        ) => (),
                        (
                            PointerCast::RefToRaw(Mutable::Immutable),
                            PointerType::Reference(_, Mutable::Immutable),
                        ) => (),
                        (
                            PointerCast::RefToRaw(Mutable::Mutable),
                            PointerType::Reference(_, Mutable::Mutable),
                        ) => (),
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
                    let from = self.body.type_of_operand(operand, self.ctxt);
                    self.assert(
                        unsafety::transmutable(&from, to),
                        || format!("Cannot transmute {} into {}", from, to),
                        loc,
                    );
                }
            },
            super::Rvalue::DecodeUtf8(ptr, index) => {
                let byte_ptr = self.body.type_of_operand(ptr, self.ctxt);
                let index = self.body.type_of_operand(index, self.ctxt);
                self.assert(
                    byte_ptr == Type::pointer(Type::Byte),
                    || "First operand for decode should be a byte pointer".to_string(),
                    loc,
                );
                self.assert(
                    index == Type::Int,
                    || "Second operand should be an index".to_string(),
                    loc,
                );
            }
            super::Rvalue::Len(place) => {
                let ty = self.body.type_of_place(place, self.ctxt);
                self.assert(
                    matches!(ty, Type::Array(..)),
                    || "Expected an array type".to_string(),
                    loc,
                );
            }
        }
    }
    fn visit_stmt(&mut self, loc: Location, stmt: &Stmt) {
        self.super_visit_stmt(loc, stmt);
        match &stmt.kind {
            StmtKind::Assign(lhs, rhs) => {
                let lhs_ty = self.body.type_of_place(lhs, self.ctxt);
                let rhs_ty = self.body.type_of_rvalue(rhs, self.ctxt);
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
            StmtKind::Assert(operand, _) => {
                let condition_ty = self.body.type_of_operand(operand, self.ctxt);
                self.assert(
                    condition_ty == Type::Bool,
                    || format!("Can only assert on bools not {}", condition_ty),
                    stmt.loc,
                );
            }
            StmtKind::Print(operand) => {
                if let Some(operand) = operand {
                    let ty = self.body.type_of_operand(operand, self.ctxt);
                    self.assert(
                        !ty.is_resource(self.ctxt),
                        || format!("Cannot print resource {}", ty),
                        stmt.loc,
                    );
                }
            }
            StmtKind::Deallocate(operand) => {
                let pointer = self.body.type_of_operand(operand, self.ctxt);
                self.assert(
                    pointer.as_pointer().is_some(),
                    || format!("Cannot deallocate {}", pointer),
                    stmt.loc,
                );
            }
        }
    }
}
