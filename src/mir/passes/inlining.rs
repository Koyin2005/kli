use std::collections::HashMap;

use crate::{
    CtxtRef,
    def_ids::DefId,
    mir::{
        BasicBlock, BasicBlockId, ConstValue, Constant, Local, Location, Operand, Place, PlaceBase,
        Rvalue, StmtKind, TerminatorKind,
        passes::{MirPass, optimisation_enabled, should_dump},
        visitor::{MutVisit, Visit},
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
    fn run_with_ctxt(
        &self,
        ctxt: CtxtRef<'ctxt>,
        body: &'_ mut crate::mir::Body<'ctxt>,
        mir_ctxt: &crate::mir::Context<'ctxt>,
    ) {
        let mut inline_budget = 10usize;
        let mut inline_sites = InlineSiteFinder { site: None, ctxt };
        loop {
            let Some(new_budget) = inline_budget.checked_sub(1) else {
                break;
            };
            inline_budget = new_budget;
            inline_sites.visit_body(body);
            let Some(site) = inline_sites.site.take() else {
                break;
            };
            if should_dump(ctxt, body.src) {
                println!("Inlining {}", ctxt.display_path_for(site.call));
            }
            let InlineSite {
                location,
                place,
                call: id,
                args,
            } = site;
            let current_block_count = body.block_info.blocks().len();
            let current_local_count = body.locals.len();
            let mut callee_blocks = HashMap::new();
            let target = mir_ctxt.with_body(crate::mir::BodySource::Function(id), |func_body| {
                let new_body = func_body.clone();
                let new_locals = new_body.locals;
                let new_blocks = new_body.block_info;

                body.locals.extend(new_locals.into_iter().map(|mut local| {
                    local.kind = match local.kind {
                        crate::mir::LocalKind::Param(_) | crate::mir::LocalKind::Var(_) => {
                            crate::mir::LocalKind::Temp
                        }
                        kind => kind,
                    };
                    local
                }));
                callee_blocks.extend((0..new_blocks.blocks().len()).map(|block_index| {
                    let caller_id = BasicBlockId::new(block_index + current_block_count);
                    let callee_id = BasicBlockId::new(block_index);
                    (caller_id, callee_id)
                }));
                body.block_info
                    .blocks_mut()
                    .extend(new_blocks.into_blocks());
                BasicBlockId::new(current_block_count)
            });
            let Location {
                block,
                stmt: Some(stmt),
            } = location
            else {
                unreachable!()
            };
            let next_block = target;
            let block = &mut body.block_info.blocks_mut()[block];
            block.stmts[stmt].kind = StmtKind::Noop;
            let src_info = block.expect_terminator().src_info;
            let new_terminator = block.terminator.replace(crate::mir::Terminator {
                src_info,
                kind: TerminatorKind::Goto(next_block),
            });

            let (first, stmts) = block.stmts.as_slice().split_at(stmt.into_usize());
            let first_len = first.len();
            let new_stmts = stmts.iter().cloned().collect();
            let new_block = BasicBlock {
                stmts: new_stmts,
                terminator: new_terminator,
            };
            block.stmts.truncate(first_len);
            for (i, arg) in args.into_iter().enumerate() {
                let local = Local::new(current_local_count + i);
                block.stmts.push(crate::mir::Stmt {
                    loc: src_info,
                    kind: StmtKind::Assign(Place::local(local), Box::new(Rvalue::Use(arg))),
                });
            }
            let returned_block = body.block_info.blocks_mut().push(new_block);
            let mut replacer = Replacer {
                return_place: place,
                new_blocks: callee_blocks,
                block_count: current_block_count,
                return_block: returned_block,
                locals_count: current_local_count,
            };
            replacer.visit_body(body);
        }
    }
    fn run(&self, _: crate::CtxtRef<'ctxt>, _: &'_ mut crate::mir::Body<'ctxt>) {
        unreachable!()
    }
}

struct InlineSite<'ctxt> {
    location: Location,
    place: Place,
    call: DefId,
    args: Vec<Operand<'ctxt>>,
}
struct InlineSiteFinder<'ctxt> {
    site: Option<InlineSite<'ctxt>>,
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> Visit<'ctxt> for InlineSiteFinder<'ctxt> {
    fn visit_assign(&mut self, loc: Location, place: &Place, rvalue: &Rvalue<'ctxt>) {
        let Rvalue::Call(
            Operand::Constant(Constant {
                ty: _,
                value: ConstValue::Named(id, generic_args),
            }),
            args,
        ) = rvalue
        else {
            return;
        };
        if !generic_args.is_empty() {
            return;
        }
        let id = *id;
        if should_dump(self.ctxt, crate::mir::BodySource::Function(id)) {
            println!("{:?} {:?}", generic_args, self.ctxt.display_path_for(id));
        }
        self.site.get_or_insert(InlineSite {
            location: loc,
            place: place.clone(),
            call: id,
            args: args.clone(),
        });
    }
}

struct Replacer {
    return_place: Place,
    return_block: BasicBlockId,
    block_count: usize,
    locals_count: usize,
    /// Map from the caller's block to the callee's block
    new_blocks: HashMap<BasicBlockId, BasicBlockId>,
}
impl<'ctxt> MutVisit<'ctxt> for Replacer {
    fn visit_terminator(&mut self, loc: Location, terminator: &mut crate::mir::Terminator<'ctxt>) {
        self.super_visit_terminator(loc, terminator);
        match &mut terminator.kind {
            TerminatorKind::Assert(_, _, block) | TerminatorKind::Goto(block) => {
                *block = BasicBlockId::new(block.into_usize() + self.block_count);
            }
            TerminatorKind::Switch(_, switch_targets) => {
                for target in &mut switch_targets.targets {
                    target.target =
                        BasicBlockId::new(target.target.into_usize() + self.block_count);
                }
                switch_targets.otherwise =
                    BasicBlockId::new(switch_targets.otherwise.into_usize() + self.block_count)
            }
            TerminatorKind::Return => {
                terminator.kind = TerminatorKind::Goto(self.return_block);
            }
            TerminatorKind::Unreachable | TerminatorKind::Panic => (),
        }
    }
    fn visit_local(&mut self, _: Location, local: &mut Local) {
        *local = Local::new(local.into_usize() + self.locals_count);
    }
    fn visit_block(&mut self, id: BasicBlockId, block: &mut BasicBlock<'ctxt>) {
        if !self.new_blocks.contains_key(&id) {
            return;
        }
        self.super_visit_block(id, block);
    }
    fn visit_place(&mut self, loc: Location, place: &mut Place) {
        if place.base != PlaceBase::ReturnPlace {
            self.super_visit_place(loc, place);
            return;
        }
        place.base = self.return_place.base;
        let mut new_projections = self.return_place.projections.clone();
        new_projections.extend(place.projections.drain(..));
        place.projections = new_projections;
    }
}
