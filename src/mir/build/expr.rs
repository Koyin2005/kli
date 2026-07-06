use std::collections::HashMap;

use crate::{
    ast::{BinaryOp, IsResource},
    index_vec::IndexVec,
    mir::{
        self, AggregateKind, Constant, ConstantValue, CopyNonOverlapping, Local, Operand,
        OverflowOp, Place, PointerCast, Rvalue, build::Builder,
    },
    resolved_ast::Builtin,
    src_loc::SrcLoc,
    typed_ast::{self, Expr, ExprKind, FieldId, Pattern},
    types::{FieldName, FunctionType, LIST_LEN_FIELD, Type},
};
pub(super) enum BuiltinResult {
    Rvalue(Rvalue),
    Unit,
}
impl From<BuiltinResult> for Rvalue {
    fn from(value: BuiltinResult) -> Self {
        match value {
            BuiltinResult::Rvalue(value) => value,
            BuiltinResult::Unit => Rvalue::Use(Operand::Constant(Constant::unit())),
        }
    }
}
impl Builder<'_> {
    fn as_constant(&mut self, expr: &Expr) -> Option<Constant> {
        match expr.kind {
            ExprKind::Bool(value) => Some(Constant::bool(value)),
            ExprKind::Int(value) => Some(Constant::int(value)),
            ExprKind::Unit => Some(Constant::unit()),
            ExprKind::Function(id, ref generic_args) => {
                let ty = expr.ty.clone();
                Some(Constant {
                    ty: Box::new(ty),
                    value: ConstantValue::NamedConst(id, generic_args.clone()),
                })
            }
            ExprKind::Lambda(ref lambda) if lambda.is_resource == IsResource::Data => {
                Some(Self::lambda_code_constant(self.ctxt, lambda))
            }
            ExprKind::Const(id, ref args) => {
                let ty = expr.ty.clone();
                Some(Constant {
                    ty: Box::new(ty),
                    value: ConstantValue::NamedConst(id, args.clone()),
                })
            }
            _ => None,
        }
    }
    fn as_place(&mut self, expr: &Expr) -> Option<Place> {
        if let ExprKind::Load(place) = &expr.kind {
            Some(self.lower_place(place))
        } else {
            None
        }
    }
    pub(super) fn as_operand(&mut self, expr: &Expr) -> Option<Operand> {
        if let Some(constant) = self.as_constant(expr) {
            Some(Operand::Constant(constant))
        } else {
            self.as_place(expr).map(Operand::Load)
        }
    }
    pub(super) fn place(&mut self, expr: &Expr) -> Place {
        if let Some(place) = self.as_place(expr) {
            place
        } else {
            Place::local(self.expr_into_temp(expr))
        }
    }
    pub(super) fn len_operand(&mut self, loc: SrcLoc, ty: &Type, place: Place) -> Operand {
        if let Type::List(_) = ty {
            Operand::Load(Place::local(self.assign_to_temp(
                loc,
                Type::Int,
                Rvalue::Use(Operand::Load(place.with_field(LIST_LEN_FIELD))),
            )))
        } else {
            Operand::Load(Place::local(self.assign_to_temp(
                loc,
                Type::Int,
                Rvalue::Len(place),
            )))
        }
    }
    fn operand_as_place(&mut self, loc: SrcLoc, ty: Type, operand: Operand) -> Place {
        match operand {
            Operand::Load(place) => place,
            Operand::Constant(_) => {
                Place::local(self.assign_to_temp(loc, ty, Rvalue::Use(operand)))
            }
        }
    }
    pub(super) fn operand(&mut self, expr: &Expr) -> Operand {
        if let Some(operand) = self.as_operand(expr) {
            operand
        } else {
            Operand::Load(Place::local(self.expr_into_temp(expr)))
        }
    }
    pub(super) fn lower_place(&mut self, place: &typed_ast::Place) -> Place {
        match &place.kind {
            typed_ast::PlaceKind::Var(var) => {
                let Some(local) = self.body.local_for_var(var.1) else {
                    unreachable!("should have a local for {:?} at {:?}", var, place.loc)
                };
                Place::local(local)
            }
            typed_ast::PlaceKind::Upvar(id, var) => Place::local(Local::new(
                self.ctxt
                    .captures(*id)
                    .unwrap_or_default()
                    .capture_index(var.1)
                    .unwrap(),
            )),
            typed_ast::PlaceKind::Field(place, field) => self.lower_place(place).with_field(*field),
            typed_ast::PlaceKind::Invalid => unreachable!("cannot lower invalid place"),
            typed_ast::PlaceKind::Deref(value) => self.place(value).with_deref(),
        }
    }
    pub(super) fn expr_into_temp(&mut self, expr: &Expr) -> Local {
        let temp = self.new_temp(expr.ty.clone());
        self.expr_into_dest(Place::local(temp), expr);
        temp
    }
    fn assign_to_pattern(&mut self, pattern: &Pattern, value: &Expr) {
        match &pattern.kind {
            &typed_ast::PatternKind::Binding(None, _, var, ref ty) => {
                let place = Place::local(self.new_var(var, (**ty).clone()));
                self.expr_into_dest(place, value);
            }
            _ => {
                let place = Place::local(self.expr_into_temp(value));
                self.assign_place_to_pattern(pattern, place);
            }
        }
    }
    pub(super) fn assign_place_to_pattern(&mut self, pattern: &Pattern, place: Place) {
        match &pattern.kind {
            &typed_ast::PatternKind::Binding(borrowed, _, var, ref ty) => {
                let var_place = Place::local(self.new_var(var, (**ty).clone()));
                if let Some((mutable, region)) = borrowed {
                    self.assign(pattern.loc, var_place, Rvalue::Ref(mutable, region, place));
                    return;
                }
                self.assign(pattern.loc, var_place, Rvalue::Use(Operand::Load(place)));
            }
            typed_ast::PatternKind::Ref(pattern) => {
                self.assign_place_to_pattern(pattern, place.with_deref());
            }
            typed_ast::PatternKind::Bool(_)
            | typed_ast::PatternKind::Int(_)
            | typed_ast::PatternKind::Unit => (),
            typed_ast::PatternKind::Record(fields) => {
                for field in fields {
                    self.assign_place_to_pattern(
                        &field.pattern,
                        place.clone().with_field(field.index),
                    );
                }
            }
            typed_ast::PatternKind::Err => unreachable!(),
            typed_ast::PatternKind::Case(id, _, index, inner) => {
                if let Some(inner) = inner {
                    self.assign_place_to_pattern(
                        inner,
                        place.with_case_downcast(*index, self.ctxt.expect_ident(*id).symbol),
                    );
                }
            }
        }
    }
    pub fn stmt(&mut self, stmt: &typed_ast::Stmt) {
        match &stmt.kind {
            typed_ast::StmtKind::Expr(expr) => {
                self.expr_stmt(expr);
            }
            typed_ast::StmtKind::Let(binding) => {
                self.assign_to_pattern(&binding.pattern, &binding.value);
            }
        }
    }
    pub fn expr_into_dest(&mut self, dest: Place, expr: &Expr) {
        match &expr.kind {
            ExprKind::Err => unreachable!("Cannot have err here"),
            ExprKind::Block(block_body, ..) => {
                for stmt in block_body.stmts.iter() {
                    self.stmt(stmt);
                }
                self.expr_into_dest(dest, &block_body.expr);
            }
            ExprKind::Panic => {
                self.panic(expr.loc);
            }
            ExprKind::Case(expr, arms) => {
                self.build_match(dest, expr, arms);
            }
            ExprKind::Record(_)
            | ExprKind::Function(..)
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Unit
            | ExprKind::Load(_)
            | ExprKind::Call(..)
            | ExprKind::Binary(..)
            | ExprKind::List(..)
            | ExprKind::Print(_)
            | ExprKind::For { .. }
            | ExprKind::Assign(..)
            | ExprKind::Borrow { .. }
            | ExprKind::VariantInit(..)
            | ExprKind::String(_)
            | ExprKind::Lambda(_)
            | ExprKind::BuiltinCall(..)
            | ExprKind::Const(..)
            | ExprKind::AddressOf(..)
            | ExprKind::NamedRecord(..) => {
                let rvalue = self.build_rvalue(expr);
                self.assign(expr.loc, dest, rvalue);
            }
        }
    }
    fn binary_op_rvalue(op: mir::BinaryOp, left: Operand, right: Operand) -> Rvalue {
        Rvalue::Binary(op, Box::new((left, right)))
    }
    pub(super) fn builtin_call(
        &mut self,
        loc: SrcLoc,
        ty: &Type,
        builtin: Builtin,
        args: &[Expr],
    ) -> BuiltinResult {
        let operands = args
            .iter()
            .map(|operand| self.operand(operand))
            .collect::<Vec<_>>();
        match builtin {
            Builtin::Memcopy => {
                let [dst, src, count] = operands.try_into().unwrap();
                self.push_stmt(
                    loc,
                    mir::StmtKind::CopyNonOverlapping(Box::new(CopyNonOverlapping {
                        dst,
                        src,
                        count,
                    })),
                );
                BuiltinResult::Unit
            }
            Builtin::Offset => {
                let [first, second] = operands.try_into().unwrap();
                BuiltinResult::Rvalue(Rvalue::Binary(
                    mir::BinaryOp::Offset,
                    Box::new((first, second)),
                ))
            }
            Builtin::Transmute => BuiltinResult::Rvalue(Rvalue::Cast(
                mir::CastKind::Transmute(ty.clone()),
                { operands }.swap_remove(0),
            )),
            Builtin::Allocate => BuiltinResult::Rvalue(Rvalue::Allocate {
                ty: ty.as_pointer().cloned().expect("should be a pointer"),
                count: { operands }.swap_remove(0),
            }),
            Builtin::Deallocate => {
                self.push_stmt(loc, mir::StmtKind::Deallocate({ operands }.swap_remove(0)));
                BuiltinResult::Unit
            }
            Builtin::Freeze => BuiltinResult::Rvalue(Rvalue::pointer_cast(
                PointerCast::Freeze,
                { operands }.swap_remove(0),
            )),
            Builtin::BoxFromRaw => BuiltinResult::Rvalue(Rvalue::pointer_cast(
                PointerCast::RawToBox,
                { operands }.swap_remove(0),
            )),
            Builtin::BoxIntoRaw => BuiltinResult::Rvalue(Rvalue::pointer_cast(
                PointerCast::BoxToRaw,
                { operands }.swap_remove(0),
            )),
            Builtin::RefFromRaw(mutable) => {
                let (_, region, _) = ty.as_reference_type().expect("should be a reference type");
                BuiltinResult::Rvalue(Rvalue::pointer_cast(
                    PointerCast::RawToRef(mutable, region),
                    { operands }.swap_remove(0),
                ))
            }
            Builtin::RefIntoRaw(mutable) => BuiltinResult::Rvalue(Rvalue::pointer_cast(
                PointerCast::RefToRaw(mutable),
                { operands }.swap_remove(0),
            )),
            Builtin::PtrRead => {
                let [ptr] = { operands }.try_into().unwrap();
                let deref = self
                    .operand_as_place(loc, args[0].ty.clone(), ptr)
                    .with_deref();
                BuiltinResult::Rvalue(Rvalue::Use(Operand::Load(deref)))
            }
            Builtin::PtrWrite => {
                let [ptr, value] = { operands }.try_into().unwrap();
                let deref = self
                    .operand_as_place(loc, args[0].ty.clone(), ptr)
                    .with_deref();
                self.assign(loc, deref, Rvalue::Use(value));
                BuiltinResult::Unit
            }
        }
    }
    pub fn build_rvalue(&mut self, expr: &Expr) -> Rvalue {
        match &expr.kind {
            ExprKind::Err => unreachable!("Cannot have err here"),
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Bool(_)
            | ExprKind::Load(_)
            | ExprKind::Function(..)
            | ExprKind::Const(..) => {
                let operand = self
                    .as_operand(expr)
                    .unwrap_or_else(|| unreachable!("should be an constant operand '{:?}' ", expr));
                Rvalue::Use(operand)
            }
            ExprKind::NamedRecord(id, generic_args, fields) => {
                let mut field_map = fields
                    .iter()
                    .map(|field| (field.index, self.operand(&field.value)))
                    .collect::<HashMap<_, _>>();
                let fields = (0..fields.len())
                    .map(FieldId::new)
                    .map(|field| field_map.remove(&field).unwrap())
                    .collect::<IndexVec<FieldId, _>>();
                Rvalue::Aggregate(
                    AggregateKind::NamedRecord(*id, generic_args.clone()),
                    fields,
                )
            }
            ExprKind::Record(fields) => {
                let mut field_map = fields
                    .iter()
                    .map(|field| (field.index, self.operand(&field.value)))
                    .collect::<HashMap<_, _>>();
                let fields = (0..fields.len())
                    .map(FieldId::new)
                    .map(|field| field_map.remove(&field).unwrap())
                    .collect::<IndexVec<FieldId, _>>();

                let Type::Record(ref rec_fields) = expr.ty else {
                    unreachable!("Should be a record")
                };
                let field_names = rec_fields.iter().map(|field| field.name).collect();
                Rvalue::Aggregate(AggregateKind::Record { field_names }, fields)
            }
            ExprKind::String(value) => {
                let len = value.len().try_into().unwrap();
                let array_ty = Type::Array(Box::new(Type::Byte), len);
                let bytes = self.assign_to_temp(
                    expr.loc,
                    Type::pointer(array_ty.clone()),
                    Rvalue::Allocate {
                        ty: array_ty.clone(),
                        count: Operand::Constant(Constant::int(1)),
                    },
                );
                self.assign(
                    expr.loc,
                    Place::local(bytes).with_deref(),
                    Rvalue::Aggregate(
                        AggregateKind::Array(Type::Byte, len),
                        value
                            .bytes()
                            .map(|b| Operand::Constant(Constant::byte(b)))
                            .collect(),
                    ),
                );
                let ptr = self.assign_to_temp(
                    expr.loc,
                    Type::pointer(Type::Byte),
                    Rvalue::pointer_cast(
                        PointerCast::RawToRaw(Type::Byte),
                        Operand::Load(Place::local(bytes)),
                    ),
                );
                Rvalue::Aggregate(
                    AggregateKind::String,
                    [
                        Operand::Load(Place::local(ptr)),
                        Operand::Constant(Constant::int(value.len().try_into().unwrap())),
                        Operand::Constant(Constant::int(value.len().try_into().unwrap())),
                    ]
                    .into(),
                )
            }
            &ExprKind::VariantInit(id, index, ref args, ref value) => Rvalue::Aggregate(
                AggregateKind::Variant(id, index, args.clone()),
                [self.operand(value)].into(),
            ),
            ExprKind::List(exprs) => {
                let ty = if let Type::List(ty) = &expr.ty {
                    (**ty).clone()
                } else {
                    unreachable!("Should be an array")
                };
                let array_ty = Type::Array(Box::new(ty.clone()), exprs.len().try_into().unwrap());
                let len_constant =
                    Operand::Constant(Constant::int(exprs.len().try_into().unwrap()));
                let ptr_to_buf = self.assign_to_temp(
                    expr.loc,
                    Type::pointer(array_ty.clone()),
                    Rvalue::Allocate {
                        ty: array_ty.clone(),
                        count: Operand::Constant(Constant::int(1)),
                    },
                );
                let operands = exprs.iter().map(|expr| self.operand(expr)).collect();
                self.assign(
                    expr.loc,
                    Place::local(ptr_to_buf).with_deref(),
                    Rvalue::Aggregate(
                        AggregateKind::Array(ty.clone(), exprs.len().try_into().unwrap()),
                        operands,
                    ),
                );
                let ptr = self.assign_to_temp(
                    expr.loc,
                    Type::pointer(ty.clone()),
                    Rvalue::pointer_cast(
                        PointerCast::RawToRaw(ty.clone()),
                        Operand::Load(Place::local(ptr_to_buf)),
                    ),
                );
                Rvalue::Aggregate(
                    AggregateKind::ArrayList(ty),
                    [
                        Operand::Load(Place::local(ptr)),
                        len_constant.clone(),
                        len_constant.clone(),
                    ]
                    .into(),
                )
            }
            ExprKind::Call(callee, args) => match &callee.ty {
                Type::Function(function_ty) => {
                    let FunctionType { resource, .. } = function_ty;
                    let callee_value = self.operand(callee);
                    let arg_values = args.iter().map(|arg| self.operand(arg)).collect::<Vec<_>>();
                    match resource {
                        IsResource::Data => Rvalue::Call(callee_value, arg_values),
                        IsResource::Resource => {
                            let closure_place =
                                self.operand_as_place(callee.loc, callee.ty.clone(), callee_value);
                            let env = closure_place.clone().with_field(FieldId::new(0));
                            let code = closure_place.clone().with_field(FieldId::new(1));
                            let mut arg_values = arg_values;
                            arg_values.insert(0, Operand::Load(env));
                            Rvalue::Call(Operand::Load(code), arg_values)
                        }
                    }
                }
                _ => unreachable!("Can't call non function at {:?}", expr.loc),
            },
            ExprKind::Binary(binary_op, left, right) => {
                let left_operand = self.operand(left);
                let right_operand = self.operand(right);
                let overflow_op = match binary_op {
                    BinaryOp::Add => OverflowOp::Add,
                    BinaryOp::Divide => {
                        //Division can fail in 2 ways
                        //Divide by zero
                        //Divide int min by -1
                        let is_zero = self.assign_equals(
                            expr.loc,
                            right_operand.clone(),
                            Operand::Constant(Constant::int(0)),
                        );
                        self.assert(
                            expr.loc,
                            Operand::Load(Place::local(is_zero)),
                            mir::AssertKind::DivideByZero,
                        );
                        let is_left_min = self.assign_equals(
                            expr.loc,
                            left_operand.clone(),
                            Operand::Constant(Constant::int(ConstantValue::MIN_INT)),
                        );
                        let is_right_neg_1 = self.assign_equals(
                            expr.loc,
                            left_operand.clone(),
                            Operand::Constant(Constant::int(-1)),
                        );
                        let overflow = self.assign_binary_result(
                            expr.loc,
                            Type::Bool,
                            mir::BinaryOp::BitwiseAnd,
                            Operand::Load(Place::local(is_left_min)),
                            Operand::Load(Place::local(is_right_neg_1)),
                        );
                        self.assert(
                            expr.loc,
                            Operand::Load(Place::local(overflow)),
                            mir::AssertKind::DivideOverflow,
                        );
                        return Self::binary_op_rvalue(
                            mir::BinaryOp::Divide,
                            left_operand,
                            right_operand,
                        );
                    }
                    BinaryOp::Subtract => OverflowOp::Subtract,
                    BinaryOp::Multiply => OverflowOp::Multiply,
                };
                let checked_result = self.assign_to_temp(
                    expr.loc,
                    Type::tuple(vec![Type::Bool, Type::Int]),
                    Rvalue::Binary(
                        mir::BinaryOp::Overflow(overflow_op),
                        Box::new((left_operand, right_operand)),
                    ),
                );
                let overflow =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(0)));
                self.assert(expr.loc, overflow, mir::AssertKind::Overflow(overflow_op));
                let result =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(1)));
                Rvalue::Use(result)
            }
            ExprKind::Block(..) | ExprKind::Panic | ExprKind::Case(..) => {
                let temp = self.expr_into_temp(expr);
                Rvalue::Use(Operand::Load(Place::local(temp)))
            }
            ExprKind::For { .. } | ExprKind::Print(_) | ExprKind::Assign(..) => {
                self.expr_stmt(expr);
                Rvalue::Use(Operand::Constant(Constant::unit()))
            }
            &ExprKind::Borrow {
                mutable,
                ref place,
                region,
            } => Rvalue::Ref(mutable, region, self.lower_place(place)),
            ExprKind::AddressOf(place) => Rvalue::RawPtrTo(self.lower_place(place)),
            ExprKind::Lambda(lambda) => {
                let is_resource = lambda.is_resource == IsResource::Resource;
                let function = if !is_resource {
                    Operand::Constant(Self::lambda_code_constant(self.ctxt, lambda))
                } else {
                    Operand::Constant(Self::closure_shim(
                        self.mir_context,
                        self.ctxt,
                        lambda.id,
                        lambda,
                    ))
                };
                if is_resource {
                    let env_ty = Type::closure_env(lambda.captures.iter().cloned());
                    let env = self.assign_to_temp(
                        expr.loc,
                        Type::pointer(env_ty.clone()),
                        Rvalue::Allocate {
                            ty: env_ty,
                            count: Operand::Constant(Constant::int(1)),
                        },
                    );
                    self.assign(
                        expr.loc,
                        Place::local(env).with_deref(),
                        Rvalue::Aggregate(
                            AggregateKind::Record {
                                field_names: lambda
                                    .captures
                                    .iter()
                                    .map(|capture| capture.var.0)
                                    .map(FieldName::Named)
                                    .collect(),
                            },
                            lambda
                                .captures
                                .iter()
                                .map(|capture| {
                                    Operand::Load(Place::local(
                                        self.body.local_for_var(capture.var.1).map_or_else(
                                            || {
                                                Local::new(
                                                    self.ctxt
                                                        .captures(lambda.id)
                                                        .unwrap_or_default()
                                                        .capture_index(capture.var.1)
                                                        .unwrap(),
                                                )
                                            },
                                            std::convert::identity,
                                        ),
                                    ))
                                })
                                .collect(),
                        ),
                    );

                    let erased_env = self.assign_to_temp(
                        expr.loc,
                        Type::pointer(Type::Byte),
                        Rvalue::pointer_cast(
                            PointerCast::RawToRaw(Type::Byte),
                            Operand::Load(Place::local(env)),
                        ),
                    );
                    let param_tys = lambda.param_tys.clone();
                    Rvalue::Aggregate(
                        AggregateKind::Closure(param_tys, lambda.return_type.clone()),
                        [Operand::Load(Place::local(erased_env)), function]
                            .into_iter()
                            .collect(),
                    )
                } else {
                    Rvalue::Use(function)
                }
            }
            &ExprKind::BuiltinCall(builtin, _, ref args) => {
                self.builtin_call(expr.loc, &expr.ty, builtin, args).into()
            }
        }
    }
}
