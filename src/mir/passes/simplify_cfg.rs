use crate::{
    index_vec::IndexVec,
    mir::{BasicBlock, BasicBlockId, StmtKind, TerminatorKind, passes::BodyPass},
};

pub enum SimplifyCfg {
    Initial,
    AfterInlining,
}

impl<'ctxt> BodyPass<'ctxt> for SimplifyCfg {
    fn name(&self) -> &'static str {
        match *self {
            Self::AfterInlining => "simplify-cfg-after-inlining",
            Self::Initial => "simplify-cfg-initial",
        }
    }
    fn run(&self, _: crate::CtxtRef<'ctxt>, body: &mut crate::mir::Body<'ctxt>) {
        for block in body.block_info.blocks_mut() {
            Self::remove_noops(block);
        }
        let mut modified = true;
        while modified {
            modified = false;
            let block_indices = body.block_info.blocks().indices().collect::<Vec<_>>();
            for block in block_indices {
                match body.block_info.blocks()[block].expect_terminator().kind {
                    TerminatorKind::Goto(target, ref args) => {
                        if !args.is_empty() {
                            continue;
                        }
                        if body.block_info.predecessors()[target].len() != 1 {
                            continue;
                        }
                        Self::steal(body.block_info.blocks_mut(), target, block);
                        modified = true;
                        continue;
                    }
                    _ => continue,
                };
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
