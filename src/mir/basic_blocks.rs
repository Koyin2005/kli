use std::{rc::Rc, sync::OnceLock};

use crate::{
    index_vec::IndexVec,
    mir::{
        BasicBlock, BasicBlockId,
        traversal::{self, postorder},
    },
};

pub type Predecessors = IndexVec<BasicBlockId, Vec<BasicBlockId>>;

#[derive(Default)]
struct Cache {
    dominators: OnceLock<traversal::DominatorTree>,
    predecessors: OnceLock<Predecessors>,
    reverse_postorder: OnceLock<Vec<BasicBlockId>>,
}
#[derive(Clone)]
pub struct BasicBlocks {
    blocks: IndexVec<BasicBlockId, BasicBlock>,
    cache: Rc<Cache>,
}
impl BasicBlocks {
    pub fn new(blocks: IndexVec<BasicBlockId, BasicBlock>) -> Self {
        Self {
            blocks,
            cache: Rc::new(Cache::default()),
        }
    }
    pub fn blocks(&self) -> &IndexVec<BasicBlockId, BasicBlock> {
        &self.blocks
    }
    pub fn dominators(&self) -> &traversal::DominatorTree {
        self.cache
            .dominators
            .get_or_init(|| traversal::dominators(self))
    }
    pub fn predecessors(&self) -> &Predecessors {
        self.cache.predecessors.get_or_init(|| {
            let mut preds = Predecessors::new_from(self.blocks.len(), Vec::new());
            for (block_id, block) in self.blocks.iter_enumerated() {
                if let Some(terminator) = &block.terminator {
                    for succ in terminator.successors() {
                        preds[succ].push(block_id);
                    }
                }
            }
            preds
        })
    }
    pub fn reverse_postorder(&self) -> &[BasicBlockId] {
        self.cache.reverse_postorder.get_or_init(|| {
            let mut post_order = postorder(self).collect::<Vec<_>>();
            post_order.reverse();
            post_order
        })
    }
    pub fn blocks_mut(&mut self) -> &mut IndexVec<BasicBlockId, BasicBlock> {
        self.dirty_cache();
        self.blocks_mut_dont_dirty()
    }
    pub fn blocks_mut_dont_dirty(&mut self) -> &mut IndexVec<BasicBlockId, BasicBlock> {
        &mut self.blocks
    }
    fn dirty_cache(&mut self) {
        if let Some(cache) = Rc::get_mut(&mut self.cache) {
            *cache = Cache::default();
        } else {
            self.cache = Rc::new(Cache::default());
        }
    }
}
