use crate::{
    collect::CtxtRef,
    index_vec::IndexVec,
    mir::{
        AssertKind, BasicBlock, BasicBlockId, BinaryOp, Body, BodySource, Context, Local,
        LocalInfo, LocalKind, Locals, Operand, Place, Rvalue, Stmt, StmtKind, SwitchTarget,
        SwitchTargets, Terminator, TerminatorKind, basic_blocks::BasicBlocks,
    },
    resolved_ast::Var,
    src_loc::SrcLoc,
    typed_ast::FieldId,
    types::{CaseId, Type},
};
mod expr;
mod function;
mod loops;
mod matches;
mod stmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldProjection {
    Field(FieldId),
    CaseDowncast(CaseId),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceBuilder {
    pub place: Place,
    pub projections: Vec<FieldProjection>,
}
impl PlaceBuilder {
    pub fn new(place: Place) -> Self {
        Self {
            place,
            projections: Vec::new(),
        }
    }
    pub fn push_field(&mut self, field: FieldId) {
        self.projections.push(FieldProjection::Field(field));
    }
    pub fn with_field(mut self, field: FieldId) -> Self {
        self.push_field(field);
        self
    }
    pub fn push_case_downcast(&mut self, case: CaseId) {
        self.projections.push(FieldProjection::CaseDowncast(case));
    }
    pub fn with_case_downcast(mut self, case: CaseId) -> Self {
        self.push_case_downcast(case);
        self
    }
}
#[derive(Clone, Debug)]
pub struct TargetPlace<'ctxt> {
    pub place: Place,
    pub kind: TargetPlaceKind<'ctxt>,
}
#[derive(Clone, Debug)]
pub enum TargetPlaceKind<'ctxt> {
    Index(Operand<'ctxt>),
    Base,
}
impl<'a> TargetPlace<'a> {
    pub fn new(place: Place) -> Self {
        Self {
            place,
            kind: TargetPlaceKind::Base,
        }
    }
    pub fn with_index(place: Place, index: Operand<'a>) -> Self {
        Self {
            place,
            kind: TargetPlaceKind::Index(index),
        }
    }
}
pub struct Builder<'mir, 'ctxt> {
    pub mir_context: &'mir mut Context<'ctxt>,
    body: Body<'ctxt>,
    current_block: BasicBlockId,
    pub ctxt: CtxtRef<'ctxt>,
}
impl<'mir, 'ctxt> Builder<'mir, 'ctxt> {
    pub fn new(
        mir_context: &'mir mut Context<'ctxt>,
        source: BodySource,
        return_type: Type<'ctxt>,
        ctxt: CtxtRef<'ctxt>,
    ) -> Self {
        Self {
            mir_context,
            body: Body {
                src: source,
                locals: Locals::default(),
                block_info: BasicBlocks::new(IndexVec::from_value(1, BasicBlock::default())),
                return_type,
            },
            current_block: BasicBlockId::ENTRY,
            ctxt,
        }
    }
    pub(super) fn new_local(&mut self, ty: Type<'ctxt>, kind: LocalKind) -> Local {
        self.body.locals.push(LocalInfo { ty, kind })
    }
    pub(super) fn new_local_from_info(&mut self, info: LocalInfo<'ctxt>) -> Local {
        self.body.locals.push(info)
    }
    pub(super) fn finish_assert_to_new_block(
        &mut self,
        loc: SrcLoc,
        operand: Operand<'ctxt>,
        assert_kind: AssertKind,
    ) {
        let new_block = self.new_block();
        self.finish_block(loc, TerminatorKind::Assert(operand, assert_kind, new_block));
        self.switch_to_block(new_block);
    }
    pub(super) fn new_temp(&mut self, ty: Type<'ctxt>) -> Local {
        self.new_local_from_info(LocalInfo {
            ty,
            kind: super::LocalKind::Temp,
        })
    }
    pub(super) fn new_var(&mut self, var: Var, ty: Type<'ctxt>) -> Local {
        self.new_local_from_info(LocalInfo {
            ty,
            kind: super::LocalKind::Var(var),
        })
    }
    pub(super) fn new_block(&mut self) -> BasicBlockId {
        self.body
            .block_info
            .blocks_mut()
            .push(BasicBlock::default())
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
    pub(super) fn finish_block(&mut self, loc: SrcLoc, terminator: TerminatorKind<'ctxt>) {
        self.body.block_info.blocks_mut()[self.current_block].terminator = Some(Terminator {
            src_info: loc,
            kind: terminator,
        });
    }
    pub(super) fn finish_block_with_switch_targets(
        &mut self,
        loc: SrcLoc,
        operand: Operand<'ctxt>,
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
        operand: Operand<'ctxt>,
        targets: SwitchTargets,
    ) {
        self.finish_block(loc, TerminatorKind::Switch(operand, targets));
    }
    pub(super) fn finish_block_with_if(
        &mut self,
        loc: SrcLoc,
        operand: Operand<'ctxt>,
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
    pub(super) fn push_stmt(&mut self, loc: SrcLoc, kind: StmtKind<'ctxt>) {
        self.body.block_info.blocks_mut()[self.current_block]
            .stmts
            .push(Stmt { loc, kind });
    }
    pub(super) fn assign_to_temp(
        &mut self,
        loc: SrcLoc,
        ty: Type<'ctxt>,
        value: Rvalue<'ctxt>,
    ) -> Local {
        let temp = self.new_temp(ty);
        self.assign(loc, Place::local(temp), value);
        temp
    }
    pub(super) fn assign_equals(
        &mut self,
        loc: SrcLoc,
        left: Operand<'ctxt>,
        right: Operand<'ctxt>,
    ) -> Local {
        self.assign_binary_result(
            loc,
            Type::new_bool(self.ctxt),
            BinaryOp::Equals,
            left,
            right,
        )
    }
    pub(super) fn assign_binary_result(
        &mut self,
        loc: SrcLoc,
        ty: Type<'ctxt>,
        op: BinaryOp,
        left: Operand<'ctxt>,
        right: Operand<'ctxt>,
    ) -> Local {
        self.assign_to_temp(loc, ty, Rvalue::Binary(op, Box::new((left, right))))
    }
    pub(super) fn panic(&mut self, loc: SrcLoc) {
        let block = self.new_block();
        self.finish_block(loc, TerminatorKind::Panic);
        self.switch_to_block(block);
    }
    pub(super) fn assign(&mut self, loc: SrcLoc, place: Place, value: Rvalue<'ctxt>) {
        self.push_stmt(loc, StmtKind::Assign(place, Box::new(value)));
    }
}
