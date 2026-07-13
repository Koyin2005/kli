use std::collections::{HashSet, VecDeque};

use crate::{
    CtxtRef, Symbol,
    config::Feature,
    index_vec::IndexVec,
    mir::{
        BasicBlock, BasicBlockId, Body, BodySource,
        dump::MirDump,
        passes::{
            dead_store::DeadStoreElim, remove_unreachable::RemoveUnreachable,
            remove_unused_locals::RemoveUnusedLocals, remove_zst::RemoveZst,
            simplify_cfg::SimplifyCfg,
        },
    },
};
mod dead_store;
mod remove_unreachable;
mod remove_unused_locals;
mod remove_zst;
mod simplify_cfg;
pub(super) fn optimisation_enabled(ctxt: CtxtRef<'_>) -> bool {
    ctxt.config().has_feature(Feature::Optimise)
}
pub trait MirPass {
    fn name(&self) -> &'static str;
    fn run(&self, ctxt: CtxtRef<'_>, body: &mut Body);
    fn enabled(&self, ctxt: CtxtRef<'_>) -> bool {
        _ = ctxt;
        true
    }
}

pub(super) fn should_dump(ctxt: CtxtRef<'_>, src: BodySource) -> bool {
    let Some(paths) = ctxt.config().arguments_for(Feature::OutputMir) else {
        return false;
    };
    let body_src = src.def_id();
    paths.iter().any(|path| {
        let Some(id) = ctxt.def_id_for_path(path.to_string().split(".").map(Symbol::intern)) else {
            return false;
        };
        ctxt.self_with_anecstors(body_src).any(|src| src == id)
    })
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
    fn enabled(&self, ctxt: CtxtRef<'_>) -> bool {
        ctxt.config().has_feature(Feature::OutputMir)
    }
}
pub fn passes() -> &'static [&'static dyn MirPass] {
    &[
        &RemoveZst,
        &SimplifyCfg,
        &SimplifyCfg,
        &RemoveUnreachable,
        &DeadStoreElim,
        &RemoveUnusedLocals,
        &SimplifyCfg,
        &RemoveUnreachable,
        &DumpMir,
    ]
}

pub fn preorderder(
    blocks: &IndexVec<BasicBlockId, BasicBlock>,
) -> Vec<(Option<BasicBlockId>, BasicBlockId)> {
    let mut stack = VecDeque::from([(None, BasicBlockId::ENTRY)]);
    let mut seen = HashSet::new();
    let mut bbs = Vec::new();
    while let Some((parent, current)) = stack.pop_front() {
        if !seen.insert(current) {
            continue;
        }
        bbs.push((parent, current));
        for successor in blocks[current].expect_terminator().successors() {
            stack.push_back((Some(current), successor));
        }
    }
    bbs
}
