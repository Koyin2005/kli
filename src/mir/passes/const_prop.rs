use std::collections::HashMap;

use crate::{
    CtxtRef,
    mir::{
        AggregateKind, Constant, Locals, Operand, Place, Rvalue, StmtKind, TerminatorKind,
        passes::MirPass, visitor::MutVisit,
    },
    typed_ast::FieldId,
    types::Type,
};

pub struct ConstProp;
impl MirPass for ConstProp {
    fn name(&self) -> &'static str {
        "const-prop"
    }
    fn run(&self, ctxt: crate::CtxtRef<'_>, body: &mut crate::mir::Body) {
        let mut constifier = Constifier {
            values: Default::default(),
            ctxt,
            local_info: &body.locals,
            return_type: &body.return_type,
        };
        for (id, block) in body.blocks.iter_mut_enumerated() {
            constifier.visit_block(id, block);
        }
    }
}
struct Constifier<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
    values: HashMap<Place, Constant>,
    local_info: &'ctxt Locals,
    return_type: &'ctxt Type,
}
impl Constifier<'_> {
    fn eval_rvalue(&mut self, place: &Place, rvalue: &mut Rvalue) -> Option<Constant> {
        match rvalue {
            Rvalue::Use(operand) => self.eval_operand(operand),
            Rvalue::Len(place) => {
                let Type::Array(_, count) =
                    place.type_of(self.ctxt, self.local_info, self.return_type)
                else {
                    return None;
                };
                Some(Constant::int(count.try_into().ok()?))
            }
            Rvalue::Binary(op, operands) => {
                let (left, right) = &mut **operands;
                let left = self.eval_operand(left)?;
                let right = self.eval_operand(right)?;
                let left = left
                    .value
                    .as_scalar()
                    .and_then(|value| value.try_into().ok());
                let right = right
                    .value
                    .as_scalar()
                    .and_then(|value| value.try_into().ok());
                let left: i64 = left?;
                let right: i64 = right?;
                Some(match op {
                    crate::mir::BinaryOp::Overflow(overflow_op) => {
                        let (result, overflow) = match overflow_op {
                            crate::mir::OverflowOp::Add => left.overflowing_add(right),
                            crate::mir::OverflowOp::Subtract => left.overflowing_sub(right),
                            crate::mir::OverflowOp::Multiply => left.overflowing_mul(right),
                        };
                        self.values.insert(
                            place.clone().with_field(FieldId::new(0)),
                            Constant::bool(overflow),
                        );
                        self.values.insert(
                            place.clone().with_field(FieldId::new(1)),
                            Constant::int(result),
                        );
                        return None;
                    }
                    crate::mir::BinaryOp::Unchecked(overflow_op) => {
                        Constant::int(match overflow_op {
                            crate::mir::OverflowOp::Add => left.checked_add(right)?,
                            crate::mir::OverflowOp::Subtract => left.checked_sub(right)?,
                            crate::mir::OverflowOp::Multiply => left.checked_mul(right)?,
                        })
                    }
                    crate::mir::BinaryOp::Wrapping(overflow_op) => {
                        Constant::int(match overflow_op {
                            crate::mir::OverflowOp::Add => left.wrapping_add(right),
                            crate::mir::OverflowOp::Subtract => left.wrapping_sub(right),
                            crate::mir::OverflowOp::Multiply => left.wrapping_mul(right),
                        })
                    }
                    crate::mir::BinaryOp::Greater => Constant::bool(left > right),
                    crate::mir::BinaryOp::Offset => return None,
                    crate::mir::BinaryOp::Divide => Constant::int(left.checked_div(right)?),
                    crate::mir::BinaryOp::Equals => Constant::bool(left == right),
                    crate::mir::BinaryOp::BitwiseAnd => return None,
                    crate::mir::BinaryOp::Lesser => Constant::bool(left < right),
                })
            }
            Rvalue::Aggregate(kind, fields) => {
                let mut place = place.clone();
                if let AggregateKind::Variant(id, case, _) = kind {
                    let name = self.ctxt.type_def(*id).case(*case).name;
                    place = place.with_case_downcast(*case, name)
                }
                for (id, field) in fields.iter_mut_enumerated() {
                    self.make_constant(field);
                    let Some(value) = self.eval_operand(field) else {
                        continue;
                    };
                    self.values.insert(place.clone().with_field(id), value);
                }
                None
            }
            _ => None,
        }
    }
    fn eval_operand(&self, operand: &Operand) -> Option<Constant> {
        match operand {
            Operand::Constant(constant) => Some(constant.clone()),
            Operand::Load(place) => self.constant_for_place(place),
        }
    }
    fn constant_for_place(&self, place: &Place) -> Option<Constant> {
        self.values.get(place).cloned()
    }
    fn make_constant(&mut self, operand: &mut Operand) {
        if let Operand::Load(place) = operand
            && let Some(value) = self.constant_for_place(place)
        {
            *operand = Operand::Constant(value);
        }
    }
    fn store_constant_for_place(&mut self, place: &Place, value: Constant) {
        self.values.insert(place.clone(), value);
    }

    fn as_scalar_constant(operand: &Operand) -> Option<i128> {
        if let Operand::Constant(constant) = operand {
            constant.value.as_scalar()
        } else {
            None
        }
    }
}
impl MutVisit for Constifier<'_> {
    fn visit_operand(&mut self, _: crate::mir::Location, operand: &mut Operand) {
        self.make_constant(operand);
    }
    fn visit_stmt(&mut self, loc: crate::mir::Location, stmt: &mut crate::mir::Stmt) {
        match &mut stmt.kind {
            StmtKind::Assign(place, value) => {
                let constant = self.eval_rvalue(place, value);
                if let Some(constant) = constant {
                    **value = Rvalue::Use(Operand::Constant(constant.clone()));
                    self.store_constant_for_place(place, constant);
                }
            }
            _ => (),
        }
        self.super_visit_stmt(loc, stmt);
    }
    fn visit_terminator(
        &mut self,
        _: crate::mir::Location,
        terminator: &mut crate::mir::Terminator,
    ) {
        match &mut terminator.kind {
            TerminatorKind::Goto(_)
            | TerminatorKind::Panic
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => return,
            TerminatorKind::Switch(operand, targets) => {
                self.make_constant(operand);
                let Some(target) = Self::as_scalar_constant(operand).map(|value| {
                    targets
                        .targets
                        .iter()
                        .find(|target| target.value == value)
                        .map_or(targets.otherwise, |target| target.target)
                }) else {
                    return;
                };
                terminator.kind = TerminatorKind::Goto(target);
            }
            
            TerminatorKind::Assert(operand, _,block) => {
                self.make_constant(operand);
                if let Some(constant) = self.eval_operand(operand)
                    && let Some(0) = constant.value.as_scalar()
                {
                    terminator.kind = TerminatorKind::Goto(*block);
                }
            }
        }
    }
    fn visit_block(&mut self, id: crate::mir::BasicBlockId, block: &mut crate::mir::BasicBlock) {
        self.values.clear();
        self.super_visit_block(id, block);
    }
}
