use std::collections::{HashMap, HashSet, VecDeque};

use crate::{
    CtxtRef,
    config::{Config, Feature},
    index_vec::IndexVec,
    mir::{
        BasicBlock, BasicBlockId, Body, BodySource,
        dump::MirDump,
        passes::{
            const_prop::ConstProp, dead_store::DeadStoreElim,
            remove_unreachable::RemoveUnreachable, remove_unused_locals::RemoveUnusedLocals,
            remove_zst::RemoveZst, simplify_cfg::SimplifyCfg,
        },
    },
};
mod const_prop;
mod dead_store;
mod remove_unreachable;
mod remove_unused_locals;
mod remove_zst;
mod simplify_cfg;
pub trait MirPass {
    fn name(&self) -> &'static str;
    fn run(&self, ctxt: CtxtRef<'_>, body: &mut Body);
}

pub(super) fn should_dump(ctxt: CtxtRef<'_>, src: BodySource) -> bool {
    let Some(children) = ctxt.config().features.get(&Feature::OutputMir) else {
        return false;
    };
    children.iter().any(|child| src.is_child_of(*child, ctxt))
}
pub struct DumpMir;
impl MirPass for DumpMir {
    fn name(&self) -> &'static str {
        "dump-mir"
    }
    fn run(&self, ctxt: CtxtRef<'_>, body: &mut Body) {
        if should_dump(ctxt, body.src) {
            let _ = MirDump::new(Box::new(std::io::stdout()), ctxt).write_body(body);
        }
    }
}
pub fn passes(config: &Config) -> Box<[&'static dyn MirPass]> {
    let mut passes = vec![];
    passes.push(&RemoveZst as &_);
    if config.features.contains_key(&Feature::Optimise) {
        passes.push(&SimplifyCfg as &_);
        passes.push(&ConstProp as &_);
        passes.push(&RemoveUnreachable as &_);
        passes.push(&DeadStoreElim as &_);
        passes.push(&RemoveUnusedLocals as &_);
        passes.push(&SimplifyCfg as &_);
        passes.push(&RemoveUnreachable as &_);
    }
    if config.features.contains_key(&Feature::OutputMir) {
        passes.push(&DumpMir as &_);
    }
    passes.into_boxed_slice()
}

pub fn reachable(blocks: &IndexVec<BasicBlockId, BasicBlock>) -> HashSet<BasicBlockId> {
    let mut stack = VecDeque::from([BasicBlockId::ENTRY]);
    let mut seen = HashSet::new();
    while let Some(current) = stack.pop_front() {
        if !seen.insert(current) {
            continue;
        }
        for successor in blocks[current].expect_terminator().successors() {
            stack.push_back(successor);
        }
    }
    seen
}

pub fn predecessors(
    blocks: &IndexVec<BasicBlockId, BasicBlock>,
    block: BasicBlockId,
) -> Vec<BasicBlockId> {
    let succ_map = blocks
        .iter()
        .map(|block| block.expect_terminator().successors().collect::<Vec<_>>())
        .collect::<IndexVec<BasicBlockId, _>>();
    let mut pred_map = HashMap::new();
    for (block, succ) in succ_map.into_iter_enumerated() {
        for succ in succ {
            pred_map.entry(succ).or_insert(HashSet::new()).insert(block);
        }
    }
    pred_map.remove(&block).map_or(Vec::new(), |items| {
        let mut items = items.into_iter().collect::<Vec<_>>();
        items.sort();
        items
    })
}
