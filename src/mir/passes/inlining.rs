use crate::{
    CtxtRef,
    mir::{
        ConstValue, Constant, Operand, Rvalue,
        passes::{MirPass, optimisation_enabled},
        visitor::MutVisit,
    },
};

pub(super) struct Inline;
impl<'ctxt> MirPass<'ctxt> for Inline {
    fn name(&self) -> &'static str {
        "inline"
    }
    fn enabled(&self, ctxt: crate::CtxtRef<'ctxt>) -> bool {
        optimisation_enabled(ctxt)
    }
    fn run(&self, ctxt: crate::CtxtRef<'ctxt>, body: &'_ mut crate::mir::Body<'ctxt>) {
        Inliner { ctxt }.visit_body(body);
    }
}

struct Inliner<'ctxt> {
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> MutVisit<'ctxt> for Inliner<'ctxt> {
    fn visit_rvalue(&mut self, loc: crate::mir::Location, rvalue: &mut crate::mir::Rvalue<'ctxt>) {
        if let Rvalue::Call(
            Operand::Constant(Constant {
                ty: _,
                value: ConstValue::Named(id, generic_args),
            }),
            args,
        ) = rvalue
        {}
        self.super_visit_rvalue(loc, rvalue);
    }
}
