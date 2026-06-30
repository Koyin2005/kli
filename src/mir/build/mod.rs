use crate::{
    collect::CtxtRef,
    index_vec::IndexVec,
    mir::{
        AssertKind, BasicBlock, BasicBlockId, BinaryOp, Body, BodySource, Context, Local,
        LocalInfo, LocalKind, Locals, Operand, Place, Rvalue, Stmt, StmtKind, SwitchTarget,
        SwitchTargets, Terminator, TerminatorKind,
    },
    resolved_ast::Var,
    src_loc::SrcLoc,
    types::Type,
};
mod expr;
mod function;
mod loops;
mod matches;
mod stmt;
pub struct Builder<'ctxt> {
    pub mir_context: &'ctxt mut Context,
    body: Body,
    current_block: BasicBlockId,
    pub ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> Builder<'ctxt> {
    pub fn new(
        mir_context: &'ctxt mut Context,
        source: BodySource,
        return_type: Type,
        captures: Option<super::Captures>,
        ctxt: CtxtRef<'ctxt>,
    ) -> Self {
        Self {
            mir_context,
            body: Body {
                capture_info: captures,
                src: source,
                locals: Locals::default(),
                blocks: IndexVec::from_iter([BasicBlock::default()]),
                return_type,
            },
            current_block: BasicBlockId::ENTRY,
            ctxt,
        }
    }
    pub(super) fn new_local(&mut self, ty: Type, kind: LocalKind) -> Local {
        self.body.locals.push(LocalInfo { ty, kind })
    }
    pub(super) fn new_local_from_info(&mut self, info: LocalInfo) -> Local {
        self.body.locals.push(info)
    }
    pub(super) fn assert(&mut self, loc: SrcLoc, operand: Operand, assert_kind: AssertKind) {
        self.push_stmt(loc, StmtKind::Assert(operand, assert_kind));
    }
    pub(super) fn new_temp(&mut self, ty: Type) -> Local {
        self.new_local_from_info(LocalInfo {
            ty,
            kind: super::LocalKind::Temp,
        })
    }
    pub(super) fn new_var(&mut self, var: Var, ty: Type) -> Local {
        self.new_local_from_info(LocalInfo {
            ty,
            kind: super::LocalKind::Var(var),
        })
    }
    pub(super) fn new_block(&mut self) -> BasicBlockId {
        self.body.blocks.push(BasicBlock::default())
    }
    pub(super) fn switch_to_block(&mut self, block: BasicBlockId) {
        self.current_block = block;
    }
    /// Returns the new block and switches to it
    pub(super) fn switch_to_new_block(&mut self) -> BasicBlockId {
        let block = self.new_block();
        self.current_block = block;
        block
    }
    /// Returns the new block while terminating the old block with a goto to the new block
    pub(super) fn goto_to_new_block(&mut self, loc: SrcLoc) -> BasicBlockId {
        let block = self.new_block();
        self.finish_block(loc, TerminatorKind::Goto(block));
        self.current_block = block;
        block
    }
    pub(super) fn finish_block(&mut self, loc: SrcLoc, terminator: TerminatorKind) {
        self.body.blocks[self.current_block].terminator = Some(Terminator {
            src_info: loc,
            kind: terminator,
        });
    }
    pub(super) fn finish_block_with_switch_targets(
        &mut self,
        loc: SrcLoc,
        operand: Operand,
        targets: Vec<SwitchTarget>,
        otherwise: BasicBlockId,
    ) {
        self.finish_block(
            loc,
            TerminatorKind::Switch(operand, SwitchTargets { targets, otherwise }),
        );
    }
    pub(super) fn finish_block_with_switch(
        &mut self,
        loc: SrcLoc,
        operand: Operand,
        targets: SwitchTargets,
    ) {
        self.finish_block(loc, TerminatorKind::Switch(operand, targets));
    }
    pub(super) fn finish_block_with_if(
        &mut self,
        loc: SrcLoc,
        operand: Operand,
        true_block: BasicBlockId,
        false_block: BasicBlockId,
    ) {
        self.finish_block_with_switch(
            loc,
            operand,
            SwitchTargets {
                targets: vec![SwitchTarget {
                    value: 0,
                    target: false_block,
                }],
                otherwise: true_block,
            },
        );
    }
    pub(super) fn finish_block_with_goto(&mut self, loc: SrcLoc, block: BasicBlockId) {
        self.finish_block(loc, TerminatorKind::Goto(block));
    }
    pub(super) fn push_stmt(&mut self, loc: SrcLoc, kind: StmtKind) {
        self.body.blocks[self.current_block]
            .stmts
            .push(Stmt { loc, kind });
    }
    pub(super) fn assign_to_temp(&mut self, loc: SrcLoc, ty: Type, value: Rvalue) -> Local {
        let temp = self.new_temp(ty);
        self.assign(loc, Place::local(temp), value);
        temp
    }
    pub(super) fn assign_equals(&mut self, loc: SrcLoc, left: Operand, right: Operand) -> Local {
        self.assign_binary_result(loc, Type::Bool, BinaryOp::Equals, left, right)
    }
    pub(super) fn assign_binary_result(
        &mut self,
        loc: SrcLoc,
        ty: Type,
        op: BinaryOp,
        left: Operand,
        right: Operand,
    ) -> Local {
        self.assign_to_temp(loc, ty, Rvalue::Binary(op, Box::new((left, right))))
    }
    pub(super) fn panic(&mut self, loc: SrcLoc) {
        let block = self.new_block();
        self.finish_block(loc, TerminatorKind::Panic);
        self.switch_to_block(block);
    }
    pub(super) fn assign(&mut self, loc: SrcLoc, place: Place, value: Rvalue) {
        self.push_stmt(loc, StmtKind::Assign(place, Box::new(value)));
    }
}
