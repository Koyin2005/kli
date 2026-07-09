use std::collections::HashSet;

use crate::{
    index_vec::IndexVec,
    mir::{
        Local, LocalKind, PlaceBase, StmtKind,
        passes::MirPass,
        visitor::{MutVisit, PlaceCtxt, Visit},
    },
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

        let mut next_local = Local::new(0);
        let local_map = body
            .locals
            .indices()
            .map(|local| {
                let new_local = next_local.next();
                if finder.locals.contains(&local) {
                    next_local = new_local;
                    Some(new_local)
                } else {
                    None
                }
            })
            .collect::<IndexVec<Local, _>>();
        LocalReplacer { locals: &local_map }.visit_body(body);
        body.locals.retain(|local, _| local_map[local].is_some());
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
    locals: &'a IndexVec<Local, Option<Local>>,
}
impl MutVisit for LocalReplacer<'_> {
    fn visit_local(&mut self, _: crate::mir::Location, local: &mut Local) {
        if let Some(new_local) = self.locals[*local] {
            *local = new_local;
        }
    }
    fn visit_stmt(&mut self, loc: crate::mir::Location, stmt: &mut crate::mir::Stmt) {
        if let StmtKind::Assign(place, rvalue) = &mut stmt.kind
            && let PlaceBase::Local(local) = place.base
            && place.projections.is_empty()
            && rvalue.can_remove_if_unused()
            && self.locals[local].is_none()
        {
            stmt.kind = StmtKind::Noop;
        }
        self.super_visit_stmt(loc, stmt);
    }
}
