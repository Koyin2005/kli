use crate::{
    index_vec::IndexVec,
    mir::{
        BasicBlockId,
        passes::{MirPass, reachable},
    },
};

pub struct RemoveUnreachable;
impl MirPass for RemoveUnreachable {
    fn name(&self) -> &'static str {
        "remove-unreachable"
    }
    fn run(&self, _ctxt: crate::CtxtRef<'_>, body: &mut crate::mir::Body) {
        let seen = reachable(&body.blocks);
        let mut next_block_id = BasicBlockId::ENTRY;
        let block_map = body
            .blocks
            .indices()
            .map(|id| {
                let next_block = next_block_id.next();
                if seen.contains(&id) {
                    Some(std::mem::replace(&mut next_block_id, next_block))
                } else {
                    None
                }
            })
            .collect::<IndexVec<BasicBlockId, _>>();
        for (_, block) in body.blocks.iter_mut_enumerated() {
            block
                .expect_terminator_mut()
                .successors_mut()
                .for_each(|block| {
                    let Some(id) = block_map[*block] else {
                        return;
                    };
                    *block = id;
                });
        }
        body.blocks.retain(|id, _| block_map[id].is_some());
    }
}
