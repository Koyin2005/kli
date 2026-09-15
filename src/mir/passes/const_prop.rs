use crate::{
    CtxtRef,
    def_ids::DefId,
    index_vec::IndexVec,
    mir::{
        self, BasicBlockId, BinaryOp, Body, Constant, Local, Operand, OverflowOp,
        Place, PlaceBase, PlaceProjection, Rvalue, Stmt, StmtKind,
        passes::{
            BodyPass,
            dataflow::{self, Analysis, Domain},
            optimisation_enabled,
        },
        visitor::MutVisit,
    },
    typed_ast::FieldId,
    types::{CaseId, GenericArgs},
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum LocalValue<'ctxt> {
    Variant(DefId, CaseId, GenericArgs<'ctxt>, Option<Constant<'ctxt>>),
    Tuple(IndexVec<FieldId, Constant<'ctxt>>),
    Simple(Constant<'ctxt>),
}

impl<'ctxt> Domain for Values<'ctxt> {
    fn initial<'b>(body: &Body<'b>) -> Self {
        Values::from_value(body.locals.len(), None)
    }
    fn join(&mut self, other: &Self) -> bool {
        let mut changed = false;
        for (dst, src) in self.iter_mut().zip(other) {
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
}
struct ConstAnalysis<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
}

impl<'ctxt> Analysis<'ctxt> for ConstAnalysis<'ctxt> {
    type Domain = Values<'ctxt>;
    fn apply_stmt_effect(&mut self, state: &mut Self::Domain, stmt: &Stmt<'ctxt>) {
        let StmtKind::OldStore(place, rvalue) = &stmt.kind else {
            return;
        };
        let PlaceBase::Local(local) = place.base else {
            unreachable!();
        };
        if !place.projections.is_empty() {
            state[local] = None;
            return;
        }
        state[local] = eval_rvalue(self.ctxt, &state, rvalue);
    }

    fn propagate_to_basic_blocks(
        &self,
        state: &Self::Domain,
        terminator: &mir::Terminator<'ctxt>,
        f: impl FnMut(BasicBlockId),
    ) {
            dataflow::prop_uniform(self, state, terminator, f);
    
    }
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
        let mut states = ConstAnalysis { ctxt }.iterate_to_fixpoint(body);
        for (block_id, block) in body
            .block_info
            .blocks_mut_dont_dirty()
            .iter_mut_enumerated()
        {
            let state = &mut states[block_id];

            for (id, stmt) in &mut block.stmts.iter_mut_enumerated() {
                apply_stmt_effect(ctxt, state, stmt);
                OperandUpdater {
                    values: &state,
                    ctxt,
                }
                .visit_stmt(mir::Location::stmt(block_id, id), stmt);
            }
            OperandUpdater {
                values: &state,
                ctxt,
            }
            .visit_terminator(
                mir::Location::terminator(block_id),
                block.expect_terminator_mut(),
            );
        }
    }
}
fn apply_stmt_effect<'ctxt>(ctxt: CtxtRef<'ctxt>, values: &mut Values<'ctxt>, stmt: &Stmt<'ctxt>) {
    let StmtKind::OldStore(place, rvalue) = &stmt.kind else {
        return;
    };
    let PlaceBase::Local(local) = place.base else {
        unreachable!()
    };
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
            mir::AggregateKind::Variant(id, case, args) => Some(LocalValue::Variant(
                *id,
                *case,
                args.clone(),
                if let Some(field) = fields.iter().next() {
                    let LocalValue::Simple(constant) = eval_operand(values, field)? else {
                        return None;
                    };
                    Some(constant)
                } else {
                    None
                },
            )),
        },
        Rvalue::Binary(op, operands) => {
            let (left, right) = &**operands;
            let left = eval_operand(values, left)?;
            let right = eval_operand(values, right)?;
            match op {
                BinaryOp::Equals => Some(LocalValue::Simple(Constant::bool(ctxt, left == right))),
                BinaryOp::Overflow(op) => {
                    let LocalValue::Simple(left) = left else {
                        return None;
                    };
                    let LocalValue::Simple(right) = right else {
                        return None;
                    };
                    let left = left.value.as_scalar()? as i64;
                    let right = right.value.as_scalar()? as i64;
                    let (left, right) = match op {
                        OverflowOp::Add => left.overflowing_add(right),
                        OverflowOp::Multiply => left.overflowing_mul(right),
                        OverflowOp::Subtract => left.overflowing_sub(right),
                    };
                    let left_value = Constant::int(ctxt, left);
                    let right_value = Constant::bool(ctxt, right);
                    Some(LocalValue::Tuple(IndexVec::from([left_value, right_value])))
                }
                _ => None,
            }
        }
        Rvalue::Discriminant(place) => {
            let LocalValue::Variant(def_id, case, _, _) = load_value(values, place)? else {
                unreachable!("Should be a variant")
            };
            let value = ctxt.type_def(def_id).case_value(case).1;
            Some(LocalValue::Simple(Constant::int(ctxt, value.into())))
        }
        _ => None,
    }
}

fn load_value<'ctxt>(values: &Values<'ctxt>, place: &Place<'ctxt>) -> Option<LocalValue<'ctxt>> {
    let PlaceBase::Local(local) = place.base else {
        unreachable!()
    };
    let mut value = values[local].clone()?;
    for projection in place.projections.iter() {
        value = match projection {
            PlaceProjection::Field(field) => {
                let LocalValue::Tuple(fields) = value else {
                    return None;
                };
                LocalValue::Simple(fields[*field].clone())
            }
            PlaceProjection::CaseDowncast(case, _) => {
                let LocalValue::Variant(_, current_case, _, value) = value else {
                    return None;
                };
                if *case != current_case {
                    return None;
                }
                LocalValue::Tuple(value.into_iter().collect())
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
        LocalValue::Variant(id, case, args, value) => Rvalue::Aggregate(
            mir::AggregateKind::Variant(id, case, args),
            value.into_iter().map(Operand::Constant).collect(),
        ),
    }
}

type Values<'ctxt> = IndexVec<Local, Option<LocalValue<'ctxt>>>;

struct OperandUpdater<'a, 'ctxt> {
    values: &'a Values<'ctxt>,
    ctxt: CtxtRef<'ctxt>,
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
        if let Some(value) = eval_rvalue(self.ctxt, self.values, rvalue) {
            *rvalue = as_rvalue(value);
        }
    }
}
