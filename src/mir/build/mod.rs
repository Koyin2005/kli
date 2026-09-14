use std::collections::HashMap;

use crate::{
    collect::CtxtRef,
    index_vec::IndexVec,
    mir::{
        AssertKind, BasicBlock, BasicBlockId, Body, BodySource, Context, Local,
        LocalInfo, Operand, Operation, Place, Reg, RegInfo, Regs, Rvalue, Stmt, StmtKind,
        SwitchTarget, SwitchTargets, Terminator, TerminatorKind, Value, basic_blocks::BasicBlocks,
    },
    resolved_ast::{Var, VarId},
    src_loc::SrcLoc,
    types::Type,
};
mod expr;
mod function;
mod loops;
mod matches;
mod stmt;
pub(super) enum VarKind<'ctxt> {
    Local(Local),
    Value(Value<'ctxt>),
}
pub struct Builder<'mir, 'ctxt> {
    pub mir_context: &'mir mut Context<'ctxt>,
    body: Body<'ctxt>,
    current_block: BasicBlockId,
    pub ctxt: CtxtRef<'ctxt>,
    variables: HashMap<VarId, VarKind<'ctxt>>,
}
impl<'mir, 'ctxt> Builder<'mir, 'ctxt> {
    pub fn new(
        mir_context: &'mir mut Context<'ctxt>,
        source: BodySource,
        return_type: Type<'ctxt>,
        params: impl IntoIterator<Item = (Var, Type<'ctxt>)>,
        ctxt: CtxtRef<'ctxt>,
    ) -> Self {
        let mut variables = HashMap::new();
        let registers = params
            .into_iter()
            .enumerate()
            .map(|(i, (var, ty))| {
                variables.insert(
                    var.1,
                    VarKind::Value(Value::Reg(Reg(i.try_into().expect("too many variables")))),
                );
                RegInfo { ty }
            })
            .collect::<Regs>();
        let param_count = registers.len().try_into().expect("too many params");
        Self {
            mir_context,
            body: Body {
                src: source,
                param_count,
                locals: IndexVec::new(),
                block_info: BasicBlocks::new(IndexVec::from_value(1, BasicBlock::default())),
                return_type,
                registers,
            },
            variables,
            current_block: BasicBlockId::ENTRY,
            ctxt,
        }
    }
    pub(super) fn declare_var(&mut self, var: VarId, kind: VarKind<'ctxt>) {
        self.variables.insert(var, kind);
    }
    pub(super) fn resolve_var(&mut self, var: VarId) -> Option<&VarKind<'ctxt>> {
        self.variables.get(&var)
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
        self.finish_block(
            loc,
            TerminatorKind::OldAssert(operand, assert_kind, new_block),
        );
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
    pub(super) fn new_block_with_args<const N: usize>(
        &mut self,
        args: [Type<'ctxt>; N],
    ) -> (BasicBlockId, [Reg; N]) {
        let mut block = BasicBlock::default();
        let args = args.map(|arg| {
            let reg = self.body.registers.push(RegInfo { ty: arg });
            block.args.push(reg);
            reg
        });
        (self.body.block_info.blocks_mut().push(block), args)
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
        self.finish_block(loc, TerminatorKind::Goto(block, Vec::new()));
        self.current_block = block;
        block
    }
    pub(super) fn finish_block(&mut self, loc: SrcLoc, terminator: TerminatorKind<'ctxt>) {
        self.body.block_info.blocks_mut()[self.current_block].terminator = Some(Terminator {
            src_info: loc,
            kind: terminator,
        });
    }
    pub(super) fn finish_block_with_old_switch_targets(
        &mut self,
        loc: SrcLoc,
        operand: Operand<'ctxt>,
        targets: Vec<SwitchTarget>,
        otherwise: BasicBlockId,
    ) {
        self.finish_block(
            loc,
            TerminatorKind::OldSwitch(operand, SwitchTargets { targets, otherwise }),
        );
    }
    pub(super) fn finish_block_with_switch(
        &mut self,
        loc: SrcLoc,
        operand: Value<'ctxt>,
        targets: SwitchTargets,
    ) {
        self.finish_block(loc, TerminatorKind::Switch(operand, targets));
    }
    pub(super) fn finish_block_with_if(
        &mut self,
        loc: SrcLoc,
        value: Value<'ctxt>,
        true_block: BasicBlockId,
        false_block: BasicBlockId,
    ) {
        self.finish_block_with_switch(
            loc,
            value,
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
        self.finish_block(loc, TerminatorKind::Goto(block, Vec::new()));
    }
    pub(super) fn push_stmt(&mut self, loc: SrcLoc, kind: StmtKind<'ctxt>) {
        self.body.block_info.blocks_mut()[self.current_block]
            .stmts
            .push(Stmt { loc, kind });
    }
    pub(super) fn push_operation(&mut self, loc: SrcLoc, operation: Operation<'ctxt>) -> Reg {
        let reg = self.body.registers.push(RegInfo {
            ty: operation.result_type(self.ctxt, &self.body.registers, &self.body.locals),
        });
        self.push_stmt(loc, StmtKind::Assign(reg, operation));
        reg
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
    pub(super) fn panic(&mut self, loc: SrcLoc) {
        let block = self.new_block();
        self.finish_block(loc, TerminatorKind::Panic);
        self.switch_to_block(block);
    }
    pub(super) fn assign(&mut self, loc: SrcLoc, place: Place, value: Rvalue<'ctxt>) {
        self.push_stmt(loc, StmtKind::OldStore(place, Box::new(value)));
    }
}
