use std::collections::HashSet;

use crate::mir::{
    Local, LocalKind, PlaceBase, StmtKind,
    passes::{MirPass, optimisation_enabled},
    visitor::{MutVisit, PlaceCtxt, Visit},
};

pub struct DeadStoreElim;
impl MirPass for DeadStoreElim {
    fn name(&self) -> &'static str {
        "dead-store-elim"
    }
    fn run(&self, _: crate::CtxtRef<'_>, body: &mut crate::mir::Body) {
        let mut finder = LocalFinder {
            locals: HashSet::from_iter(body.locals.iter_enumerated().filter_map(
                |(local, info)| {
                    if matches!(info.kind, LocalKind::Param(..)) {
                        Some(local)
                    } else {
                        None
                    }
                },
            )),
        };
        finder.visit_body(body);
        LocalReplacer {
            locals: &finder.locals,
        }
        .visit_body(body);
    }
    fn enabled(&self, ctxt: crate::CtxtRef<'_>) -> bool {
        optimisation_enabled(ctxt)
    }
}

struct LocalFinder {
    locals: HashSet<Local>,
}
impl Visit for LocalFinder {
    fn visit_local(&mut self, ctxt: PlaceCtxt, _: crate::mir::Location, local: Local) {
        if let PlaceCtxt::Read = ctxt {
            self.locals.insert(local);
        }
    }
}

struct LocalReplacer<'a> {
    locals: &'a HashSet<Local>,
}
impl MutVisit for LocalReplacer<'_> {
    fn visit_stmt(&mut self, loc: crate::mir::Location, stmt: &mut crate::mir::Stmt) {
        if let StmtKind::Assign(place, rvalue) = &mut stmt.kind
            && let PlaceBase::Local(local) = place.base
            && place.projections.is_empty()
            && rvalue.can_remove_if_unused()
            && !self.locals.contains(&local)
        {
            stmt.kind = StmtKind::Noop;
        }
        self.super_visit_stmt(loc, stmt);
    }
}
