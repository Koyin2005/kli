use std::collections::HashMap;

use crate::{
    Symbol,
    builtins::Builtin,
    index_vec::IndexVec,
    mir::{
        self, AggregateKind, ConstValue, Constant, Local, Operand, OverflowOp, Place, Rvalue,
        build::Builder,
    },
    typed_ast::{self, BinaryOp, Expr, ExprKind, FieldId, LogicalOp, Pattern},
    types::{FunctionType, Type},
};
pub(super) enum BuiltinResult {
    Rvalue(Rvalue),
}
impl From<BuiltinResult> for Rvalue {
    fn from(value: BuiltinResult) -> Self {
        match value {
            BuiltinResult::Rvalue(value) => value,
        }
    }
}
impl Builder<'_> {
    fn as_constant(&mut self, expr: &Expr) -> Option<Constant> {
        match expr.kind {
            ExprKind::Bool(value) => Some(Constant::bool(value)),
            ExprKind::Int(value) => Some(Constant {
                ty: Box::new(expr.ty.clone()),
                value: ConstValue::Scalar(value as i128),
            }),
            ExprKind::Unit => Some(Constant::unit()),
            ExprKind::String(ref value) => Some(Constant {
                ty: Box::new(expr.ty.clone()),
                value: ConstValue::String(Symbol::intern(value)),
            }),
            ExprKind::Function(id, ref generic_args) => {
                let ty = expr.ty.clone();
                Some(Constant {
                    ty: Box::new(ty),
                    value: ConstValue::Named(id, generic_args.clone()),
                })
            }
            ExprKind::Lambda(ref lambda) => Some(Self::lambda_code_constant(self.ctxt, lambda)),
            ExprKind::Const(id, ref args) => {
                let ty = expr.ty.clone();
                Some(Constant {
                    ty: Box::new(ty),
                    value: ConstValue::Named(id, args.clone()),
                })
            }
            ExprKind::VariantInit(_, case, _, None) => {
                let ty = expr.ty.clone();
                Some(Constant {
                    ty: Box::new(ty),
                    value: ConstValue::Variant(case, None),
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
    pub(super) fn operand(&mut self, expr: &Expr) -> Operand {
        if let Some(operand) = self.as_operand(expr) {
            operand
        } else {
            Operand::Load(Place::local(self.expr_into_temp(expr)))
        }
    }
    pub(super) fn lower_place(&mut self, place: &typed_ast::Place) -> Place {
        match &place.kind {
            typed_ast::PlaceKind::Index(base, index) => {
                let base = self.place(base);
                let index = self.expr_into_temp(index);
                let len = self.assign_to_temp(place.loc, Type::UINT, Rvalue::Len(base.clone()));
                let in_bounds = self.assign_to_temp(
                    place.loc,
                    Type::Bool,
                    Self::binary_op_rvalue(
                        mir::BinaryOp::Lesser,
                        Operand::Load(Place::local(index)),
                        Operand::Load(Place::local(len)),
                    ),
                );
                self.finish_assert_to_new_block(
                    place.loc,
                    Operand::Load(Place::local(in_bounds)),
                    mir::AssertKind::InBounds,
                );
                base.with_index(index)
            }
            typed_ast::PlaceKind::Deref(base) => self.place(base).with_deref(),
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
        }
    }
    pub(super) fn expr_into_temp(&mut self, expr: &Expr) -> Local {
        let temp = self.new_temp(expr.ty.clone());
        self.expr_into_dest(Place::local(temp), expr);
        temp
    }
    fn assign_to_pattern(&mut self, pattern: &Pattern, value: &Expr) {
        match &pattern.kind {
            &typed_ast::PatternKind::Binding(_, var, ref ty) => {
                let place = Place::local(self.new_var(var, (**ty).clone()));
                self.expr_into_dest(place, value);
            }
            _ => {
                let local = self.expr_into_temp(value);
                self.assign_place_to_pattern(pattern, Place::local(local));
            }
        }
    }
    pub(super) fn assign_place_to_pattern(&mut self, pattern: &Pattern, place: Place) {
        match &pattern.kind {
            &typed_ast::PatternKind::Binding(_, var, ref ty) => {
                let var_place = Place::local(self.new_var(var, (**ty).clone()));
                self.assign(pattern.loc, var_place, Rvalue::Use(Operand::Load(place)));
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
                        place
                            .with_case_downcast(*index, self.ctxt.expect_ident(*id).symbol)
                            .with_field(FieldId::new(0)),
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
            ExprKind::Unsafe(expr) => {
                self.expr_into_dest(dest, expr);
            }
            ExprKind::Panic | ExprKind::NeverToAny(_) | ExprKind::Return(_) => {
                self.expr_stmt(expr);
            }
            ExprKind::Case(expr, arms) => {
                self.build_match(dest, expr, arms);
            }
            ExprKind::Logic(op, left, right) => {
                //Evaluate the left hand side
                let left_operand = self.operand(left);
                let branch_block = self.current_block;

                //Create the block for the short circuit side
                let constant_block = self.new_block();

                //Evaluate the right hand side
                let rhs_block = self.switch_to_new_block();
                self.expr_into_dest(dest.clone(), right);
                let merge_block = self.goto_to_new_block(right.loc);

                let (true_block, false_block, value, const_loc) = match op {
                    LogicalOp::And => (rhs_block, constant_block, false, left.loc),
                    LogicalOp::Or => (constant_block, rhs_block, true, right.loc),
                };

                self.switch_to_block(constant_block);
                self.assign(
                    const_loc,
                    dest,
                    Rvalue::Use(Operand::Constant(Constant::bool(value))),
                );
                self.finish_block_with_goto(const_loc, merge_block);

                self.switch_to_block(branch_block);
                self.finish_block_with_if(left.loc, left_operand, true_block, false_block);

                self.switch_to_block(merge_block);
            }
            ExprKind::Record(_)
            | ExprKind::Function(..)
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Unit
            | ExprKind::Load(_)
            | ExprKind::Call(..)
            | ExprKind::Binary(..)
            | ExprKind::Print(_)
            | ExprKind::For { .. }
            | ExprKind::Assign(..)
            | ExprKind::VariantInit(..)
            | ExprKind::String(_)
            | ExprKind::Lambda(_)
            | ExprKind::BuiltinCall(..)
            | ExprKind::Const(..)
            | ExprKind::NamedRecord(..)
            | ExprKind::While(..)
            | ExprKind::Tuple(..)
            | ExprKind::Array(..) => {
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
        ty: &Type,
        builtin: Builtin,
        args: &[Expr],
    ) -> BuiltinResult {
        let operands = args
            .iter()
            .map(|operand| self.operand(operand))
            .collect::<Vec<_>>();
        match builtin {
            Builtin::BoxAlloc => {
                let [operand] = operands.try_into().unwrap();
                let Type::Box(ty) = ty else { unreachable!() };
                BuiltinResult::Rvalue(Rvalue::AllocateBox((**ty).clone(), operand))
            }
            Builtin::Len => {
                let [operand] = operands.try_into().unwrap();
                let Operand::Load(place) = operand else {
                    unreachable!()
                };
                BuiltinResult::Rvalue(Rvalue::Len(place))
            }
            Builtin::ArrayAddr => {
                let [operand] = operands.try_into().unwrap();
                let Operand::Load(place) = operand else {
                    unreachable!()
                };
                BuiltinResult::Rvalue(Rvalue::AddrOf(place))
            }
            Builtin::WrappingAdd => {
                let [left, right] = operands.try_into().unwrap();
                BuiltinResult::Rvalue(Self::binary_op_rvalue(
                    mir::BinaryOp::Wrapping(OverflowOp::Add),
                    left,
                    right,
                ))
            }
            Builtin::OverflowingAdd => {
                let [left, right] = operands.try_into().unwrap();
                BuiltinResult::Rvalue(Self::binary_op_rvalue(
                    mir::BinaryOp::Overflow(OverflowOp::Add),
                    left,
                    right,
                ))
            }
            Builtin::Transmute => BuiltinResult::Rvalue(Rvalue::Cast(
                mir::CastKind::Transmute(ty.clone()),
                { operands }.swap_remove(0),
            )),
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
            | ExprKind::Const(..)
            | ExprKind::VariantInit(.., None)
            | ExprKind::String(..)
            | ExprKind::Lambda(_) => {
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
            ExprKind::Tuple(fields) => Rvalue::Aggregate(
                AggregateKind::Tuple,
                fields.iter().map(|field| self.operand(field)).collect(),
            ),
            &ExprKind::VariantInit(id, index, ref args, Some(ref value)) => Rvalue::Aggregate(
                AggregateKind::Variant(id, index, args.clone()),
                [self.operand(value)].into(),
            ),
            ExprKind::Call(callee, args) => match &callee.ty {
                Type::Function(function_ty) => {
                    let FunctionType { .. } = function_ty;
                    let callee_value = self.operand(callee);
                    let arg_values = args.iter().map(|arg| self.operand(arg)).collect::<Vec<_>>();
                    Rvalue::Call(callee_value, arg_values)
                }
                _ => unreachable!("Can't call non function at {:?}", expr.loc),
            },
            ExprKind::Binary(binary_op, left, right) => {
                let (left_operand, right_operand, overflow_op) = match binary_op {
                    BinaryOp::Add => (self.operand(left), self.operand(right), OverflowOp::Add),
                    BinaryOp::Divide => {
                        let left_operand = self.operand(left);
                        let right_operand = self.operand(right);
                        //Division can fail in 2 ways
                        //Divide by zero
                        //Divide int min by -1
                        let is_zero = self.assign_equals(
                            expr.loc,
                            right_operand.clone(),
                            Operand::Constant(Constant::int(0)),
                        );
                        self.finish_assert_to_new_block(
                            expr.loc,
                            Operand::Load(Place::local(is_zero)),
                            mir::AssertKind::DivideByZero,
                        );
                        let is_left_min = self.assign_equals(
                            expr.loc,
                            left_operand.clone(),
                            Operand::Constant(Constant::int(ConstValue::MIN_INT)),
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
                        self.finish_assert_to_new_block(
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
                    BinaryOp::Subtract => (
                        self.operand(left),
                        self.operand(right),
                        OverflowOp::Subtract,
                    ),
                    BinaryOp::Multiply => (
                        self.operand(left),
                        self.operand(right),
                        OverflowOp::Multiply,
                    ),
                    BinaryOp::Equals => {
                        let left_operand = self.operand(left);
                        let right_operand = self.operand(right);
                        return Self::binary_op_rvalue(
                            mir::BinaryOp::Equals,
                            left_operand,
                            right_operand,
                        );
                    }
                    BinaryOp::Lesser => {
                        let left_operand = self.operand(left);
                        let right_operand = self.operand(right);
                        return Self::binary_op_rvalue(
                            mir::BinaryOp::Lesser,
                            left_operand,
                            right_operand,
                        );
                    }
                    BinaryOp::Greater => {
                        let left_operand = self.operand(left);
                        let right_operand = self.operand(right);
                        return Self::binary_op_rvalue(
                            mir::BinaryOp::Greater,
                            left_operand,
                            right_operand,
                        );
                    }
                };
                let checked_result = self.assign_to_temp(
                    expr.loc,
                    Type::pair(Type::Bool, expr.ty.clone()),
                    Rvalue::Binary(
                        mir::BinaryOp::Overflow(overflow_op),
                        Box::new((left_operand, right_operand)),
                    ),
                );
                let overflow =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(0)));
                self.finish_assert_to_new_block(
                    expr.loc,
                    overflow,
                    mir::AssertKind::Overflow(overflow_op),
                );
                let result =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(1)));
                Rvalue::Use(result)
            }
            ExprKind::Block(..)
            | ExprKind::Panic
            | ExprKind::Case(..)
            | ExprKind::NeverToAny(_)
            | ExprKind::Logic(..)
            | ExprKind::Return(_)
            | ExprKind::Unsafe(_) => {
                let temp = self.expr_into_temp(expr);
                Rvalue::Use(Operand::Load(Place::local(temp)))
            }
            ExprKind::For { .. }
            | ExprKind::Print(_)
            | ExprKind::Assign(..)
            | ExprKind::While(..) => {
                self.expr_stmt(expr);
                Rvalue::Use(Operand::Constant(Constant::unit()))
            }
            &ExprKind::BuiltinCall(builtin, _, ref args) => {
                self.builtin_call(&expr.ty, builtin, args).into()
            }
            ExprKind::Array(fields) => {
                let ty = expr.ty.as_array().unwrap().clone();
                Rvalue::AllocateArray(ty, fields.iter().map(|field| self.operand(field)).collect())
            }
        }
    }
}
