use crate::{
    index_vec::IndexVec,
    mir::{
        AssertKind, BasicBlock, BasicBlockId, Body, BodySource, Context, Local, LocalInfo,
        LocalKind, Locals, Operand, Place, Stmt, Terminator,
    },
    resolved_ast::Var,
    types::Type,
};
mod expr;
mod function;
pub struct Builder<'ctxt> {
    pub context: &'ctxt mut Context,
    body: Body,
    current_block: BasicBlockId,
}
impl<'ctxt> Builder<'ctxt> {
    pub fn new(context: &'ctxt mut Context, source: BodySource, return_type: Type) -> Self {
        Self {
            context,
            body: Body {
                src: source,
                locals: Locals::default(),
                blocks: IndexVec::from_iter([BasicBlock::default()]),
                return_type,
            },
            current_block: BasicBlockId::zero(),
        }
    }
    pub fn locals(&mut self) -> &mut Locals {
        &mut self.body.locals
    }
    pub fn new_local(&mut self, ty: Type, kind: LocalKind) -> Local {
        self.body.locals.push(LocalInfo { ty, kind })
    }
    pub fn new_local_from_info(&mut self, info: LocalInfo) -> Local {
        self.body.locals.push(info)
    }
    pub fn assert(&mut self, operand: Operand, assert_kind: AssertKind) {
        self.push_stmt(Stmt::Assert(operand, assert_kind));
    }
    pub fn new_temp(&mut self, ty: Type) -> Local {
        self.new_local_from_info(LocalInfo {
            ty,
            kind: super::LocalKind::Temp,
        })
    }
    pub fn new_var(&mut self, var: Var, ty: Type) -> Local {
        self.new_local_from_info(LocalInfo {
            ty,
            kind: super::LocalKind::Var(var),
        })
    }
    pub fn new_drop_flag(&mut self, place: Place, ty: Type) -> Local {
        self.new_local_from_info(LocalInfo {
            ty,
            kind: super::LocalKind::DropFlag(place),
        })
    }
    pub fn new_block(&mut self) -> BasicBlockId {
        self.body.blocks.push(BasicBlock::default())
    }
    pub fn switch_to_block(&mut self, block: BasicBlockId) {
        self.current_block = block;
    }
    pub fn finish_block(&mut self, terminator: Terminator) {
        self.body.blocks[self.current_block].terminator = Some(terminator);
    }
    pub fn push_stmt(&mut self, stmt: Stmt) {
        self.body.blocks[self.current_block].stmts.push(stmt);
    }
    pub fn finish(self) -> Body {
        self.body
    }
}
