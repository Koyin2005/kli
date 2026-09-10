use crate::{
    CtxtRef,
    mir::{
        self, BasicBlock, BasicBlockId, Body, BodySource, ConstValue, Constant, LocalKind,
        Location, Operand, Place, PlaceBase, Rvalue, StmtId, StmtKind, Terminator,
        passes::optimisation_enabled, visitor::MutVisit,
    },
    monomorph::instantiate_body,
    src_loc::SrcLoc,
    types::GenericArgs,
};

pub fn run_pass<'ctxt>(ctxt: CtxtRef<'ctxt>, mir: &mut mir::Context<'ctxt>) {
    if !optimisation_enabled(ctxt) {
        return;
    }
    let mut updated_mir = mir.clone();
    for current_body in updated_mir.bodies.iter_mut() {
        let mut budget: u32 = 20;
        let mut sites = find_inlining_sites(current_body);

        loop {
            let site = if let Some(site) = sites.pop() {
                site
            } else {
                sites = find_inlining_sites(current_body);
                if let Some(site) = sites.pop() {
                    site
                } else {
                    break;
                }
            };


            let InlininingSite {
                return_place,
                src,
                args,
                block,
                stmt,
            } = site;
            let body_id = mir.get_bodies_with_src(src)[0];
            let budget_estimate = mir.bodies[body_id].block_info.blocks().len() as _;
            budget = if let Some(next_budget) = budget.checked_sub(budget_estimate){
                next_budget
            } else {
                break;
            };

            let body = instantiate_body(ctxt, mir, body_id, args);

            let fresh_blocks = body.block_info.into_blocks();
            let locals = body.locals;
            let entry_block = current_body.block_info.blocks().last();

            let current_block = &mut current_body.block_info.blocks_mut()[block];
            let rest = current_block.stmts.split_off(stmt.next());

            current_block.stmts.pop();
            let terminator = current_block.terminator.take();

            let old_block_count = current_body.block_info.blocks().len();
            let old_local_count = current_body.locals.len();
            current_body.block_info.blocks_mut().extend(fresh_blocks);
            current_body
                .locals
                .extend(locals.into_iter().map(|mut local| {
                    local.kind = LocalKind::Temp;
                    local
                }));

            struct Updater {
                local_count: u32,
                block_count: u32,
                target: BasicBlockId,
                return_place: Place,
            }

            impl<'ctxt> MutVisit<'ctxt> for Updater {
                fn visit_local(&mut self, _: Location, local: &mut mir::Local) {
                    *local = mir::Local(local.0 + self.local_count);
                }
                fn visit_place(&mut self, loc: Location, place: &mut Place) {
                    self.super_visit_place(loc, place);
                    if place.base != PlaceBase::ReturnPlace {
                        return;
                    }
                    let old_projections = std::mem::take(&mut place.projections);
                    *place = self.return_place.clone();
                    place.projections.extend(old_projections);
                }
                fn visit_terminator(&mut self, loc: Location, terminator: &mut Terminator<'ctxt>) {
                    self.super_visit_terminator(loc, terminator);
                    match &mut terminator.kind {
                        mir::TerminatorKind::Return => {
                            terminator.kind = mir::TerminatorKind::Goto(self.target);
                        }
                        _ => {
                            for succ in terminator.successors_mut() {
                                *succ = BasicBlockId(succ.0 + self.block_count);
                            }
                        }
                    }
                }
            }
            let target = current_body.block_info.blocks_mut().push(BasicBlock {
                stmts: rest,
                terminator,
            });
            let mut updater = Updater {
                return_place,
                block_count: old_block_count as u32,
                target,
                local_count: old_local_count as _,
            };
            current_body.block_info.blocks_mut()[block].terminator = Some(Terminator {
                src_info: SrcLoc::dummy(),
                kind: mir::TerminatorKind::Goto(entry_block),
            });
            for block in (old_block_count..target.into_usize()).map(BasicBlockId::new) {
                updater.visit_block(block, &mut current_body.block_info.blocks_mut()[block]);
            }
        }
    }
    *mir = updated_mir;
}
struct InlininingSite<'ctxt> {
    src: BodySource,
    args: GenericArgs<'ctxt>,
    block: BasicBlockId,
    return_place: Place,
    stmt: StmtId,
}
fn find_inlining_sites<'ctxt>(body: &Body<'ctxt>) -> Vec<InlininingSite<'ctxt>> {
    let mut sites = Vec::new();
    for (block_id, block) in body.block_info.blocks().iter_enumerated() {
        for (stmt_id, stmt) in block.stmts.iter_enumerated() {
            let StmtKind::Assign(place, value) = &stmt.kind else {
                continue;
            };
            let Rvalue::Call(
                Operand::Constant(Constant {
                    ty: _,
                    value: ConstValue::Named(id, ref args),
                }),
                _,
            ) = **value
            else {
                continue;
            };
            sites.push(InlininingSite {
                src: BodySource::Function(id),
                return_place: place.clone(),
                args: args.clone(),
                block: block_id,
                stmt: stmt_id,
            });
        }
    }
    sites
}
