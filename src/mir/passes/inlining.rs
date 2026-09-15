use crate::{
    CtxtRef,
    mir::{
        self, BasicBlock, BasicBlockId, Body, BodySource, Local, Location, Place, TerminatorKind,
        passes::optimisation_enabled, visitor::MutVisit,
    },
    monomorph::instantiate_body,
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
        while let Some(site) = find_inlining_site(mir, current_body) {
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
                    _ = stmt;
                    false
                }));
                let TerminatorKind::Goto(ref mut target, ..) =
                    current_block.expect_terminator_mut().kind
                else {
                    panic!("should be a goto")
                };
                let target = std::mem::replace(target, callee_entry);
                for (local, arg) in body.param_locals_iter().zip(args) {
                    let _local = Local::new(local.into_usize() + current_body.locals.len());
                    _ = arg;
                    todo!("fix inlining");
                }
                let mut updater = Updater {
                    block_count: current_body.block_info.blocks().len() as _,
                    local_count: current_body.locals.len() as _,
                    return_target: target,
                    _return_place: return_place,
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
    return_place: Place<'ctxt>,
    args: Vec<()>,
}
fn find_inlining_site<'ctxt>(
    mir: &mir::Context<'ctxt>,
    body: &Body<'ctxt>,
) -> Option<InlininingSite<'ctxt>> {
    _ = mir;
    _ = body;
    todo!("fix stuff")
}

fn split_calls<'ctxt>(body: &mut Body<'ctxt>) {
    _ = body;
    todo!("fix stuff")
}

struct Updater<'ctxt> {
    local_count: u32,
    block_count: u32,
    return_target: BasicBlockId,
    _return_place: Place<'ctxt>,
}

impl<'ctxt> MutVisit<'ctxt> for Updater<'ctxt> {
    fn visit_local(&mut self, _: Location, local: &mut mir::Local) {
        *local = mir::Local(local.0 + self.local_count);
    }
    fn visit_block(&mut self, id: BasicBlockId, block: &mut BasicBlock<'ctxt>) {
        for (stmt_id, stmt) in block.stmts.iter_mut_enumerated() {
            self.visit_stmt(mir::Location::stmt(id, stmt_id), stmt);
        }
        let terminator = block.expect_terminator_mut();
        self.visit_terminator(mir::Location::terminator(id), terminator);
        let _src_info = terminator.src_info;
        match &mut terminator.kind {
            mir::TerminatorKind::Return(_) => {
                let mir::TerminatorKind::Return(value) = std::mem::replace(
                    &mut terminator.kind,
                    mir::TerminatorKind::Goto(self.return_target, Vec::new()),
                ) else {
                    unreachable!()
                };
                _ = value;
                todo!("fix inlining")
            }
            _ => {
                for succ in terminator.successors_mut() {
                    *succ = BasicBlockId(succ.0 + self.block_count);
                }
            }
        };
    }
}

const INSTR_BUDGET: u32 = 1;
fn inline_budget_used_by(body: &Body<'_>) -> u32 {
    let total_cost = body
        .block_info
        .blocks()
        .iter()
        .map(|block| {
            block.stmts.len()
                + 'a: {
                    let Some(term) = &block.terminator else {
                        break 'a INSTR_BUDGET;
                    };
                    match term.kind {
                        TerminatorKind::Switch(_, ref switch_targets) => {
                            (2 + switch_targets.targets.iter().len()) as u32 * INSTR_BUDGET
                        }
                        TerminatorKind::Unreachable => INSTR_BUDGET,
                        TerminatorKind::Return(_) => 0,
                        TerminatorKind::Goto(..) => INSTR_BUDGET,
                        TerminatorKind::Panic => 2 * INSTR_BUDGET,
                    }
                } as usize
        })
        .sum::<usize>()
        + body.locals.len();
    total_cost.try_into().unwrap_or(100)
}

fn inline_budget_for_body(body: &Body<'_>) -> u32 {
    let mut total_budget = 40;
    if body.block_info.blocks().len() < 4 {
        total_budget += 10;
    }
    total_budget
}
