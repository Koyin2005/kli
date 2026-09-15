use crate::{
    index_vec::IndexVec,
    mir::{
        BasicBlock, BasicBlockId, Operation, Stmt, StmtKind, TerminatorKind, Value,
        passes::BodyPass,
    },
    src_loc::SrcLoc,
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
        let mut modified = true;
        while modified {
            modified = false;
            let block_indices = body.block_info.blocks().indices().collect::<Vec<_>>();
            for block in block_indices {
                let term = body.block_info.blocks()[block].expect_terminator();
                let loc = term.src_info;
                match term.kind {
                    TerminatorKind::Goto(target, ref args) => {
                        if body.block_info.predecessors()[target].len() != 1 {
                            continue;
                        }
                        let args = args.clone();
                        modified =
                            Self::steal(body.block_info.blocks_mut(), target, block, args, loc);
                        continue;
                    }
                    TerminatorKind::Switch(ref value, ref targets) => {
                        let scalar_value = match value {
                            Value::Bool(value) => *value as i64,
                            Value::Int(value) => *value,
                            Value::Char(value) => *value as i64,
                            _ => continue,
                        };
                        let target = targets.branch_for_value(scalar_value as i128);
                        modified = Self::steal(
                            body.block_info.blocks_mut(),
                            target,
                            block,
                            Vec::new(),
                            loc,
                        );
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
    fn steal<'ctxt>(
        blocks: &mut IndexVec<BasicBlockId, BasicBlock<'ctxt>>,
        target: BasicBlockId,
        block: BasicBlockId,
        args: Vec<Value<'ctxt>>,
        loc: SrcLoc,
    ) -> bool {
        if target == block {
            return false;
        }
        let new_stmts = std::mem::take(&mut blocks[target].stmts);
        {
            for (reg, arg) in blocks[target].args.clone().into_iter().zip(args) {
                blocks[block].stmts.push(Stmt {
                    loc,
                    kind: StmtKind::Assign(reg, Operation::Copy(arg)),
                });
            }
        }
        blocks[block].stmts.extend(new_stmts);

        let new_term = std::mem::replace(
            &mut blocks[target].expect_terminator_mut().kind,
            TerminatorKind::Unreachable,
        );
        blocks[block].expect_terminator_mut().kind = new_term;
        true
    }
    fn remove_noops(block: &mut BasicBlock) {
        block.stmts.retain(|_, stmt| {
            !matches!(
                stmt.kind,
                StmtKind::Noop | StmtKind::PanicIf(Value::Bool(false))
            )
        });
    }
}
