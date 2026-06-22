use crate::mir::{
    BasicBlock, BasicBlockId, Body, Constant, Local, Operand, Place, PlaceBase, PlaceProjection,
    Rvalue, Stmt, Terminator,
};

pub trait Visit {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        super_visit_stmt(self, stmt);
    }
    fn visit_operand(&mut self, operand: &Operand) {
        super_visit_operand(self, operand);
    }
    fn visit_local(&mut self, local: Local) {
        super_visit_local(self, local);
    }
    fn visit_place(&mut self, place: &Place) {
        super_visit_place(self, place);
    }
    fn visit_projection(&mut self, projection: PlaceProjection) {
        super_visit_projection(self, projection);
    }
    fn visit_constant(&mut self, constant: &Constant) {
        super_visit_constant(self, constant);
    }
    fn visit_rvalue(&mut self, rvalue: &Rvalue) {
        super_visit_rvalue(self, rvalue);
    }
    fn visit_terminator(&mut self, terminator: &Terminator) {
        super_visit_terminator(self, terminator);
    }
    fn visit_block(&mut self, id: BasicBlockId, block: &BasicBlock) {
        super_visit_block(self, id, block)
    }
    fn visit_body(&mut self, body: &Body) {
        for (id, block) in body.blocks.iter_enumerated() {
            self.visit_block(id, block);
        }
    }
}
pub fn super_visit_terminator<V: Visit + ?Sized>(v: &mut V, terminator: &Terminator) {
    match terminator {
        Terminator::Goto(_) | Terminator::Panic | Terminator::Return | Terminator::Unreachable => {
            ()
        }
        Terminator::Switch(operand, _) => v.visit_operand(operand),
    }
}
pub fn super_visit_block<V: Visit + ?Sized>(v: &mut V, _: BasicBlockId, info: &BasicBlock) {
    for stmt in info.stmts.iter() {
        v.visit_stmt(stmt);
    }
    v.visit_terminator(info.expect_terminator());
}
pub fn super_visit_rvalue<V: Visit + ?Sized>(v: &mut V, rvalue: &Rvalue) {
    match rvalue {
        Rvalue::Len(place) => v.visit_place(place),
        Rvalue::Use(operand) => v.visit_operand(operand),
        Rvalue::Aggregate(_, fields) => {
            for field in fields {
                v.visit_operand(field);
            }
        }
        Rvalue::Call(operand, operands) => {
            v.visit_operand(operand);
            for operand in operands {
                v.visit_operand(operand);
            }
        }
        Rvalue::Binary(_, operands) => {
            let (left, right) = operands.as_ref();
            v.visit_operand(left);
            v.visit_operand(right);
        }
        Rvalue::Ref(_, place) => {
            v.visit_place(place);
        }
        Rvalue::Allocate { ty: _, count } => {
            v.visit_operand(count);
        }
        Rvalue::PointerCast(_, operand) => {
            v.visit_operand(operand);
        }
        Rvalue::DecodeUtf8(operand1, operand2) => {
            v.visit_operand(operand1);
            v.visit_operand(operand2);
        }
    }
}
pub fn super_visit_projection<V: Visit + ?Sized>(v: &mut V, projection: PlaceProjection) {
    match projection {
        PlaceProjection::ConstantIndex(_)
        | PlaceProjection::DowncastSome
        | PlaceProjection::Field(_) => todo!(),
        PlaceProjection::Index(local) => v.visit_local(local),
        PlaceProjection::Deref => (),
    }
}
pub fn super_visit_local<V: Visit + ?Sized>(_v: &mut V, _local: Local) {}
pub fn super_visit_place<V: Visit + ?Sized>(v: &mut V, place: &Place) {
    match place.base {
        PlaceBase::Local(local) => v.visit_local(local),
        _ => (),
    }
    for projection in place.projections.iter() {
        v.visit_projection(*projection);
    }
}
pub fn super_visit_constant<V: Visit + ?Sized>(_v: &mut V, _constant: &Constant) {}
pub fn super_visit_operand<V: Visit + ?Sized>(v: &mut V, operand: &Operand) {
    match operand {
        Operand::Load(place) => v.visit_place(place),
        Operand::Constant(constant) => v.visit_constant(constant),
    }
}
pub fn super_visit_stmt<V: Visit + ?Sized>(v: &mut V, stmt: &Stmt) {
    match stmt {
        Stmt::Noop => (),
        Stmt::Assert(operand, _) | Stmt::Deallocate(operand) => v.visit_operand(operand),
        Stmt::Assign(place, rvalue) => {
            v.visit_place(place);
            v.visit_rvalue(rvalue);
        }
        Stmt::Print(operand) => {
            if let Some(operand) = operand {
                v.visit_operand(operand);
            }
        }
    }
}
