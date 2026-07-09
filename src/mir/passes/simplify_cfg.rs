use std::collections::BTreeMap;

use crate::{
    index_vec::IndexVec,
    mir::{
        BasicBlock, BasicBlockId, Operand, StmtKind, TerminatorKind,
        passes::{MirPass, predecessors},
    },
};

pub struct SimplifyCfg;

impl MirPass for SimplifyCfg {
    fn name(&self) -> &'static str {
        "simplify-cfg"
    }
    fn run(&self, _: crate::CtxtRef<'_>, body: &mut crate::mir::Body) {
        for block in body.blocks.iter_mut() {
            Self::remove_noops(block);
        }
        let mut modified = true;
        while modified {
            modified = false;
            for block in body.blocks.indices() {
                let targets = match body.blocks[block].expect_terminator().kind {
                    TerminatorKind::Goto(target) => {
                        if predecessors(&body.blocks, target).len() != 1 {
                            continue;
                        }
                        Self::steal(&mut body.blocks, target, block);
                        modified = true;
                        continue;
                    }
                    TerminatorKind::Switch(ref operand, ref targets) => {
                        if let Operand::Constant(constant) = operand
                            && let Some(value) = constant.value.as_scalar()
                            && let target = targets.branch_for_value(value)
                            && predecessors(&body.blocks, target).len() == 1
                        {
                            Self::steal(&mut body.blocks, target, block);
                            modified = true;
                            continue;
                        }
                        body.blocks[block]
                            .expect_terminator()
                            .successors()
                            .filter_map(|succ| {
                                if predecessors(&body.blocks, succ).len() > 1 {
                                    return None;
                                }
                                if !body.blocks[succ].stmts.iter().all(|stmt| match stmt.kind {
                                    StmtKind::Noop => true,
                                    _ => false,
                                }) {
                                    return None;
                                }
                                let TerminatorKind::Goto(target) =
                                    body.blocks[succ].expect_terminator().kind
                                else {
                                    return None;
                                };
                                Some((succ, target))
                            })
                            .collect::<BTreeMap<_, _>>()
                    }
                    _ => continue,
                };
                for block in body.blocks[block].expect_terminator_mut().successors_mut() {
                    *block = if let Some(target) = targets.get(block) {
                        modified = true;
                        *target
                    } else {
                        continue;
                    };
                }
            }
        }
        for block in body.blocks.iter_mut() {
            Self::remove_noops(block);
        }
    }
}
impl SimplifyCfg {
    fn steal(
        blocks: &mut IndexVec<BasicBlockId, BasicBlock>,
        target: BasicBlockId,
        block: BasicBlockId,
    ) {
        let new_stmts = std::mem::take(&mut blocks[target].stmts);
        blocks[block].stmts.extend(new_stmts);

        let new_term = std::mem::replace(
            &mut blocks[target].expect_terminator_mut().kind,
            TerminatorKind::Unreachable,
        );
        blocks[block].expect_terminator_mut().kind = new_term;
    }
    fn remove_noops(block: &mut BasicBlock) {
        block
            .stmts
            .retain(|_, stmt| !matches!(stmt.kind, StmtKind::Noop));
    }
}
