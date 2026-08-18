use std::collections::HashMap;

use crate::{
    Symbol,
    builtins::{Builtin, IntegerBuiltin},
    index_vec::IndexVec,
    mir::{
        self, AggregateKind, ConstValue, Constant, Local, Operand, OverflowOp, Place, Rvalue,
        build::{Builder, TargetPlace, TargetPlaceKind},
    },
    src_loc::SrcLoc,
    typed_ast::{self, BinaryOp, Expr, ExprKind, FieldId, LogicalOp, Pattern},
    types::{IntegerKind, Type},
};
pub(super) enum BuiltinResult<'ctxt> {
    Rvalue(Rvalue<'ctxt>),
}
impl<'ctxt> From<BuiltinResult<'ctxt>> for Rvalue<'ctxt> {
    fn from(value: BuiltinResult<'ctxt>) -> Self {
        match value {
            BuiltinResult::Rvalue(value) => value,
        }
    }
}
impl<'ctxt> Builder<'_, 'ctxt> {
    fn as_constant(&mut self, expr: &Expr<'ctxt>) -> Option<Constant<'ctxt>> {
        match expr.kind {
            ExprKind::Bool(value) => Some(Constant::bool(self.ctxt, value)),
            ExprKind::Int(value) => Some(Constant {
                ty: expr.ty,
                value: ConstValue::Scalar(value as i128),
            }),
            ExprKind::Unit => Some(Constant::unit(self.ctxt)),
            ExprKind::String(ref value) => Some(Constant {
                ty: expr.ty,
                value: ConstValue::String(Symbol::intern(value)),
            }),
            ExprKind::Function(id, ref generic_args) => {
                let ty = expr.ty;
                Some(Constant {
                    ty,
                    value: ConstValue::Named(id, generic_args.clone()),
                })
            }
            ExprKind::Lambda(ref lambda) => Some(Self::lambda_code_constant(self.ctxt, lambda)),
            ExprKind::Const(id, ref args) => {
                let ty = expr.ty;
                Some(Constant {
                    ty,
                    value: ConstValue::Named(id, args.clone()),
                })
            }
            ExprKind::VariantInit(_, case, _, None) => {
                let ty = expr.ty;
                Some(Constant {
                    ty,
                    value: ConstValue::Variant(case, None),
                })
            }
            ExprKind::Char(char) => Some(Constant::char(self.ctxt, char)),
            _ => None,
        }
    }
    fn as_place(&mut self, expr: &Expr<'ctxt>, in_value_ctxt: bool) -> Option<Place> {
        if let ExprKind::Load(place) = &expr.kind {
            if matches!(place.kind, typed_ast::PlaceKind::Index(..)) && in_value_ctxt {
                return None;
            }
            Some(self.lower_place(place))
        } else {
            None
        }
    }
    pub(super) fn as_operand(&mut self, expr: &Expr<'ctxt>) -> Option<Operand<'ctxt>> {
        if let Some(constant) = self.as_constant(expr) {
            Some(Operand::Constant(constant))
        } else {
            self.as_place(expr, true).map(Operand::Load)
        }
    }
    pub(super) fn place(&mut self, expr: &Expr<'ctxt>) -> Place {
        if let Some(place) = self.as_place(expr, false) {
            place
        } else {
            Place::local(self.expr_into_temp(expr))
        }
    }
    pub(super) fn operand(&mut self, expr: &Expr<'ctxt>) -> Operand<'ctxt> {
        if let Some(operand) = self.as_operand(expr) {
            operand
        } else {
            Operand::Load(Place::local(self.expr_into_temp(expr)))
        }
    }
    pub fn assign_to_target(&mut self, target: TargetPlace<'ctxt>, value: &Expr<'ctxt>) {
        let TargetPlace { place, kind } = target;
        match kind {
            TargetPlaceKind::Base => {
                self.expr_into_dest(place, value);
            }
            TargetPlaceKind::Index(index) => {
                let loc = value.loc;
                let value = self.operand(value);
                self.push_stmt(loc, mir::StmtKind::AssignIndex(place, index, value));
            }
        }
    }
    pub fn assign_rvalue(
        &mut self,
        loc: SrcLoc,
        ty: Type<'ctxt>,
        target: TargetPlace<'ctxt>,
        rvalue: Rvalue<'ctxt>,
    ) {
        let TargetPlace { place, kind } = target;
        match kind {
            TargetPlaceKind::Base => {
                self.assign(loc, place, rvalue);
            }
            TargetPlaceKind::Index(index) => {
                let value = if let Rvalue::Use(operand) = rvalue {
                    operand
                } else {
                    Operand::Load(Place::local(self.assign_to_temp(loc, ty, rvalue)))
                };
                self.push_stmt(loc, mir::StmtKind::AssignIndex(place, index, value));
            }
        }
    }
    pub fn expr_into_target(&mut self, target: TargetPlace<'ctxt>, expr: &Expr<'ctxt>) {
        match &expr.kind {
            ExprKind::Err => unreachable!("Cannot have err here"),
            ExprKind::Block(block_body, ..) => {
                for stmt in block_body.stmts.iter() {
                    self.stmt(stmt);
                }
                self.expr_into_target(target, &block_body.expr);
            }
            ExprKind::Unsafe(expr) => {
                self.expr_into_target(target, expr);
            }
            ExprKind::Panic | ExprKind::NeverToAny(_) | ExprKind::Return(_) => {
                self.expr_stmt(expr);
            }
            ExprKind::Case(expr, arms) => {
                self.build_match(target, expr, arms);
            }
            ExprKind::Logic(op, left, right) => {
                //Evaluate the left hand side
                let left_operand = self.operand(left);
                let branch_block = self.current_block;

                //Create the block for the short circuit side
                let constant_block = self.new_block();

                //Evaluate the right hand side
                let rhs_block = self.switch_to_new_block();
                self.expr_into_target(target.clone(), right);
                let merge_block = self.goto_to_new_block(right.loc);

                let (true_block, false_block, value, const_loc) = match op {
                    LogicalOp::And => (rhs_block, constant_block, false, left.loc),
                    LogicalOp::Or => (constant_block, rhs_block, true, right.loc),
                };

                self.switch_to_block(constant_block);
                self.assign_rvalue(
                    const_loc,
                    Type::new_bool(self.ctxt),
                    target,
                    Rvalue::Use(Operand::Constant(Constant::bool(self.ctxt, value))),
                );
                self.finish_block_with_goto(const_loc, merge_block);

                self.switch_to_block(branch_block);
                self.finish_block_with_if(left.loc, left_operand, true_block, false_block);

                self.switch_to_block(merge_block);
            }
            ExprKind::Function(..)
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Unit
            | ExprKind::Load(_)
            | ExprKind::Call(..)
            | ExprKind::Binary(..)
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
            | ExprKind::Array(..)
            | ExprKind::Char(_) => {
                let rvalue = self.build_rvalue(expr);
                self.assign_rvalue(expr.loc, expr.ty, target, rvalue);
            }
        }
    }
    pub fn lower_target_place(&mut self, place: &typed_ast::Place<'ctxt>) -> TargetPlace<'ctxt> {
        match &place.kind {
            typed_ast::PlaceKind::Index(array, index) => {
                let place = self.place(array);
                let index = self.operand(index);
                TargetPlace::with_index(place, index)
            }
            _ => TargetPlace::new(self.lower_place(place)),
        }
    }
    pub fn target_place(&mut self, expr: &Expr<'ctxt>) -> TargetPlace<'ctxt> {
        match expr.kind {
            ExprKind::Load(ref place) => self.lower_target_place(place),
            _ => TargetPlace::new(Place::local(self.expr_into_temp(expr))),
        }
    }
    pub(super) fn lower_place(&mut self, place: &typed_ast::Place<'ctxt>) -> Place {
        match &place.kind {
            typed_ast::PlaceKind::Index(..) => {
                let index_rvalue = self.load_place(place);
                let local = self.assign_to_temp(place.loc, place.ty, index_rvalue);
                Place::local(local)
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
    pub(super) fn expr_into_temp(&mut self, expr: &Expr<'ctxt>) -> Local {
        let temp = self.new_temp(expr.ty);
        self.expr_into_dest(Place::local(temp), expr);
        temp
    }
    fn assign_to_pattern(&mut self, pattern: &Pattern<'ctxt>, value: &Expr<'ctxt>) {
        match pattern.kind {
            typed_ast::PatternKind::Binding(_, var, ty) => {
                let place = Place::local(self.new_var(var, ty));
                self.expr_into_dest(place, value);
            }
            _ => {
                let local = self.expr_into_temp(value);
                self.assign_place_to_pattern(pattern, Place::local(local));
            }
        }
    }
    pub(super) fn assign_place_to_pattern(&mut self, pattern: &Pattern<'ctxt>, place: Place) {
        match pattern.kind {
            typed_ast::PatternKind::Binding(_, var, ty) => {
                let var_place = Place::local(self.new_var(var, ty));
                self.assign(pattern.loc, var_place, Rvalue::Use(Operand::Load(place)));
            }
            typed_ast::PatternKind::Bool(_)
            | typed_ast::PatternKind::Int(_)
            | typed_ast::PatternKind::Unit
            | typed_ast::PatternKind::Char(_) => (),
            typed_ast::PatternKind::Record(ref fields) => {
                for field in fields {
                    self.assign_place_to_pattern(
                        &field.pattern,
                        place.clone().with_field(field.index),
                    );
                }
            }
            typed_ast::PatternKind::Err => unreachable!(),
            typed_ast::PatternKind::Case(id, _, index, ref inner) => {
                if let Some(inner) = inner {
                    self.assign_place_to_pattern(
                        inner,
                        place
                            .with_case_downcast(index, self.ctxt.expect_ident(id).symbol)
                            .with_field(FieldId::new(0)),
                    );
                }
            }
        }
    }
    pub fn stmt(&mut self, stmt: &typed_ast::Stmt<'ctxt>) {
        match &stmt.kind {
            typed_ast::StmtKind::Expr(expr) => {
                self.expr_stmt(expr);
            }
            typed_ast::StmtKind::Let(binding) => {
                self.assign_to_pattern(&binding.pattern, &binding.value);
            }
        }
    }
    pub fn expr_into_dest(&mut self, dest: Place, expr: &Expr<'ctxt>) {
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
                self.build_match(TargetPlace::new(dest), expr, arms);
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
                    Rvalue::Use(Operand::Constant(Constant::bool(self.ctxt, value))),
                );
                self.finish_block_with_goto(const_loc, merge_block);

                self.switch_to_block(branch_block);
                self.finish_block_with_if(left.loc, left_operand, true_block, false_block);

                self.switch_to_block(merge_block);
            }
            ExprKind::Function(..)
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Unit
            | ExprKind::Load(_)
            | ExprKind::Call(..)
            | ExprKind::Binary(..)
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
            | ExprKind::Array(..)
            | ExprKind::Char(_) => {
                let rvalue = self.build_rvalue(expr);
                self.assign(expr.loc, dest, rvalue);
            }
        }
    }
    fn binary_op_rvalue(
        op: mir::BinaryOp,
        left: Operand<'ctxt>,
        right: Operand<'ctxt>,
    ) -> Rvalue<'ctxt> {
        Rvalue::Binary(op, Box::new((left, right)))
    }
    pub(super) fn builtin_call(
        &mut self,
        loc: SrcLoc,
        ty: Type<'ctxt>,
        builtin: Builtin,
        args: &[Expr<'ctxt>],
    ) -> BuiltinResult<'ctxt> {
        let mut operands = || {
            args.iter()
                .map(|operand| self.operand(operand))
                .collect::<Vec<_>>()
        };
        match builtin {
            Builtin::EprintString => {
                let [arg] = operands().try_into().unwrap();
                self.push_stmt(
                    args[0].loc,
                    mir::StmtKind::Print {
                        value: arg,
                        err: true,
                    },
                );
                BuiltinResult::Rvalue(Rvalue::Use(Operand::Constant(Constant::unit(self.ctxt))))
            }
            Builtin::PtrCopy => {
                let [dst, src, count] = operands().try_into().unwrap();
                self.push_stmt(loc, mir::StmtKind::Copy { dst, src, count });
                BuiltinResult::Rvalue(Rvalue::Use(Operand::Constant(Constant::unit(self.ctxt))))
            }
            Builtin::Offset => {
                let [ptr, offset] = operands().try_into().unwrap();
                BuiltinResult::Rvalue(Self::binary_op_rvalue(mir::BinaryOp::Offset, ptr, offset))
            }
            Builtin::GcAlloc => {
                let [cap] = operands().try_into().unwrap();
                let ty = ty.as_raw_ptr().unwrap();
                BuiltinResult::Rvalue(Rvalue::GcAlloc(ty, cap))
            }
            Builtin::PtrRead => {
                let [ptr] = args else { unreachable!() };
                let ptr = self.place(ptr);
                BuiltinResult::Rvalue(Rvalue::Use(Operand::Load(ptr.with_deref())))
            }
            Builtin::PtrWrite => {
                let [ptr, value] = args else { unreachable!() };
                let ptr = self.place(ptr);
                let value = self.build_rvalue(value);
                self.assign(loc, ptr.with_deref(), value);
                BuiltinResult::Rvalue(Rvalue::Use(Operand::Constant(Constant::unit(self.ctxt))))
            }
            Builtin::Bitcast => {
                let [arg] = operands().try_into().unwrap();
                BuiltinResult::Rvalue(Rvalue::Cast(mir::CastKind::Transmute, arg, ty))
            }
            Builtin::IntegerBuiltin(integer_builtin) => match integer_builtin {
                IntegerBuiltin::IntMaxValue => {
                    let kind = ty.as_integer().unwrap();
                    let value = kind.max_value_scalar();
                    BuiltinResult::Rvalue(Rvalue::Use(Operand::Constant(Constant::integer(
                        self.ctxt, kind, value,
                    ))))
                }
                IntegerBuiltin::ShiftLeft => {
                    let [first, second] = operands().try_into().unwrap();
                    BuiltinResult::Rvalue(Self::binary_op_rvalue(
                        mir::BinaryOp::ShiftLeft,
                        first,
                        second,
                    ))
                }
                IntegerBuiltin::ShiftRight => {
                    let [first, second] = operands().try_into().unwrap();
                    BuiltinResult::Rvalue(Self::binary_op_rvalue(
                        mir::BinaryOp::ShiftRight,
                        first,
                        second,
                    ))
                }
                IntegerBuiltin::WrappingAdd => {
                    let [left, right] = operands().try_into().unwrap();
                    BuiltinResult::Rvalue(Self::binary_op_rvalue(
                        mir::BinaryOp::Wrapping(OverflowOp::Add),
                        left,
                        right,
                    ))
                }
                IntegerBuiltin::OverflowingAdd => {
                    let [left, right] = operands().try_into().unwrap();
                    BuiltinResult::Rvalue(Self::binary_op_rvalue(
                        mir::BinaryOp::Overflow(OverflowOp::Add),
                        left,
                        right,
                    ))
                }
                IntegerBuiltin::WrappingSub => {
                    let [left, right] = operands().try_into().unwrap();
                    BuiltinResult::Rvalue(Self::binary_op_rvalue(
                        mir::BinaryOp::Wrapping(OverflowOp::Subtract),
                        left,
                        right,
                    ))
                }
                IntegerBuiltin::OverflowingSub => {
                    let [left, right] = operands().try_into().unwrap();
                    BuiltinResult::Rvalue(Self::binary_op_rvalue(
                        mir::BinaryOp::Overflow(OverflowOp::Subtract),
                        left,
                        right,
                    ))
                }
                IntegerBuiltin::WrappingMul => {
                    let [left, right] = operands().try_into().unwrap();
                    BuiltinResult::Rvalue(Self::binary_op_rvalue(
                        mir::BinaryOp::Wrapping(OverflowOp::Multiply),
                        left,
                        right,
                    ))
                }
                IntegerBuiltin::OverflowingMul => {
                    let [left, right] = operands().try_into().unwrap();
                    BuiltinResult::Rvalue(Self::binary_op_rvalue(
                        mir::BinaryOp::Overflow(OverflowOp::Multiply),
                        left,
                        right,
                    ))
                }
                IntegerBuiltin::Truncate => {
                    let [arg] = operands().try_into().unwrap();
                    let kind = ty.as_integer().unwrap();
                    BuiltinResult::Rvalue(Rvalue::Cast(
                        mir::CastKind::IntegerCast(mir::IntegerCast::Truncate(kind)),
                        arg,
                        ty,
                    ))
                }
                IntegerBuiltin::Widen => {
                    let [operand] = operands().try_into().unwrap();
                    let kind = ty.as_simple_scalar().unwrap();
                    let (signed, size) = kind.as_integer().signed_and_size();
                    BuiltinResult::Rvalue(Rvalue::Cast(
                        mir::CastKind::IntegerCast(if signed {
                            mir::IntegerCast::SignExtend(size)
                        } else {
                            mir::IntegerCast::ZeroExtend(size)
                        }),
                        operand,
                        ty,
                    ))
                }
            },
            Builtin::ReadLine => BuiltinResult::Rvalue(Rvalue::ReadLine),
            Builtin::UninitNew => {
                let [operand] = operands().try_into().unwrap();
                BuiltinResult::Rvalue(Rvalue::Cast(mir::CastKind::Transmute, operand, ty))
            }
            Builtin::UninitAssumeInit => {
                let [operand] = operands().try_into().unwrap();
                BuiltinResult::Rvalue(Rvalue::Cast(mir::CastKind::Transmute, operand, ty))
            }
            Builtin::UninitZeroed => {
                let ty = ty.as_uninit().unwrap();
                BuiltinResult::Rvalue(Rvalue::UninitZeroed(ty))
            }
            Builtin::PrintString => {
                let [arg] = operands().try_into().unwrap();
                self.push_stmt(
                    args[0].loc,
                    mir::StmtKind::Print {
                        value: arg,
                        err: false,
                    },
                );
                BuiltinResult::Rvalue(Rvalue::Use(Operand::Constant(Constant::unit(self.ctxt))))
            }
            Builtin::ArrayGetUnchecked => {
                let [array, index] = args else { unreachable!() };
                let _ = self.place(array);
                let index = self.operand(index);
                let _ = self.assign_to_temp(args[1].loc, args[1].ty, Rvalue::Use(index));
                todo!("remove me")
            }
            Builtin::ArraySetUnchecked => {
                let [array, index, value] = args else {
                    unreachable!()
                };
                let _ = self.place(array);
                let index = self.operand(index);
                let _ = self.assign_to_temp(args[1].loc, args[1].ty, Rvalue::Use(index));
                let _ = self.operand(value);
                todo!("remove me")
            }
            Builtin::RawArrayAlloc => {
                let [count] = operands().try_into().unwrap();
                let ty = ty.as_raw_array().unwrap();
                BuiltinResult::Rvalue(Rvalue::AllocateRawArray { ty, count })
            }
            Builtin::BoxAlloc => {
                let [operand] = operands().try_into().unwrap();
                let Some(ty) = ty.as_box() else {
                    unreachable!()
                };
                BuiltinResult::Rvalue(Rvalue::AllocateBox(ty, operand))
            }
            Builtin::Len => {
                let place = self.place(&args[0]);
                BuiltinResult::Rvalue(Rvalue::Len(place))
            }
            Builtin::ArrayAddr => {
                let place = self.place(&args[0]);
                BuiltinResult::Rvalue(Rvalue::AddrOf(place))
            }
            Builtin::Transmute => BuiltinResult::Rvalue(Rvalue::Cast(
                mir::CastKind::Transmute,
                { operands() }.swap_remove(0),
                ty,
            )),
        }
    }
    pub fn load_place(&mut self, place: &typed_ast::Place<'ctxt>) -> Rvalue<'ctxt> {
        match place.kind {
            typed_ast::PlaceKind::Index(ref array, ref index) => {
                let place = self.place(array);
                let index = self.operand(index);
                Rvalue::LoadIndex(place, index)
            }
            _ => {
                let operand = self.lower_place(place);
                Rvalue::Use(Operand::Load(operand))
            }
        }
    }
    pub fn build_rvalue(&mut self, expr: &Expr<'ctxt>) -> Rvalue<'ctxt> {
        match &expr.kind {
            ExprKind::Err => unreachable!("Cannot have err here"),
            ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::Bool(_)
            | ExprKind::Function(..)
            | ExprKind::Const(..)
            | ExprKind::VariantInit(.., None)
            | ExprKind::String(..)
            | ExprKind::Lambda(_)
            | ExprKind::Char(_) => {
                let operand = self
                    .as_operand(expr)
                    .unwrap_or_else(|| unreachable!("should be an constant operand '{:?}' ", expr));
                Rvalue::Use(operand)
            }
            ExprKind::Load(place) => self.load_place(place),
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
            ExprKind::Tuple(fields) => Rvalue::Aggregate(
                AggregateKind::Tuple,
                fields.iter().map(|field| self.operand(field)).collect(),
            ),
            &ExprKind::VariantInit(id, index, ref args, Some(ref value)) => Rvalue::Aggregate(
                AggregateKind::Variant(id, index, args.clone()),
                [self.operand(value)].into(),
            ),
            ExprKind::Call(callee, args) => {
                let _ = callee
                    .ty
                    .as_function()
                    .unwrap_or_else(|| unreachable!("Can't call non function at {:?}", expr.loc));

                let callee_value = self.operand(callee);
                let arg_values = args.iter().map(|arg| self.operand(arg)).collect::<Vec<_>>();
                Rvalue::Call(callee_value, arg_values)
            }
            ExprKind::Binary(binary_op, left, right) => {
                let (left_operand, right_operand, overflow_op) = match binary_op {
                    BinaryOp::Add => (self.operand(left), self.operand(right), OverflowOp::Add),
                    BinaryOp::Divide => {
                        let left_operand = self.operand(left);
                        let right_operand = self.operand(right);
                        let kind = left.ty.as_integer().unwrap();
                        //Division can fail in 2 ways
                        //Divide by zero
                        //Divide int min by -1
                        let is_zero = self.assign_equals(
                            expr.loc,
                            right_operand.clone(),
                            Operand::Constant(Constant::integer(self.ctxt, kind, 0)),
                        );
                        self.finish_assert_to_new_block(
                            expr.loc,
                            Operand::Load(Place::local(is_zero)),
                            mir::AssertKind::DivideByZero,
                        );

                        if let IntegerKind::Signed(size) = kind {
                            let is_left_min = self.assign_equals(
                                expr.loc,
                                left_operand.clone(),
                                Operand::Constant(Constant::integer(
                                    self.ctxt,
                                    kind,
                                    kind.min_value_scalar(),
                                )),
                            );
                            let is_right_neg_1 = self.assign_equals(
                                expr.loc,
                                left_operand.clone(),
                                Operand::Constant(Constant::int(self.ctxt, size, -1)),
                            );
                            let overflow = self.assign_binary_result(
                                expr.loc,
                                Type::new_bool(self.ctxt),
                                mir::BinaryOp::BitwiseAnd,
                                Operand::Load(Place::local(is_left_min)),
                                Operand::Load(Place::local(is_right_neg_1)),
                            );
                            self.finish_assert_to_new_block(
                                expr.loc,
                                Operand::Load(Place::local(overflow)),
                                mir::AssertKind::DivideOverflow,
                            );
                        }
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
                    BinaryOp::BitwiseOr => {
                        let left_operand = self.operand(left);
                        let right_operand = self.operand(right);
                        return Self::binary_op_rvalue(
                            mir::BinaryOp::BitwiseOr,
                            left_operand,
                            right_operand,
                        );
                    }
                    BinaryOp::BitwiseAnd => {
                        let left_operand = self.operand(left);
                        let right_operand = self.operand(right);
                        return Self::binary_op_rvalue(
                            mir::BinaryOp::BitwiseAnd,
                            left_operand,
                            right_operand,
                        );
                    }
                };
                let checked_result = self.assign_to_temp(
                    expr.loc,
                    Type::pair(self.ctxt, expr.ty, Type::new_bool(self.ctxt)),
                    Rvalue::Binary(
                        mir::BinaryOp::Overflow(overflow_op),
                        Box::new((left_operand, right_operand)),
                    ),
                );
                let overflow =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(1)));
                self.finish_assert_to_new_block(
                    expr.loc,
                    overflow,
                    mir::AssertKind::Overflow(overflow_op),
                );
                let result =
                    Operand::Load(Place::local(checked_result).with_field(FieldId::new(0)));
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
            ExprKind::For { .. } | ExprKind::Assign(..) | ExprKind::While(..) => {
                self.expr_stmt(expr);
                Rvalue::Use(Operand::Constant(Constant::unit(self.ctxt)))
            }
            &ExprKind::BuiltinCall(_, builtin, _, ref args) => {
                self.builtin_call(expr.loc, expr.ty, builtin, args).into()
            }
            ExprKind::Array(fields) => {
                let ty = expr.ty.as_array().unwrap();
                Rvalue::AllocateArray(ty, fields.iter().map(|field| self.operand(field)).collect())
            }
        }
    }
}
