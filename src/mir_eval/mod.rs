use crate::{mir::Context, monomorph::collect::Instance};

pub struct Interpret<'ctxt> {
    ctxt: &'ctxt Context,
}
impl<'ctxt> Interpret<'ctxt> {
    pub fn new(ctxt: &'ctxt Context) -> Self {
        Self { ctxt }
    }
}

impl Interpret<'_> {
    pub fn run(&mut self, entry: Instance) {}
}
