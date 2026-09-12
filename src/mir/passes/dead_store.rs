use std::collections::HashSet;

use crate::mir::{
    Local, PlaceBase, StmtKind,
    passes::{BodyPass, optimisation_enabled, remove_noops::remove_noops},
    visitor::{MutVisit, PlaceCtxt, Visit},
};

pub struct DeadStoreElim;
impl BodyPass<'_> for DeadStoreElim {
    fn name(&self) -> &'static str {
        "dead-store-elim"
    }
    fn run(&self, _: crate::CtxtRef<'_>, body: &mut crate::mir::Body) {
        let mut finder = LocalFinder {
            locals: HashSet::from_iter(body.locals.indices().filter_map(|local| {
                if local.0 < body.param_count {
                    Some(local)
                } else {
                    None
                }
            })),
        };
        finder.visit_body(body);
        let mut replacer = LocalReplacer {
            locals: &finder.locals,
            changed: false,
        };
        replacer.visit_body(body);
        if replacer.changed {
            remove_noops(body);
        }
    }
    fn enabled(&self, ctxt: crate::CtxtRef<'_>) -> bool {
        optimisation_enabled(ctxt)
    }
}

struct LocalFinder {
    locals: HashSet<Local>,
}
impl Visit<'_> for LocalFinder {
    fn visit_local(&mut self, ctxt: PlaceCtxt, _: crate::mir::Location, local: Local) {
        if let PlaceCtxt::Read = ctxt {
            self.locals.insert(local);
        }
    }
}

struct LocalReplacer<'a> {
    locals: &'a HashSet<Local>,
    changed: bool,
}
impl<'ctxt> MutVisit<'ctxt> for LocalReplacer<'_> {
    fn visit_stmt(&mut self, loc: crate::mir::Location, stmt: &mut crate::mir::Stmt) {
        if let StmtKind::Assign(place, rvalue) = &mut stmt.kind
            && let PlaceBase::Local(local) = place.base
            && place.projections.is_empty()
            && rvalue.can_remove_if_unused()
            && !self.locals.contains(&local)
        {
            stmt.kind = StmtKind::Noop;
            self.changed = true;
        }
        self.super_visit_stmt(loc, stmt);
    }
}
