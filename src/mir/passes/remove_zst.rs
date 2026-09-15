use crate::{
    CtxtRef,
    layout::{Layout, calculate_layout},
    mir::{
        Locals, Location,
        Operation::{self, Load},
        Regs, StmtKind,
        passes::BodyPass,
        visitor::MutVisit,
    },
    types::Type,
};

pub struct RemoveZst;
impl RemoveZst {
    fn is_zst<'ctxt>(ty: Type<'ctxt>, ctxt: CtxtRef<'ctxt>) -> bool {
        calculate_layout(ctxt, ty)
            .as_ref()
            .is_ok_and(Layout::is_align_1_zst)
    }
}
impl<'ctxt> BodyPass<'ctxt> for RemoveZst {
    fn name(&self) -> &'static str {
        "remove-zst"
    }
    fn run(&self, ctxt: crate::CtxtRef<'ctxt>, body: &mut crate::mir::Body<'ctxt>) {
        struct RemoveZstVisit<'ctxt, 'a>(CtxtRef<'ctxt>, &'a Locals<'ctxt>, &'a Regs<'ctxt>);
        impl<'ctxt> MutVisit<'ctxt> for RemoveZstVisit<'ctxt, '_> {
            fn visit_stmt(&mut self, loc: Location, stmt: &mut crate::mir::Stmt<'ctxt>) {
                match &mut stmt.kind {
                    StmtKind::Store(place, _) => {
                        if RemoveZst::is_zst(place.type_of(self.0, self.1, self.2), self.0) {
                            stmt.kind = StmtKind::Noop;
                            return;
                        }
                    }
                    StmtKind::Assign(_, operation) => {
                        if let Load(place) = operation
                            && let ty = place.type_of(self.0, self.1, self.2)
                            && RemoveZst::is_zst(ty, self.0)
                        {
                            *operation = Operation::Zeroed(ty);
                            return;
                        }
                    }
                    _ => (),
                }
                self.super_visit_stmt(loc, stmt);
            }
        }
        let mut visit = RemoveZstVisit(ctxt, &body.locals, &body.registers);
        for (id, block) in body
            .block_info
            .blocks_mut_dont_dirty()
            .iter_mut_enumerated()
        {
            visit.visit_block(id, block);
        }
    }
}
