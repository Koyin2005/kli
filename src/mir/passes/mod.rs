use crate::{
    CtxtRef,
    config::{Config, Feature},
    mir::{
        Body, BodySource,
        dump::MirDump,
        passes::{remove_unreachable::RemoveUnreachable, remove_zst::RemoveZst},
    },
};
mod remove_unreachable;
mod remove_zst;
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
        passes.push(&RemoveUnreachable as &_);
    }
    if config.features.contains_key(&Feature::OutputMir) {
        passes.push(&DumpMir as &_);
    }
    passes.into_boxed_slice()
}
