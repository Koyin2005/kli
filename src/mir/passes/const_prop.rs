use crate::{
    CtxtRef, def_ids::DefId, index_vec::IndexVec, mir::{
        self, BasicBlockId, Body, Local, Place, PlaceBase, PlaceProjection, Reg, Stmt, StmtKind, passes::{
            BodyPass,
            dataflow::{self, Analysis, Domain},
            optimisation_enabled,
        }, visitor::MutVisit,
    }, typed_ast::FieldId, types::{CaseId, GenericArgs},
};

type Constant<'ctxt> = ();
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
    _ctxt: CtxtRef<'ctxt>,
}

impl<'ctxt> Analysis<'ctxt> for ConstAnalysis<'ctxt> {
    type Domain = Values<'ctxt>;
    fn apply_stmt_effect(&mut self, state: &mut Self::Domain, stmt: &Stmt<'ctxt>) {
        let StmtKind::Assign(reg, operation) = &stmt.kind else {
            return;
        };
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
        let mut states = ConstAnalysis { _ctxt: ctxt }.iterate_to_fixpoint(body);
        for (block_id, block) in body
            .block_info
            .blocks_mut_dont_dirty()
            .iter_mut_enumerated()
        {
            let state = &mut states[block_id];

            for (id, stmt) in &mut block.stmts.iter_mut_enumerated() {
                apply_stmt_effect(ctxt, state, stmt);
                OperandUpdater {
                    values: state,
                    ctxt,
                }
                .visit_stmt(mir::Location::stmt(block_id, id), stmt);
            }
            OperandUpdater {
                values: state,
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
    _ = ctxt;
    let StmtKind::Assign(reg, rvalue) = &stmt.kind else {
        return;
    };
}

fn eval_rvalue<'ctxt>(_ctxt: CtxtRef<'ctxt>, _values: &Values<'ctxt>) -> Option<LocalValue<'ctxt>> {
    todo!()
}

fn load_value<'ctxt>(values: &Values<'ctxt>, reg: Reg) -> Option<LocalValue<'ctxt>> {
    None   
}
type Values<'ctxt> = IndexVec<Reg, Option<LocalValue<'ctxt>>>;

struct OperandUpdater<'a, 'ctxt> {
    values: &'a Values<'ctxt>,
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> MutVisit<'ctxt> for OperandUpdater<'_, 'ctxt> {}
