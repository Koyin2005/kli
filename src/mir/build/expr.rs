use std::collections::HashMap;

use crate::{
    ast::{BinaryOp, IsResource},
    index_vec::IndexVec,
    mir::{
        self, AggregateKind, Constant, ConstantValue, Local, Operand, OverflowOp, Place, Rvalue,
        build::Builder,
    },
    typed_ast::{self, Expr, ExprKind, FieldId, Pattern},
    types::{FunctionType, Type},
};

impl Builder<'_> {
    fn as_constant(&mut self, expr: &Expr) -> Option<Constant> {
        match expr.kind {
            ExprKind::Bool(value) => Some(Constant::bool(value)),
            ExprKind::Int(value) => Some(Constant::int(value)),
            ExprKind::Unit => Some(Constant::unit()),
            ExprKind::Function(_, id, ref generic_args) => {
                let ty = expr.ty.clone();
                Some(Constant {
                    ty,
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
    fn operand_as_place(&mut self, ty: Type, operand: Operand) -> Place {
        match operand {
            Operand::Load(place) => place,
            Operand::Constant(_) => Place::local(self.assign_to_temp(ty, Rvalue::Use(operand))),
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
                let index = self
                    .body
                    .captures
                    .iter()
                    .position(|(curr, _)| curr.1 == var.1)
                    .unwrap();
                Place::local(Local::zero()).with_field(FieldId::new(index))
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
                if let Some(borrowed) = borrowed {
                    self.assign(var_place, Rvalue::Ref(*borrowed, place));
                    return;
                }
                self.assign(var_place, Rvalue::Use(Operand::Load(place)));
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
                self.panic();
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
            | ExprKind::Case(..)
            | ExprKind::Builtin(..) => {
                let rvalue = self.build_rvalue(expr);
                self.assign(dest, rvalue);
            }
        }
    }
    fn binary_op_rvalue(op: mir::BinaryOp, left: Operand, right: Operand) -> Rvalue {
        Rvalue::Binary(op, Box::new((left, right)))
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
            ExprKind::String(_) => todo!("Strings"),
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
            ExprKind::Builtin(..) => todo!("Builtins"),
            ExprKind::List(exprs) => {
                let ty = if let Type::List(ty) = &expr.ty {
                    (**ty).clone()
                } else {
                    unreachable!("Should be an array")
                };
                let len_constant =
                    Operand::Constant(Constant::int(exprs.len().try_into().unwrap()));
                let new_array = self.assign_to_temp(
                    expr.ty.clone(),
                    Rvalue::AllocateArray(ty, len_constant.clone()),
                );
                for (i, expr) in exprs.iter().enumerate() {
                    let index: u32 = i.try_into().unwrap();
                    self.expr_into_dest(Place::local(new_array).with_constant_index(index), expr);
                }
                self.assign(
                    Place::local(new_array).with_len(),
                    Rvalue::Use(len_constant),
                );
                Rvalue::Use(Operand::Load(Place::local(new_array)))
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
                                self.operand_as_place(callee.ty.clone(), callee_value);
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
                        let is_zero = self.assign_to_temp(
                            Type::Bool,
                            Self::binary_op_rvalue(
                                mir::BinaryOp::Equals,
                                right_operand.clone(),
                                Operand::Constant(Constant::int(0)),
                            ),
                        );
                        self.assert(
                            Operand::Load(Place::local(is_zero)),
                            mir::AssertKind::DivideByZero,
                        );
                        let is_left_min = self.assign_to_temp(
                            Type::Bool,
                            Self::binary_op_rvalue(
                                mir::BinaryOp::Equals,
                                left_operand.clone(),
                                Operand::Constant(Constant::int(ConstantValue::MIN_INT)),
                            ),
                        );
                        let is_right_neg_1 = self.assign_to_temp(
                            Type::Bool,
                            Self::binary_op_rvalue(
                                mir::BinaryOp::Equals,
                                left_operand.clone(),
                                Operand::Constant(Constant::int(-1)),
                            ),
                        );
                        let overflow = self.assign_to_temp(
                            Type::Bool,
                            Self::binary_op_rvalue(
                                mir::BinaryOp::BitwiseAnd,
                                Operand::Load(Place::local(is_left_min)),
                                Operand::Load(Place::local(is_right_neg_1)),
                            ),
                        );
                        self.assert(
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
                    Type::record(vec![Type::Bool, Type::Int]),
                    Rvalue::Binary(
                        mir::BinaryOp::Overflow(overflow_op),
                        Box::new((left_operand, right_operand)),
                    ),
                );
                let overflow =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(0)));
                self.assert(overflow, mir::AssertKind::Overflow(overflow_op));
                let result =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(1)));
                Rvalue::Use(result)
            }
            ExprKind::Block(..) | ExprKind::Panic => {
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
                region: _,
            } => Rvalue::Ref(*mutable, self.lower_place(place)),
            ExprKind::Case(..) => todo!("case"),
            ExprKind::Lambda(lambda) => {
                let is_resource = lambda.is_resource == IsResource::Resource;
                let function = Operand::Constant(self.lambda_code(lambda));
                if is_resource {
                    let env = self.assign_to_temp(
                        Type::RawPointer,
                        Rvalue::AllocateEnv(
                            lambda
                                .captures
                                .iter()
                                .map(|(var, _)| {
                                    (
                                        var.clone(),
                                        Operand::Load(Place::local(
                                            self.body
                                                .local_for_var(var.1)
                                                .expect("Should have a local for var"),
                                        )),
                                    )
                                })
                                .collect(),
                        ),
                    );
                    Rvalue::Aggregate(
                        AggregateKind::Closure,
                        [Operand::Load(Place::local(env)), function]
                            .into_iter()
                            .collect(),
                    )
                } else {
                    Rvalue::Use(function)
                }
            }
        }
    }
}
