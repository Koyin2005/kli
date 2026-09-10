use std::usize;

use crate::{
    CtxtRef,
    mir::{
        self, BasicBlock, BasicBlockId, Body, BodySource, ConstValue, Constant, Local, Location,
        Operand, Place, PlaceBase, Rvalue, StmtKind, Terminator, TerminatorKind,
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

    let budgets = mir
        .bodies
        .iter_mut()
        .map(|current_body| {
            let budget = inline_budget_for_body(current_body);
            split_calls(current_body);
            budget
        })
        .collect::<Vec<_>>();
    let mut updated_mir = mir.clone();
    for (current_body, mut budget) in updated_mir.bodies.iter_mut().zip(budgets) {
        loop {
            let site = if let Some(site) = find_inlining_site(mir, current_body) {
                site
            } else {
                break;
            };
            let InlininingSite {
                return_place,
                src,
                generic_args,
                block,
                args,
            } = site;
            let body_id = mir.get_bodies_with_src(src)[0];
            budget = if let Some(next_budget) =
                budget.checked_sub(inline_budget_used_by(&mir.bodies[body_id]))
            {
                next_budget
            } else {
                break;
            };

            let mut body = instantiate_body(ctxt, mir, body_id, generic_args);
            {
                let callee_entry = current_body.block_info.blocks().last().next();
                let current_block = &mut current_body.block_info.blocks_mut()[block];
                assert!(current_block.stmts.pop().is_some_and(|stmt| {
                    if let StmtKind::Assign(_, rvalue) = stmt.kind
                        && let Rvalue::Call(..) = *rvalue
                    {
                        true
                    } else {
                        false
                    }
                }));
                let TerminatorKind::Goto(ref mut target) =
                    current_block.expect_terminator_mut().kind
                else {
                    panic!("should be a goto")
                };
                let target = std::mem::replace(target, callee_entry);
                for (local, arg) in body.params_iter().zip(args) {
                    let local = Local::new(local.into_usize() + current_body.locals.len());
                    current_block.stmts.push(mir::Stmt {
                        loc: SrcLoc::dummy(),
                        kind: StmtKind::Assign(Place::local(local), Box::new(Rvalue::Use(arg))),
                    });
                }
                let mut updater = Updater {
                    block_count: current_body.block_info.blocks().len() as _,
                    local_count: current_body.locals.len() as _,
                    return_target: target,
                    return_place: return_place,
                };
                updater.visit_body(&mut body);
                current_body
                    .block_info
                    .blocks_mut()
                    .extend(body.block_info.into_blocks());
                current_body.locals.extend(body.locals);
            }
        }
    }
    *mir = updated_mir;
}
struct InlininingSite<'ctxt> {
    src: BodySource,
    generic_args: GenericArgs<'ctxt>,
    block: BasicBlockId,
    return_place: Place,
    args: Vec<Operand<'ctxt>>,
}
fn find_inlining_site<'ctxt>(
    mir: &mir::Context<'ctxt>,
    body: &Body<'ctxt>,
) -> Option<InlininingSite<'ctxt>> {
    let mut site = None;
    for (block_id, block) in body.block_info.blocks().iter_enumerated() {
        for stmt in block.stmts.iter() {
            let StmtKind::Assign(place, value) = &stmt.kind else {
                continue;
            };
            let Rvalue::Call(
                Operand::Constant(Constant {
                    ty: _,
                    value: ConstValue::Named(id, ref generic_args),
                }),
                ref args,
            ) = **value
            else {
                continue;
            };
            let call_site = InlininingSite {
                src: BodySource::Function(id),
                return_place: place.clone(),
                generic_args: generic_args.clone(),
                block: block_id,
                args: args.clone(),
            };

            let body_id = mir.get_bodies_with_src(call_site.src)[0];
            let call_site_budget = inline_budget_used_by(mir.get_body(body_id));
            if let Some((current_budget, _)) = site
                && current_budget <= call_site_budget
            {
                continue;
            }
            site = Some((call_site_budget, call_site));
        }
    }
    site.map(|(_, site)| site)
}

fn split_calls<'ctxt>(body: &mut Body<'ctxt>) {
    let mut block_id = BasicBlockId::ENTRY;
    let blocks = body.block_info.blocks_mut();
    while let Some(block) = blocks.get_mut(block_id) {
        let mut call_stmt = None;
        for (stmt_id, stmt) in block.stmts.iter_enumerated() {
            let StmtKind::Assign(_, value) = &stmt.kind else {
                continue;
            };
            let Rvalue::Call(
                Operand::Constant(Constant {
                    ty: _,
                    value: ConstValue::Named(..),
                }),
                _,
            ) = **value
            else {
                continue;
            };
            if stmt_id != block.stmts.last() {
                call_stmt = Some(stmt_id);
                break;
            } else if !matches!(block.expect_terminator().kind, TerminatorKind::Goto(_)) {
                call_stmt = Some(stmt_id);
                break;
            }
        }
        if let Some(call_stmt) = call_stmt {
            let stmts = block.stmts.split_off(call_stmt.next());
            let src_info = block.expect_terminator().src_info;
            let terminator = block.terminator.take();

            let next_block_id = blocks.push(BasicBlock { stmts, terminator });
            blocks[block_id].terminator = Some(Terminator {
                src_info,
                kind: mir::TerminatorKind::Goto(next_block_id),
            });
        }
        block_id = block_id.next();
    }
}

struct Updater {
    local_count: u32,
    block_count: u32,
    return_target: BasicBlockId,
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
                terminator.kind = mir::TerminatorKind::Goto(self.return_target);
            }
            _ => {
                for succ in terminator.successors_mut() {
                    *succ = BasicBlockId(succ.0 + self.block_count);
                }
            }
        }
    }
}

fn inline_budget_used_by(body: &Body<'_>) -> u32 {
    let total_cost = body
        .block_info
        .blocks()
        .iter()
        .map(|block| block.stmts.len())
        .sum::<usize>()
        + body.locals.len();
    total_cost.try_into().unwrap_or(100)
}

fn inline_budget_for_body(body: &Body<'_>) -> u32 {
    let mut total_budget = 15;
    if body.block_info.blocks().len() < 4 {
        total_budget += 10;
    }
    total_budget
}
