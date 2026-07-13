use std::collections::BTreeMap;

use crate::{
    index_vec::IndexVec,
    mir::{BasicBlock, BasicBlockId, Operand, StmtKind, TerminatorKind, passes::MirPass},
};

pub struct SimplifyCfg;

impl MirPass for SimplifyCfg {
    fn name(&self) -> &'static str {
        "simplify-cfg"
    }
    fn run(&self, _: crate::CtxtRef<'_>, body: &mut crate::mir::Body) {
        for block in body.block_info.blocks_mut() {
            Self::remove_noops(block);
        }
        let mut modified = true;
        while modified {
            modified = false;
            for block in body.block_info.blocks().indices() {
                let targets = match body.block_info.blocks()[block].expect_terminator().kind {
                    TerminatorKind::Goto(target) => {
                        if body.block_info.predecessors()[target].len() != 1 {
                            continue;
                        }
                        Self::steal(body.block_info.blocks_mut(), target, block);
                        modified = true;
                        continue;
                    }
                    TerminatorKind::Switch(ref operand, ref targets) => {
                        if let Operand::Constant(constant) = operand
                            && let Some(value) = constant.value.as_scalar()
                            && let target = targets.branch_for_value(value as i128)
                            && body.block_info.predecessors()[target].len() == 1
                        {
                            Self::steal(body.block_info.blocks_mut(), target, block);
                            modified = true;
                            continue;
                        }
                        body.block_info.blocks()[block]
                            .expect_terminator()
                            .successors()
                            .filter_map(|succ| {
                                if body.block_info.predecessors()[succ].len() > 1 {
                                    return None;
                                }
                                if !body.block_info.blocks()[succ]
                                    .stmts
                                    .iter()
                                    .all(|stmt| matches!(stmt.kind, StmtKind::Noop))
                                {
                                    return None;
                                }
                                let TerminatorKind::Goto(target) =
                                    body.block_info.blocks()[succ].expect_terminator().kind
                                else {
                                    return None;
                                };
                                Some((succ, target))
                            })
                            .collect::<BTreeMap<_, _>>()
                    }
                    _ => continue,
                };
                if let Some(ref mut terminator) = body.block_info.blocks_mut()[block].terminator {
                    for block in terminator.successors_mut() {
                        *block = if let Some(target) = targets.get(block) {
                            modified = true;
                            *target
                        } else {
                            continue;
                        };
                    }
                }
            }
        }
        for block in body.block_info.blocks_mut_dont_dirty().iter_mut() {
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
