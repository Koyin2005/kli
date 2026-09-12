use std::collections::{HashSet, VecDeque};

use crate::{
    CtxtRef,
    index_vec::IndexVec,
    mir::{
        self, BasicBlockId, BinaryOp, ConstValue, Constant, Local, Operand, Place, PlaceBase,
        PlaceProjection, Rvalue, Stmt, StmtKind, TerminatorKind,
        passes::{BodyPass, optimisation_enabled},
        visitor::MutVisit,
    },
    typed_ast::FieldId,
};
#[derive(Clone, Debug, PartialEq, Eq)]
enum LocalValue<'ctxt> {
    Tuple(IndexVec<FieldId, Constant<'ctxt>>),
    Simple(Constant<'ctxt>),
}

fn join_values<'ctxt>(dst: &mut Values<'ctxt>, src: &Values<'ctxt>) -> bool {
    let mut changed = false;
    for (dst, src) in dst.iter_mut().zip(src) {
        changed |= match (&mut *dst, src) {
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => {
                dst.clone_from(src);
                true
            }
            (Some(dst_value), Some(src)) => {
                if dst_value != src {
                    dst.clone_from(&None);
                    true
                } else {
                    false
                }
            }
        };
    }
    changed
}
pub struct ConstProp;
impl<'ctxt> BodyPass<'ctxt> for ConstProp {
    fn name(&self) -> &'static str {
        "const-prop"
    }
    fn enabled(&self, ctxt: crate::CtxtRef<'ctxt>) -> bool {
        optimisation_enabled(ctxt)
    }
    fn run(&self, ctxt: crate::CtxtRef<'ctxt>, body: &'_ mut crate::mir::Body<'ctxt>) {
        let mut state = IndexVec::new();
        let mut states =
            IndexVec::<BasicBlockId, _>::from_function(body.block_info.blocks().len(), |_| {
                Values::from_value(body.locals.len(), None)
            });
        let mut in_queue = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_front(BasicBlockId::ENTRY);
        in_queue.insert(BasicBlockId::ENTRY);
        while let Some(block) = queue.pop_back() {
            state.clone_from(&states[block]);
            for stmt in body.block_info.blocks()[block].stmts.iter() {
                apply_stmt_effect(ctxt, &mut state, stmt);
            }
            let terminator = body.block_info.blocks()[block].expect_terminator();

            if let TerminatorKind::Switch(condition, targets) = &terminator.kind
                && let Some(LocalValue::Simple(constant)) = eval_operand(&state, condition)
                && let ConstValue::Scalar(value) = constant.value
            {
                let succ = targets.branch_for_value(value);
                let new_state = &mut states[succ];
                if join_values(new_state, &state) {
                    queue.push_front(succ);
                }
            } else {
                for succ in terminator.successors() {
                    let new_state = &mut states[succ];
                    if join_values(new_state, &state) {
                        queue.push_front(succ);
                    }
                }
            }
        }

        for (block_id, block) in body
            .block_info
            .blocks_mut_dont_dirty()
            .iter_mut_enumerated()
        {
            let state = &mut states[block_id];

            for (id, stmt) in &mut block.stmts.iter_mut_enumerated() {
                apply_stmt_effect(ctxt, state, stmt);
                OperandUpdater { values: &state }
                    .visit_stmt(mir::Location::stmt(block_id, id), stmt);
            }
            OperandUpdater { values: &state }.visit_terminator(
                mir::Location::terminator(block_id),
                block.expect_terminator_mut(),
            );
        }
    }
}
fn apply_stmt_effect<'ctxt>(ctxt: CtxtRef<'ctxt>, values: &mut Values<'ctxt>, stmt: &Stmt<'ctxt>) {
    let StmtKind::Assign(place, rvalue) = &stmt.kind else {
        return;
    };
    let PlaceBase::Local(local) = place.base;
    if !place.projections.is_empty() {
        values[local] = None;
        return;
    }
    values[local] = eval_rvalue(ctxt, &values, rvalue);
}

fn eval_operand<'ctxt>(
    values: &Values<'ctxt>,
    operand: &Operand<'ctxt>,
) -> Option<LocalValue<'ctxt>> {
    match operand {
        Operand::Constant(constant) => Some(LocalValue::Simple(constant.clone())),
        Operand::Load(place) => load_value(values, place),
    }
}
fn eval_rvalue<'ctxt>(
    ctxt: CtxtRef<'ctxt>,
    values: &Values<'ctxt>,
    rvalue: &Rvalue<'ctxt>,
) -> Option<LocalValue<'ctxt>> {
    match rvalue {
        Rvalue::Use(operand) => eval_operand(values, operand),
        Rvalue::Aggregate(kind, fields) => match kind {
            mir::AggregateKind::Tuple => Some(LocalValue::Tuple(
                fields
                    .iter()
                    .map(|field| {
                        let LocalValue::Simple(constant) = eval_operand(values, field)? else {
                            return None;
                        };
                        Some(constant)
                    })
                    .collect::<Option<IndexVec<FieldId, _>>>()?,
            )),
            mir::AggregateKind::NamedRecord(..) => None,
            mir::AggregateKind::Variant(..) => None,
        },
        Rvalue::Binary(op, operands) => {
            let (left, right) = &**operands;
            let left = eval_operand(values, left)?;
            let right = eval_operand(values, right)?;
            match op {
                BinaryOp::Equals => Some(LocalValue::Simple(Constant::bool(ctxt, left == right))),
                _ => None,
            }
        }
        _ => None,
    }
}

fn load_value<'ctxt>(values: &Values<'ctxt>, place: &Place) -> Option<LocalValue<'ctxt>> {
    let PlaceBase::Local(local) = place.base;
    let mut value = values[local].clone()?;
    for projection in place.projections.iter() {
        value = match projection {
            PlaceProjection::Field(field) => {
                let LocalValue::Tuple(fields) = value else {
                    return None;
                };
                LocalValue::Simple(fields[*field].clone())
            }
            _ => return None,
        }
    }

    Some(value)
}

fn as_rvalue<'ctxt>(value: LocalValue<'ctxt>) -> Rvalue<'ctxt> {
    match value {
        LocalValue::Tuple(fields) => Rvalue::Aggregate(
            mir::AggregateKind::Tuple,
            fields.into_iter().map(Operand::Constant).collect(),
        ),
        LocalValue::Simple(constant) => Rvalue::Use(Operand::Constant(constant)),
    }
}

type Values<'ctxt> = IndexVec<Local, Option<LocalValue<'ctxt>>>;

struct OperandUpdater<'a, 'ctxt> {
    values: &'a Values<'ctxt>,
}
impl<'ctxt> MutVisit<'ctxt> for OperandUpdater<'_, 'ctxt> {
    fn visit_operand(&mut self, _: crate::mir::Location, operand: &mut Operand<'ctxt>) {
        if let Operand::Load(place) = operand
            && let Some(LocalValue::Simple(value)) = load_value(self.values, place)
        {
            *operand = Operand::Constant(value);
        }
    }
    fn visit_rvalue(&mut self, loc: mir::Location, rvalue: &mut Rvalue<'ctxt>) {
        self.super_visit_rvalue(loc, rvalue);
        if let Rvalue::Use(value) = rvalue
            && let Operand::Load(place) = value
            && let Some(value) = load_value(self.values, place)
        {
            *rvalue = as_rvalue(value);
        }
    }
}
