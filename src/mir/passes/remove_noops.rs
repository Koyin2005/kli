use crate::mir::{Body, StmtKind};

pub fn remove_noops(body: &mut Body) {
    for block in body.block_info.blocks_mut_dont_dirty() {
        block
            .stmts
            .retain(|_, stmt| !matches!(stmt.kind, StmtKind::Noop));
    }
}
