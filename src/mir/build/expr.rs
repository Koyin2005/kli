use std::collections::HashMap;

use crate::{
    ast::{BinaryOp, IsResource},
    index_vec::IndexVec,
    mir::{
        self, AggregateKind, Constant, ConstantValue, Local, Operand, OverflowOp, Place,
        PointerCast, Rvalue, build::Builder,
    },
    resolved_ast::Builtin,
    src_loc::SrcLoc,
    typed_ast::{self, Expr, ExprKind, FieldId, Pattern},
    types::{FunctionType, LIST_LEN_FIELD, Type},
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
            ExprKind::Function(_, id, ref generic_args) => {
                let ty = expr.ty.clone();
                Some(Constant {
                    ty: Box::new(ty),
                    value: ConstantValue::Function(id, generic_args.clone()),
                })
            }
            ExprKind::Lambda(ref lambda) if lambda.is_resource == IsResource::Data => {
                Some(self.lambda_code(lambda))
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
                let local = self.body.local_for_var(var.1).unwrap();
                Place::local(local)
            }
            typed_ast::PlaceKind::Upvar(var) => {
                let capture_info = self.body.capture_info.as_ref().unwrap();
                let env_ptr = capture_info.env_ptr.unwrap();
                let index = capture_info
                    .captures
                    .iter()
                    .position(|(curr, _)| curr.1 == var.1)
                    .unwrap();
                Place::local(env_ptr)
                    .with_deref()
                    .with_field(FieldId::new(index))
            }

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
            typed_ast::PatternKind::Binding(None, _, var, ty) => {
                let place = Place::local(self.new_var(var.clone(), (**ty).clone()));
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
            typed_ast::PatternKind::Binding(borrowed, _, var, ty) => {
                let var_place = Place::local(self.new_var(var.clone(), (**ty).clone()));
                if let Some((mutable, region)) = borrowed {
                    self.assign(
                        pattern.loc.clone(),
                        var_place,
                        Rvalue::Ref(*mutable, region.clone(), place),
                    );
                    return;
                }
                self.assign(
                    pattern.loc.clone(),
                    var_place,
                    Rvalue::Use(Operand::Load(place)),
                );
            }
            typed_ast::PatternKind::Ref(pattern) => {
                self.assign_place_to_pattern(pattern, place.with_deref());
            }
            typed_ast::PatternKind::Bool(_)
            | typed_ast::PatternKind::Int(_)
            | typed_ast::PatternKind::None => (),
            typed_ast::PatternKind::Some(pattern) => {
                self.assign_place_to_pattern(pattern, place.with_downcast_some());
            }
            typed_ast::PatternKind::Record(fields) => {
                for field in fields {
                    self.assign_place_to_pattern(
                        &field.pattern,
                        place.clone().with_field(field.index),
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
                self.panic(expr.loc.clone());
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
            | ExprKind::Some(_)
            | ExprKind::None
            | ExprKind::String(_)
            | ExprKind::Lambda(_)
            | ExprKind::BuiltinCall(..) => {
                let rvalue = self.build_rvalue(expr);
                self.assign(expr.loc.clone(), dest, rvalue);
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
            Builtin::Allocate => BuiltinResult::Rvalue(Rvalue::Allocate {
                ty: ty.as_pointer().cloned().expect("should be a pointer"),
                count: { operands }.swap_remove(0),
            }),
            Builtin::Deallocate => {
                self.push_stmt(loc, mir::StmtKind::Deallocate({ operands }.swap_remove(0)));
                BuiltinResult::Unit
            }
            Builtin::Freeze => BuiltinResult::Rvalue(Rvalue::PointerCast(
                PointerCast::Freeze,
                { operands }.swap_remove(0),
            )),
            Builtin::BoxFromRaw => BuiltinResult::Rvalue(Rvalue::PointerCast(
                PointerCast::RawToBox,
                { operands }.swap_remove(0),
            )),
            Builtin::BoxIntoRaw => BuiltinResult::Rvalue(Rvalue::PointerCast(
                PointerCast::BoxToRaw,
                { operands }.swap_remove(0),
            )),
            Builtin::RefFromRaw(mutable) => {
                let (_, region, _) = ty.as_reference_type().expect("should be a reference type");
                BuiltinResult::Rvalue(Rvalue::PointerCast(
                    PointerCast::RawToRef(mutable, region.clone()),
                    { operands }.swap_remove(0),
                ))
            }
            Builtin::RefIntoRaw(mutable) => BuiltinResult::Rvalue(Rvalue::PointerCast(
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
                    .operand_as_place(loc.clone(), args[0].ty.clone(), ptr)
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
            | ExprKind::Function(..) => {
                let operand = self
                    .as_operand(expr)
                    .unwrap_or_else(|| unreachable!("Should be an operand '{:?}' ", expr));
                Rvalue::Use(operand)
            }
            ExprKind::Record(fields) => {
                let mut field_map = fields
                    .iter()
                    .map(|field| {
                        (
                            field.index,
                            (field.name.clone(), self.operand(&field.value)),
                        )
                    })
                    .collect::<HashMap<_, _>>();
                let mut field_names = IndexVec::new();
                let fields = (0..fields.len())
                    .map(|field| {
                        let (name, operand) = field_map.remove(&FieldId::new(field)).unwrap();
                        field_names.push(name.content);
                        operand
                    })
                    .collect::<IndexVec<FieldId, _>>();
                Rvalue::Aggregate(AggregateKind::Record { field_names }, fields)
            }
            ExprKind::String(value) => {
                let len = value.len().try_into().unwrap();
                let array_ty = Type::Array(Box::new(Type::Byte), len);
                let bytes = self.assign_to_temp(
                    expr.loc.clone(),
                    Type::pointer(array_ty.clone()),
                    Rvalue::Allocate {
                        ty: array_ty.clone(),
                        count: Operand::Constant(Constant::int(1)),
                    },
                );
                self.assign(
                    expr.loc.clone(),
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
                    expr.loc.clone(),
                    Type::pointer(Type::Byte),
                    Rvalue::PointerCast(
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
            ExprKind::None => {
                let Type::Option(ty) = expr.ty.clone() else {
                    unreachable!("Should be an option")
                };
                Rvalue::Aggregate(
                    AggregateKind::Option {
                        inner: *ty,
                        is_some: false,
                    },
                    IndexVec::new(),
                )
            }
            ExprKind::Some(value) => {
                let operand = self.operand(value);
                Rvalue::Aggregate(
                    AggregateKind::Option {
                        inner: value.ty.clone(),
                        is_some: true,
                    },
                    [operand].into(),
                )
            }
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
                    expr.loc.clone(),
                    Type::pointer(array_ty.clone()),
                    Rvalue::Allocate {
                        ty: array_ty.clone(),
                        count: Operand::Constant(Constant::int(1)),
                    },
                );
                let operands = exprs.iter().map(|expr| self.operand(expr)).collect();
                self.assign(
                    expr.loc.clone(),
                    Place::local(ptr_to_buf).with_deref(),
                    Rvalue::Aggregate(
                        AggregateKind::Array(ty.clone(), exprs.len().try_into().unwrap()),
                        operands,
                    ),
                );
                let ptr = self.assign_to_temp(
                    expr.loc.clone(),
                    Type::pointer(ty.clone()),
                    Rvalue::PointerCast(
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
                            let closure_place = self.operand_as_place(
                                callee.loc.clone(),
                                callee.ty.clone(),
                                callee_value,
                            );
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
                            expr.loc.clone(),
                            right_operand.clone(),
                            Operand::Constant(Constant::int(0)),
                        );
                        self.assert(
                            expr.loc.clone(),
                            Operand::Load(Place::local(is_zero)),
                            mir::AssertKind::DivideByZero,
                        );
                        let is_left_min = self.assign_equals(
                            expr.loc.clone(),
                            left_operand.clone(),
                            Operand::Constant(Constant::int(ConstantValue::MIN_INT)),
                        );
                        let is_right_neg_1 = self.assign_equals(
                            expr.loc.clone(),
                            left_operand.clone(),
                            Operand::Constant(Constant::int(-1)),
                        );
                        let overflow = self.assign_binary_result(
                            expr.loc.clone(),
                            Type::Bool,
                            mir::BinaryOp::BitwiseAnd,
                            Operand::Load(Place::local(is_left_min)),
                            Operand::Load(Place::local(is_right_neg_1)),
                        );
                        self.assert(
                            expr.loc.clone(),
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
                    expr.loc.clone(),
                    Type::record(vec![Type::Bool, Type::Int]),
                    Rvalue::Binary(
                        mir::BinaryOp::Overflow(overflow_op),
                        Box::new((left_operand, right_operand)),
                    ),
                );
                let overflow =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(0)));
                self.assert(
                    expr.loc.clone(),
                    overflow,
                    mir::AssertKind::Overflow(overflow_op),
                );
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
            ExprKind::Borrow {
                mutable,
                place,
                region,
            } => Rvalue::Ref(*mutable, region.clone(), self.lower_place(place)),
            ExprKind::Lambda(lambda) => {
                let is_resource = lambda.is_resource == IsResource::Resource;
                let function = Operand::Constant(self.lambda_code(lambda));
                if is_resource {
                    let env_ty =
                        Type::record(lambda.captures.iter().map(|(_, ty)| ty.clone()).collect());

                    let env = self.assign_to_temp(
                        expr.loc.clone(),
                        Type::pointer(env_ty.clone()),
                        Rvalue::Allocate {
                            ty: env_ty,
                            count: Operand::Constant(Constant::int(1)),
                        },
                    );
                    self.assign(
                        expr.loc.clone(),
                        Place::local(env).with_deref(),
                        Rvalue::Aggregate(
                            AggregateKind::Record {
                                field_names: lambda
                                    .captures
                                    .iter()
                                    .map(|capture| capture.0.0.clone())
                                    .collect(),
                            },
                            lambda
                                .captures
                                .iter()
                                .map(|capture| {
                                    Operand::Load(Place::local(
                                        self.body
                                            .local_for_var(capture.0.1)
                                            .expect("Should have a local for var"),
                                    ))
                                })
                                .collect(),
                        ),
                    );

                    let erased_env = self.assign_to_temp(
                        expr.loc.clone(),
                        Type::pointer(Type::Byte),
                        Rvalue::PointerCast(
                            PointerCast::RawToRaw(Type::Byte),
                            Operand::Load(Place::local(env)),
                        ),
                    );
                    let param_tys = lambda.params.iter().map(|param| param.ty.clone()).collect();
                    Rvalue::Aggregate(
                        AggregateKind::Closure(param_tys, Box::new(lambda.return_type.clone())),
                        [Operand::Load(Place::local(erased_env)), function]
                            .into_iter()
                            .collect(),
                    )
                } else {
                    Rvalue::Use(function)
                }
            }
            &ExprKind::BuiltinCall(builtin, _, ref args) => self
                .builtin_call(expr.loc.clone(), &expr.ty, builtin, args)
                .into(),
        }
    }
}
