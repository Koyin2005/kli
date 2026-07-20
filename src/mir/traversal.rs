use std::collections::{HashMap, HashSet};

use crate::{
    index_vec::IndexVec,
    mir::{self, BasicBlockId, Successors, basic_blocks::BasicBlocks, passes::preorderder},
};

#[derive(Clone, Debug)]
pub struct DominatorTree {
    idoms: IndexVec<BasicBlockId, BasicBlockId>,
}
impl DominatorTree {
    pub fn immediate_dominator(&self, block: BasicBlockId) -> BasicBlockId {
        self.idoms.get(block).copied().unwrap_or(block)
    }
    pub fn dominates(&self, block: BasicBlockId, other: BasicBlockId) -> bool {
        if block == other {
            return true;
        }
        let mut current = other;
        loop {
            if current == block {
                return true;
            }
            let idom = self.idoms[current];
            if idom == current {
                return false;
            }
            current = idom;
        }
    }
    pub fn dominates_location(&self, loc: mir::Location, other: mir::Location) -> bool {
        if loc == other {
            return true;
        }
        if !self.dominates(loc.block, other.block) {
            return false;
        }
        match (loc.stmt, other.stmt) {
            (None, None) | (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(stmt), Some(other)) => stmt < other,
        }
    }
}

fn compress(
    ancestors: &mut IndexVec<BasicBlockId, Option<BasicBlockId>>,
    min_path_label: &mut IndexVec<BasicBlockId, BasicBlockId>,
    block: BasicBlockId,
    ancestor: BasicBlockId,
    dfs_num: &HashMap<BasicBlockId, usize>,
) {
    let mut stack = vec![block];
    let mut current = ancestor;
    while let Some(ancestor) = ancestors[current] {
        if ancestors[ancestor].is_none() {
            break;
        }
        stack.push(current);
        current = ancestor;
    }
    for w in stack.into_iter().rev() {
        let Some(ancestor) = ancestors[w] else {
            continue;
        };
        if dfs_num[&min_path_label[ancestor]] < dfs_num[&min_path_label[w]] {
            min_path_label[w] = min_path_label[ancestor];
        }
        ancestors[w] = ancestors[ancestor];
    }
}
fn eval(
    min_path_label: &mut IndexVec<BasicBlockId, BasicBlockId>,
    ancestors: &mut IndexVec<BasicBlockId, Option<BasicBlockId>>,
    dfs_num: &HashMap<BasicBlockId, usize>,
    block: BasicBlockId,
) -> BasicBlockId {
    let Some(ancestor) = ancestors[block] else {
        return block;
    };
    compress(ancestors, min_path_label, block, ancestor, dfs_num);
    min_path_label[block]
}
fn link(
    ancestors: &mut IndexVec<BasicBlockId, Option<BasicBlockId>>,
    parent: BasicBlockId,
    node: BasicBlockId,
) {
    ancestors[node] = Some(parent);
}
pub fn dominators(bbs: &BasicBlocks) -> DominatorTree {
    let dfs = preorderder(bbs.blocks());
    let dfs_num = dfs
        .iter()
        .enumerate()
        .map(|(i, (_, block))| (*block, i))
        .collect::<HashMap<_, _>>();
    let mut semi_dominator =
        IndexVec::<BasicBlockId, _>::from_iter((0..bbs.blocks().len()).map(BasicBlockId::new));
    let mut semi_dominator_bucket =
        IndexVec::<BasicBlockId, _>::from_iter((0..bbs.blocks().len()).map(|_| Vec::new()));

    let mut idom =
        IndexVec::<BasicBlockId, _>::from_iter((0..bbs.blocks().len()).map(BasicBlockId::new));
    let mut min_path_label =
        IndexVec::<BasicBlockId, _>::from_iter((0..bbs.blocks().len()).map(BasicBlockId::new));
    let mut ancestors = IndexVec::<BasicBlockId, _>::from_value(bbs.blocks().len(), None);
    for &(parent, node) in dfs.iter().rev() {
        let Some(parent) = parent else {
            continue;
        };
        for &pred in &bbs.predecessors()[node] {
            let u = eval(&mut min_path_label, &mut ancestors, &dfs_num, pred);
            if dfs_num[&u] < dfs_num[&semi_dominator[node]] {
                semi_dominator[node] = u;
            }
        }
        let semi = semi_dominator[node];
        semi_dominator_bucket[semi].push(node);

        link(&mut ancestors, parent, node);

        for v in semi_dominator_bucket[parent].drain(..) {
            let u = eval(&mut min_path_label, &mut ancestors, &dfs_num, v);
            idom[v] = if dfs_num[&u] < dfs_num[&semi_dominator[v]] {
                u
            } else {
                semi_dominator[v]
            };
        }
    }

    for &(parent, w) in dfs.iter() {
        let Some(_) = parent else {
            continue;
        };
        if idom[w] != semi_dominator[w] {
            idom[w] = idom[idom[w]];
        }
    }

    DominatorTree { idoms: idom }
}
pub struct Postorder<'a> {
    visit_stack: Vec<(BasicBlockId, Successors<'a>)>,
    visited: HashSet<BasicBlockId>,
    nodes: Vec<BasicBlockId>,
    bbs: &'a IndexVec<BasicBlockId, mir::BasicBlock>,
}
impl Postorder<'_> {
    fn new(bbs: &'_ BasicBlocks) -> Postorder<'_> {
        let mut this = Postorder {
            visit_stack: Vec::new(),
            visited: HashSet::new(),
            nodes: Vec::new(),
            bbs: bbs.blocks(),
        };
        this.visit(BasicBlockId::ENTRY);
        this.traverse_sucessors();
        this
    }
    fn visit(&mut self, node: BasicBlockId) {
        if !self.visited.insert(node) {
            return;
        }
        self.nodes.push(node);
        self.visit_stack
            .push((node, self.bbs[node].expect_terminator().successors()));
    }
    fn traverse_sucessors(&mut self) {
        while let Some(bb) = self
            .visit_stack
            .last_mut()
            .and_then(|(_, succs)| succs.next_back())
        {
            self.visit(bb);
        }
    }
}
impl Iterator for Postorder<'_> {
    type Item = BasicBlockId;
    fn next(&mut self) -> Option<Self::Item> {
        let (bb, _) = self.visit_stack.pop()?;
        self.traverse_sucessors();
        Some(bb)
    }
}

pub fn postorder(bbs: &'_ BasicBlocks) -> Postorder<'_> {
    Postorder::new(bbs)
}

pub fn reachable(blocks: &BasicBlocks) -> HashSet<BasicBlockId> {
    preorderder(blocks.blocks())
        .into_iter()
        .map(|(_, node)| node)
        .collect()
}
