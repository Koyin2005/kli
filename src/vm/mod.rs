use std::cell::Cell;

use crate::mir::{BasicBlockId, Body, Stmt, StmtId, StmtKind, Terminator};

pub enum Value {
    Int(i64),
    Bool(bool),
    Char(char),
}

pub(super) struct StackFrame<'ctxt,'mir>{
    body : &'mir Body<'ctxt>,
    stmt : Cell<StmtId>,
    block : Cell<BasicBlockId>,

}
pub struct Machine<'ctxt,'mir>{
    frames : Vec<StackFrame<'ctxt,'mir>>
}
impl<'ctxt,'mir> Machine<'ctxt,'mir>{
    pub fn new() -> Self{
        Self { frames: Vec::new() }
    }
    fn push_frame(&mut self, body: &'mir Body<'ctxt>){
        self.frames.push(StackFrame { body ,block:Cell::new(BasicBlockId::ENTRY), stmt:Cell::new(StmtId::new(0))});
    }
    fn eval_stmt(&mut self, stmt : &Stmt){
        match &stmt.kind{
            StmtKind::Noop => (),
            StmtKind::AssignBox(place, operand) => todo!(),
            StmtKind::AssignField(place, field_id, operand) => todo!(),
            StmtKind::AssignIndex(place, operand, operand1) => todo!(),
            StmtKind::Assign(place, rvalue) => todo!(),
            StmtKind::Print { value, err } => todo!(),
            StmtKind::Copy { dst, src, count } => todo!(),
        }
    }
    fn eval_terminator(&mut self, term: &Terminator){

    }
    fn current_frame(&self) -> &StackFrame<'ctxt,'mir>{
        self.frames.last().unwrap()
    }
    pub fn run(mut self, entry_point : &'mir Body<'ctxt>){
        self.push_frame(entry_point);
        loop {
            let frame = self.current_frame();
            let block = frame.block.get();
            let stmt = frame.stmt.get();
            let block = &frame.body.block_info.blocks()[block];
            if let Some(stmt) = block.stmts.get(stmt){
                self.eval_stmt(stmt);
            }
            else {
                self.eval_terminator(block.expect_terminator());
                
            }
        }

    }
}