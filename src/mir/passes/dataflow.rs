#![allow(unused)]

use std::collections::VecDeque;

use crate::{
    index_vec::IndexVec,
    mir::{BasicBlockId, Body, Stmt, Terminator},
};

pub trait Domain: Clone {
    fn initial<'ctxt>(body: &Body<'ctxt>) -> Self;
    fn join(&mut self, other: &Self) -> bool;
}

pub fn prop_uniform<'ctxt, A: Analysis<'ctxt> + ?Sized>(
    a: &A,
    state: &A::Domain,
    terminator: &Terminator<'ctxt>,
    mut f: impl FnMut(BasicBlockId),
) {
    for succ in terminator.successors() {
        f(succ);
    }
}
pub trait Analysis<'ctxt> {
    type Domain: Domain + Default;

    fn apply_stmt_effect(&mut self, state: &mut Self::Domain, stmt: &Stmt<'ctxt>);

    fn propagate_to_basic_blocks(
        &self,
        state: &Self::Domain,
        terminator: &Terminator<'ctxt>,
        f: impl FnMut(BasicBlockId),
    ) {
        prop_uniform(self, state, terminator, f);
    }
    fn iterate_to_fixpoint(&mut self, body: &Body<'ctxt>) -> IndexVec<BasicBlockId, Self::Domain> {
        let mut state = Self::Domain::default();
        let mut states =
            IndexVec::<BasicBlockId, _>::from_function(body.block_info.blocks().len(), |_| {
                Self::Domain::initial(body)
            });
        let mut queue = VecDeque::new();
        queue.push_front(BasicBlockId::ENTRY);
        while let Some(block) = queue.pop_back() {
            state.clone_from(&states[block]);
            for stmt in body.block_info.blocks()[block].stmts.iter() {
                self.apply_stmt_effect(&mut state, stmt);
            }
            let terminator = body.block_info.blocks()[block].expect_terminator();
            self.propagate_to_basic_blocks(&state, terminator, |succ| {
                let new_state = &mut states[succ];
                if new_state.join(&state) {
                    queue.push_front(succ);
                }
            });
        }
        states
    }
}
