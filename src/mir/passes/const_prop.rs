use crate::{
    index_vec::IndexVec,
    mir::{
        self, Constant, Local, Operand, PlaceBase, Rvalue, StmtKind, passes::BodyPass,
        visitor::MutVisit,
    },
};

pub struct ConstProp;
impl<'ctxt> BodyPass<'ctxt> for ConstProp {
    fn name(&self) -> &'static str {
        "const-prop"
    }
    fn run(&self, _: crate::CtxtRef<'ctxt>, body: &'_ mut crate::mir::Body<'ctxt>) {
        let mut locals = body.locals.iter().map(|_| None).collect::<Values>();
        for (block_id, block) in body
            .block_info
            .blocks_mut_dont_dirty()
            .iter_mut_enumerated()
        {
            locals.iter_mut().for_each(|local| *local = None);
            for (stmt_id, stmt) in block.stmts.iter_mut_enumerated() {
                let StmtKind::Assign(place, value) = &mut stmt.kind else {
                    OperandUpdater { values: &locals }
                        .visit_stmt(mir::Location::stmt(block_id, stmt_id), stmt);
                    continue;
                };
                if !place.projections.is_empty() {
                    OperandUpdater { values: &locals }
                        .visit_stmt(mir::Location::stmt(block_id, stmt_id), stmt);
                    continue;
                }
                {
                    let Rvalue::Use(Operand::Constant(constant)) = &mut **value else {
                        OperandUpdater { values: &locals }
                            .visit_stmt(mir::Location::stmt(block_id, stmt_id), stmt);
                        continue;
                    };
                    let PlaceBase::Local(local) = place.base;
                    locals[local] = Some(constant.clone());
                }
                OperandUpdater { values: &locals }
                    .visit_stmt(mir::Location::stmt(block_id, stmt_id), stmt);
            }

            OperandUpdater { values: &locals }.visit_terminator(
                mir::Location::terminator(block_id),
                block.expect_terminator_mut(),
            );
        }
    }
}

type Values<'ctxt> = IndexVec<Local, Option<Constant<'ctxt>>>;

struct OperandUpdater<'a, 'ctxt> {
    values: &'a Values<'ctxt>,
}
impl<'ctxt> MutVisit<'ctxt> for OperandUpdater<'_, 'ctxt> {
    fn visit_operand(&mut self, _: crate::mir::Location, operand: &mut Operand<'ctxt>) {
        if let Operand::Load(place) = operand
            && place.projections.is_empty()
            && let PlaceBase::Local(local) = place.base
            && let Some(value) = self.values[local].clone()
        {
            *operand = Operand::Constant(value);
        }
    }
}
