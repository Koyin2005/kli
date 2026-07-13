use std::collections::HashSet;

use crate::{
    index_vec::IndexVec,
    mir::{
        Local, LocalKind,
        passes::{MirPass, optimisation_enabled},
        visitor::{PlaceCtxt, Visit},
    },
};

pub struct RemoveUnusedLocals;
impl MirPass for RemoveUnusedLocals {
    fn name(&self) -> &'static str {
        "remove-unused-locals"
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
        body.locals.retain(|local, _| local_map[local].is_some());
    }
    fn enabled(&self, ctxt: crate::CtxtRef<'_>) -> bool {
        optimisation_enabled(ctxt)
    }
}

struct LocalFinder {
    locals: HashSet<Local>,
}
impl Visit for LocalFinder {
    fn visit_local(&mut self, _: PlaceCtxt, _: crate::mir::Location, local: Local) {
        self.locals.insert(local);
    }
}
