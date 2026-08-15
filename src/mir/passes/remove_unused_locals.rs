use std::collections::HashSet;

use crate::{
    index_vec::IndexVec,
    mir::{
        Local, LocalKind,
        passes::{MirPass, optimisation_enabled},
        visitor::{MutVisit, PlaceCtxt, Visit},
    },
};

pub struct RemoveUnusedLocals;
impl MirPass<'_> for RemoveUnusedLocals {
    fn name(&self) -> &'static str {
        "remove-unused-locals"
    }
    fn run(&self, ctxt: crate::CtxtRef<'_>, body: &mut crate::mir::Body) {
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
                if finder.locals.contains(&local) {
                    let new_local = next_local;

                    if local != new_local {
                        body.locals.swap(local, next_local);
                    }
                    next_local = next_local.next();
                    Some(new_local)
                } else {
                    None
                }
            })
            .collect::<IndexVec<Local, _>>();
        LocalReplacer { locals: &local_map }.visit_body(body);

        body.locals.truncate(next_local);
        if super::should_dump(ctxt, body.src) {
            println!("{:?}", local_map);
            println!("{:?}", body.locals.indices().collect::<Vec<_>>());
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
    fn visit_local(&mut self, _: PlaceCtxt, _: crate::mir::Location, local: Local) {
        self.locals.insert(local);
    }
}

struct LocalReplacer<'r> {
    locals: &'r IndexVec<Local, Option<Local>>,
}
impl<'r, 'ctxt> MutVisit<'ctxt> for LocalReplacer<'r> {
    fn visit_local(&mut self, _: crate::mir::Location, local: &mut Local) {
        *local = self.locals[*local].unwrap();
    }
}
