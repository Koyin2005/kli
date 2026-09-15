#![allow(unused)]

use std::{collections::{HashSet, VecDeque}, fmt::Debug};

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
    mut f: impl FnMut(BasicBlockId,&A::Domain),
) {
    for succ in terminator.successors() {
        f(succ,state);
    }
}
pub trait Analysis<'ctxt> {
    type Domain: Domain + Default + Debug;

    fn apply_stmt_effect(&mut self, state: &mut Self::Domain, stmt: &Stmt<'ctxt>);
    fn propagate_to_basic_blocks(
        &self,
        state: &Self::Domain,
        terminator: &Terminator<'ctxt>,
        propagate: impl FnMut(BasicBlockId,&Self::Domain),
    ) {
        prop_uniform(self, state, terminator, propagate);
    }
    fn iterate_to_fixpoint(&mut self, body: &Body<'ctxt>) -> IndexVec<BasicBlockId, Self::Domain> {
        let mut state = Self::Domain::default();
        let mut states =
            IndexVec::<BasicBlockId, _>::from_function(body.block_info.blocks().len(), |_| {
                Self::Domain::initial(body)
            });
        let mut queue = VecDeque::new();
        let mut in_queue = HashSet::new();
        queue.push_front(BasicBlockId::ENTRY);
        while let Some(block) = queue.pop_back() {
            in_queue.remove(&block);
            state.clone_from(&states[block]);
            for stmt in body.block_info.blocks()[block].stmts.iter() {
                self.apply_stmt_effect(&mut state, stmt);
            }
            let terminator = body.block_info.blocks()[block].expect_terminator();
            self.propagate_to_basic_blocks(&state, terminator, |succ,state| {
                let new_state = &mut states[succ];
                let changed = new_state.join(&state);
                if changed && in_queue.insert(succ) {
                    queue.push_front(succ);
                }
            });
        }
        states
    }
}
