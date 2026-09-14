use std::collections::HashMap;

use crate::{
    Symbol,
    ast::Mutable,
    builtins::{Builtin, IntegerBuiltin},
    index_vec::IndexVec,
    mir::{
        self, AggregateKind, Local, Operand, Place, Rvalue, Value,
        build::{Builder, VarKind},
    },
    src_loc::SrcLoc,
    typed_ast::{self, BinaryOp, Expr, ExprKind, FieldId, LogicalOp, Pattern, PlaceKind},
    types::Type,
};
impl<'mir, 'ctxt> Builder<'mir, 'ctxt> {
    fn as_place(&mut self, expr: &Expr<'ctxt>) -> Option<Place<'ctxt>> {
        if let ExprKind::Load(place) = &expr.kind {
            Some(self.lower_place(place))
        } else {
            None
        }
    }
    pub(super) fn place(&mut self, expr: &Expr<'ctxt>) -> Place<'ctxt> {
        if let Some(place) = self.as_place(expr) {
            place
        } else {
            Place::local(self.expr_into_temp(expr))
        }
    }
    pub(super) fn lower_place(&mut self, place: &typed_ast::Place<'ctxt>) -> Place<'ctxt> {
        match &place.kind {
            typed_ast::PlaceKind::Index(base, index) => {
                let base = self.place(base);
                let index = self.expr_into_temp(index);
                let len = self.assign_to_temp(
                    place.loc,
                    Type::new_int(self.ctxt),
                    Rvalue::Len(base.clone()),
                );
                let in_bounds = self.assign_to_temp(
                    place.loc,
                    Type::new_bool(self.ctxt),
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
                let Some(&VarKind::Local(local)) = self.resolve_var(var.1) else {
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
    pub(super) fn assign_to_pattern(
        &mut self,
        loc: SrcLoc,
        pattern: &Pattern<'ctxt>,
        value: Value<'ctxt>,
    ) {
        match pattern.kind {
            typed_ast::PatternKind::Binding(mutable, var, ty) => {
                if matches!(mutable, Mutable::Mutable) {
                    let local = self.new_var(var, ty);
                    self.declare_var(var.1, VarKind::Local(local));
                    self.push_stmt(loc, mir::StmtKind::Store(Place::local(local), value));
                } else {
                    self.declare_var(var.1, VarKind::Value(value));
                }
            }
            typed_ast::PatternKind::Bool(_)
            | typed_ast::PatternKind::Unit
            | typed_ast::PatternKind::Err
            | typed_ast::PatternKind::Int(_)
            | typed_ast::PatternKind::Char(_) => (),
            typed_ast::PatternKind::Case(.., case_id, ref pattern) => {
                if let Some(pattern) = pattern {
                    let field_tuple = self.push_operation(
                        loc,
                        mir::Operation::ExtractPayload(value.clone(), case_id),
                    );
                    let value = Value::Reg(self.push_operation(
                        loc,
                        mir::Operation::ExtractField(Value::Reg(field_tuple), FieldId::new(0)),
                    ));
                    self.assign_to_pattern(loc, pattern, value);
                }
            }
            typed_ast::PatternKind::Record(ref pattern_fields) => {
                for field in pattern_fields {
                    let value = Value::Reg(self.push_operation(
                        loc,
                        mir::Operation::ExtractField(value.clone(), field.index),
                    ));
                    self.assign_to_pattern(loc, &field.pattern, value);
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
                let value = self.expr_value(&binding.value);
                self.assign_to_pattern(binding.value.loc, &binding.pattern, value);
            }
        }
    }
    pub fn expr_into_dest(&mut self, dest: Place<'ctxt>, expr: &Expr<'ctxt>) {
        let value = self.expr_value(expr);
        self.push_stmt(expr.loc, mir::StmtKind::Store(dest, value));
    }
    fn binary_op_rvalue(
        op: mir::BinaryOp,
        left: Operand<'ctxt>,
        right: Operand<'ctxt>,
    ) -> Rvalue<'ctxt> {
        Rvalue::Binary(op, Box::new((left, right)))
    }
    fn load_place(&mut self, place: &typed_ast::Place<'ctxt>) -> Value<'ctxt> {
        match place.kind {
            PlaceKind::Var(var) => {
                let var = self.resolve_var(var.1).unwrap();
                match var {
                    &VarKind::Local(local) => Value::Reg(
                        self.push_operation(place.loc, mir::Operation::Load(Place::local(local))),
                    ),
                    VarKind::Value(value) => value.clone(),
                }
            }
            PlaceKind::Field(ref base, field) => {
                let base = self.load_place(base);
                Value::Reg(
                    self.push_operation(place.loc, mir::Operation::ExtractField(base, field)),
                )
            }
            PlaceKind::Index(ref base, ref index) => {
                let base = self.expr_value(base);
                let index = self.expr_value(index);
                Value::Reg(
                    self.push_operation(place.loc, mir::Operation::ExtractElement(base, index)),
                )
            }
            PlaceKind::Upvar(..) => todo!("handle upvars"),
            PlaceKind::Deref(..) => todo!("loading from boxes"),
            PlaceKind::Invalid => unreachable!("Cannot load from unknown place"),
        }
    }
    pub(super) fn expr_value(&mut self, expr: &Expr<'ctxt>) -> Value<'ctxt> {
        match &expr.kind {
            ExprKind::Unsafe(expr) => self.expr_value(expr),
            ExprKind::Block(block) => {
                for stmt in block.stmts.iter() {
                    self.stmt(stmt);
                }
                self.expr_value(&block.expr)
            }
            ExprKind::String(string) => Value::String(Symbol::intern(string)),
            &ExprKind::Bool(value) => Value::Bool(value),
            &ExprKind::Int(value) => Value::Int(value.try_into().expect("should be in range")),
            &ExprKind::Char(value) => Value::Char(value),
            ExprKind::Unit => Value::Unit,
            ExprKind::Panic | ExprKind::NeverToAny(_) | ExprKind::Return(_) | ExprKind::Err => {
                self.expr_stmt(expr);
                Value::Unknown(expr.ty)
            }
            ExprKind::BuiltinCall(builtin, _, exprs) => {
                fn get_values<'ctxt, const N: usize>(
                    this: &mut Builder<'_, 'ctxt>,
                    exprs: &[Expr<'ctxt>],
                ) -> [Value<'ctxt>; N] {
                    let elements = exprs
                        .as_array()
                        .expect("wrong amount of elements")
                        .each_ref();
                    elements.map(|element| this.expr_value(&element))
                }
                match *builtin {
                    Builtin::Len => {
                        let [array] = get_values(self, exprs);
                        Value::Reg(self.push_operation(expr.loc, mir::Operation::Len(array)))
                    }
                    Builtin::StringLen => todo!(),
                    Builtin::PrintString => todo!(),
                    Builtin::EprintString => todo!(),
                    Builtin::ReadLine => todo!(),
                    Builtin::IntegerBuiltin(integer_builtin) => match integer_builtin {
                        IntegerBuiltin::IntMaxValue => Value::Int(i64::MAX),
                        IntegerBuiltin::ShiftLeft => todo!(),
                        IntegerBuiltin::ShiftRight => todo!(),
                        IntegerBuiltin::WrappingAdd => {
                            let [left, right] = get_values(self, exprs);
                            Value::Reg(self.push_operation(
                                expr.loc,
                                mir::Operation::Arith(mir::ArithOp::Add, left, right),
                            ))
                        }
                        IntegerBuiltin::OverflowingAdd => {
                            let [left, right] = get_values(self, exprs);
                            Value::Reg(self.push_operation(
                                expr.loc,
                                mir::Operation::Arith(mir::ArithOp::AddOverflow, left, right),
                            ))
                        }
                        IntegerBuiltin::WrappingSub => {
                            let [left, right] = get_values(self, exprs);
                            Value::Reg(self.push_operation(
                                expr.loc,
                                mir::Operation::Arith(mir::ArithOp::Sub, left, right),
                            ))
                        }
                        IntegerBuiltin::OverflowingSub => {
                            let [left, right] = get_values(self, exprs);
                            Value::Reg(self.push_operation(
                                expr.loc,
                                mir::Operation::Arith(mir::ArithOp::SubOverflow, left, right),
                            ))
                        }
                        IntegerBuiltin::WrappingMul => {
                            let [left, right] = get_values(self, exprs);
                            Value::Reg(self.push_operation(
                                expr.loc,
                                mir::Operation::Arith(mir::ArithOp::Mul, left, right),
                            ))
                        }
                        IntegerBuiltin::OverflowingMul => {
                            let [left, right] = get_values(self, exprs);
                            Value::Reg(self.push_operation(
                                expr.loc,
                                mir::Operation::Arith(mir::ArithOp::MulOverflow, left, right),
                            ))
                        }
                    },
                }
            }
            ExprKind::VariantInit(def_id, case_id, generic_args, field) => {
                let fields = field
                    .as_ref()
                    .into_iter()
                    .map(|arg| self.expr_value(arg))
                    .collect();
                let variant = self.push_operation(
                    expr.loc,
                    mir::Operation::Aggregate(
                        mir::AggregateKind::Variant(*def_id, *case_id, generic_args.clone()),
                        fields,
                    ),
                );
                Value::Reg(variant)
            }
            ExprKind::Function(def_id, generic_args) => {
                Value::Function(*def_id, generic_args.clone())
            }
            ExprKind::Call(callee, args) => {
                let callee = self.expr_value(callee);
                let args = args.iter().map(|arg| self.expr_value(arg)).collect();
                Value::Reg(self.push_operation(expr.loc, mir::Operation::Call(callee, args)))
            }
            ExprKind::Load(place) => self.load_place(place),
            ExprKind::Binary(binary_op, left, right) => {
                let left = self.expr_value(left);
                let right = self.expr_value(right);
                let overflow_op = match binary_op {
                    BinaryOp::Lesser => {
                        return Value::Reg(self.push_operation(
                            expr.loc,
                            mir::Operation::Cmp(mir::Comparison::Lesser, left, right),
                        ));
                    }
                    BinaryOp::Greater => {
                        return Value::Reg(self.push_operation(
                            expr.loc,
                            mir::Operation::Cmp(mir::Comparison::Greater, left, right),
                        ));
                    }
                    BinaryOp::Equals => {
                        return Value::Reg(self.push_operation(
                            expr.loc,
                            mir::Operation::Cmp(mir::Comparison::Equals, left, right),
                        ));
                    }
                    BinaryOp::Add => mir::ArithOp::AddOverflow,
                    BinaryOp::Subtract => mir::ArithOp::SubOverflow,
                    BinaryOp::Multiply => mir::ArithOp::MulOverflow,
                    BinaryOp::Divide => todo!(),
                    BinaryOp::BitwiseOr => todo!(),
                    BinaryOp::BitwiseAnd => todo!(),
                };

                let tuple =
                    self.push_operation(expr.loc, mir::Operation::Arith(overflow_op, left, right));

                let overflowed = self.push_operation(
                    expr.loc,
                    mir::Operation::ExtractField(Value::Reg(tuple), FieldId::new(1)),
                );
                self.push_stmt(expr.loc, mir::StmtKind::PanicIf(Value::Reg(overflowed)));
                Value::Reg(self.push_operation(
                    expr.loc,
                    mir::Operation::ExtractField(Value::Reg(tuple), FieldId::new(0)),
                ))
            }
            ExprKind::Logic(logical_op, left, right) => {
                let left_value = self.expr_value(left);
                let start_block = self.current_block;

                let (true_block, false_block, true_block_value, false_block_value) = {
                    let constant_block = self.new_block();
                    let right_side = self.switch_to_new_block();
                    let right_value = self.expr_value(right);
                    match logical_op {
                        LogicalOp::And => {
                            (right_side, constant_block, right_value, left_value.clone())
                        }
                        LogicalOp::Or => {
                            (constant_block, right_side, left_value.clone(), right_value)
                        }
                    }
                };
                self.switch_to_block(start_block);
                self.finish_block_with_if(expr.loc, left_value, true_block, false_block);

                let (merge_block, [result]) = self.new_block_with_args([Type::new_bool(self.ctxt)]);
                self.switch_to_block(true_block);
                self.finish_block_with_goto_args(expr.loc, merge_block, [true_block_value]);

                self.switch_to_block(false_block);
                self.finish_block_with_goto_args(expr.loc, merge_block, [false_block_value]);

                self.switch_to_block(merge_block);
                Value::Reg(result)
            }
            ExprKind::Case(scrutinee, case_arms) => self.build_match(expr.ty, scrutinee, case_arms),
            ExprKind::Lambda(lambda) => Self::lambda_code_constant(self.ctxt, lambda),
            ExprKind::Tuple(fields) => {
                let fields = fields.iter().map(|field| self.expr_value(field)).collect();
                let tuple = self.push_operation(
                    expr.loc,
                    mir::Operation::Aggregate(AggregateKind::Tuple, fields),
                );
                Value::Reg(tuple)
            }
            ExprKind::Array(elements) => {
                let ty = expr.ty.as_array().expect("should be an array");
                let elements = elements
                    .iter()
                    .map(|element| self.expr_value(element))
                    .collect();
                let array = self.push_operation(expr.loc, mir::Operation::AllocArray(ty, elements));
                Value::Reg(array)
            }
            ExprKind::NamedRecord(def_id, generic_args, fields) => {
                let mut field_map = fields
                    .iter()
                    .map(|field| (field.index, self.expr_value(&field.value)))
                    .collect::<HashMap<_, _>>();
                let fields = (0..fields.len())
                    .map(FieldId::new)
                    .map(|field| field_map.remove(&field).unwrap())
                    .collect::<IndexVec<FieldId, _>>();
                let record = self.push_operation(
                    expr.loc,
                    mir::Operation::Aggregate(
                        AggregateKind::NamedRecord(*def_id, generic_args.clone()),
                        fields,
                    ),
                );
                Value::Reg(record)
            }
            ExprKind::While(..) | ExprKind::For { .. } | ExprKind::Assign(..) => {
                self.expr_stmt(expr);
                Value::Unit
            }
        }
    }
}
