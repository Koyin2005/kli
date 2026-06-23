use crate::{
    ast::{IsResource, Mutable},
    diagnostics::emit_fatal_diagnostic,
    mir::{BinaryOp, Body, Context, Location, PointerCast, Stmt, StmtKind, visitor::Visit},
    src_loc::SrcLoc,
    types::{FunctionType, PointerType, Type},
};
pub const CHECK_WELL_FORMED: bool = false;
pub struct WellFormed<'ctxt> {
    _ctxt: &'ctxt Context,
    body: &'ctxt Body,
}
impl<'ctxt> WellFormed<'ctxt> {
    pub fn new(body: &'ctxt Body, _ctxt: &'ctxt Context) -> Self {
        Self { _ctxt, body }
    }
    fn assert(&mut self, condition: bool, msg: impl FnOnce() -> String, loc: SrcLoc) {
        if !condition {
            emit_fatal_diagnostic(loc, msg());
        }
    }
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
    fn visit_rvalue(&mut self, loc: Location, rvalue: &super::Rvalue) {
        let loc = self.body.src_info(loc);
        match rvalue {
            super::Rvalue::Aggregate(aggregate_kind, fields) => match aggregate_kind {
                super::AggregateKind::Record { field_names } => self.assert(
                    fields.len() == field_names.len(),
                    || "Field names should be same length as fields".to_string(),
                    loc,
                ),
                super::AggregateKind::Closure => {
                    let (env, code) = self.assert_with_some(
                        fields.as_slice(),
                        |fields| match fields {
                            [env, code] => Some((env, code)),
                            _ => None,
                        },
                        || "closure should have two fields".to_string(),
                        loc.clone(),
                    );
                    let env_ty = self.body.type_of_operand(env);
                    self.assert(
                        env_ty.as_pointer().is_some_and(|ty| *ty == Type::Byte),
                        || "env should be byte pointer".to_string(),
                        loc.clone(),
                    );
                    let code = self.body.type_of_operand(code);
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
                super::AggregateKind::Option { inner, is_some } => {
                    if *is_some {
                        let field = self.assert_with_some(
                            fields.as_slice(),
                            |fields| {
                                if let [field] = fields {
                                    Some(field)
                                } else {
                                    None
                                }
                            },
                            || "Some value can only have one field".to_string(),
                            loc.clone(),
                        );
                        let field_ty = self.body.type_of_operand(field);
                        self.assert(
                            field_ty == *inner,
                            || "Some value should have same type as inner".to_string(),
                            loc,
                        );
                    } else {
                        self.assert(
                            fields.as_slice().is_empty(),
                            || "None value can not have fields".to_string(),
                            loc,
                        );
                    }
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
                        loc.clone(),
                    );
                    let ptr_ty = self.body.type_of_operand(ptr);
                    self.assert(
                        ptr_ty == Type::pointer(ty.clone()),
                        || "ptr should point to same type".to_string(),
                        loc.clone(),
                    );
                    let cap_ty = self.body.type_of_operand(cap);
                    self.assert(
                        cap_ty == Type::Int,
                        || "cap should be int".to_string(),
                        loc.clone(),
                    );
                    let len_ty = self.body.type_of_operand(len);
                    self.assert(
                        len_ty == Type::Int,
                        || "len should be int".to_string(),
                        loc.clone(),
                    );
                }
                super::AggregateKind::Array(ty, count) => {
                    self.assert(
                        fields.len() == (*count).try_into().unwrap(),
                        || format!("array requires '{}' fields", count),
                        loc.clone(),
                    );
                    for field in fields {
                        let field_ty = self.body.type_of_operand(field);
                        self.assert(
                            field_ty == *ty,
                            || "array field must have same type as array".to_string(),
                            loc.clone(),
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
                        loc.clone(),
                    );
                    let ptr_ty = self.body.type_of_operand(ptr);
                    self.assert(
                        ptr_ty == Type::pointer(Type::Byte),
                        || "ptr should be a byte pointer".to_string(),
                        loc.clone(),
                    );
                    let cap_ty = self.body.type_of_operand(cap);
                    self.assert(
                        cap_ty == Type::Int,
                        || "cap should be int".to_string(),
                        loc.clone(),
                    );
                    let len_ty = self.body.type_of_operand(len);
                    self.assert(
                        len_ty == Type::Int,
                        || "len should be int".to_string(),
                        loc.clone(),
                    );
                }
            },
            super::Rvalue::Use(_) => (),
            super::Rvalue::Call(operand, operands) => {
                let callee = self.body.type_of_operand(operand);
                let FunctionType {
                    resource, params, ..
                } = self.assert_with_some(
                    callee,
                    |ty| match ty {
                        Type::Function(function_type) => Some(function_type),
                        _ => None,
                    },
                    || "Can only call function types".to_string(),
                    loc.clone(),
                );
                self.assert(
                    resource == IsResource::Data,
                    || "Can only call data functions".to_string(),
                    loc.clone(),
                );
                let operand_tys = operands
                    .iter()
                    .map(|operand| self.body.type_of_operand(operand))
                    .collect::<Vec<_>>();
                self.assert(
                    operand_tys == params,
                    || format!("Expected '{:?}' but got '{:?}'", operand_tys, params),
                    loc,
                );
            }
            super::Rvalue::Binary(binary_op, left_and_right) => {
                let (left, right) = left_and_right.as_ref();
                match (
                    binary_op,
                    self.body.type_of_operand(left),
                    self.body.type_of_operand(right),
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
            super::Rvalue::PointerCast(pointer_cast, operand) => {
                let (pointer_type, _) = self.assert_with_some(
                    self.body.type_of_operand(operand),
                    |ty| ty.as_pointer_type().ok(),
                    || "Cannot take a non pointer type".to_string(),
                    loc.clone(),
                );
                match (pointer_cast, pointer_type) {
                    (PointerCast::BoxToRaw, PointerType::Box) => (),
                    (PointerCast::Freeze, PointerType::Reference(_, Mutable::Mutable)) => (),
                    (
                        PointerCast::RawToRaw | PointerCast::RawToBox | PointerCast::RawToRef(..),
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
            super::Rvalue::DecodeUtf8(ptr, index) => {
                let byte_ptr = self.body.type_of_operand(ptr);
                let index = self.body.type_of_operand(index);
                self.assert(
                    byte_ptr == Type::pointer(Type::Byte),
                    || "First operand for decode should be a byte pointer".to_string(),
                    loc.clone(),
                );
                self.assert(
                    index == Type::Int,
                    || "Second operand should be an index".to_string(),
                    loc,
                );
            }
            super::Rvalue::Len(place) => {
                let ty = self.body.type_of_place(place);
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
                let lhs_ty = self.body.type_of_place(lhs);
                let rhs_ty = self.body.type_of_rvalue(rhs);
                self.assert(
                    lhs_ty == rhs_ty,
                    || format!("Cannot assign non equal types {} and {}", lhs_ty, rhs_ty),
                    stmt.loc.clone(),
                );
            }
            StmtKind::Noop => (),
            StmtKind::Assert(operand, _) => {
                let condition_ty = self.body.type_of_operand(operand);
                self.assert(
                    condition_ty == Type::Bool,
                    || format!("Can only assert on bools not {}", condition_ty),
                    stmt.loc.clone(),
                );
            }
            StmtKind::Print(operand) => {
                if let Some(operand) = operand {
                    let ty = self.body.type_of_operand(operand);
                    self.assert(
                        !ty.is_resource(),
                        || format!("Cannot print resource {}", ty),
                        stmt.loc.clone(),
                    );
                }
            }
            StmtKind::Deallocate(operand) => {
                let pointer = self.body.type_of_operand(operand);
                self.assert(
                    pointer.as_pointer().is_none(),
                    || format!("Cannot deallocate {}", pointer),
                    stmt.loc.clone(),
                );
            }
        }
    }
}
