use crate::mir::{
    BasicBlock, BasicBlockId, Body, Constant, CopyNonOverlapping, DropInPlace, Local, Location,
    Operand, Place, PlaceBase, PlaceProjection, Rvalue, Stmt, StmtKind, Terminator, TerminatorKind,
};

pub trait Visit {
    fn super_visit_stmt(&mut self, loc: Location, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Noop => (),
            StmtKind::Assert(operand, _) | StmtKind::Deallocate(operand) => {
                self.visit_operand(loc, operand)
            }
            StmtKind::Assign(place, rvalue) => {
                self.visit_place(loc, place);
                self.visit_rvalue(loc, rvalue);
            }
            StmtKind::Print(operand) => {
                if let Some(operand) = operand {
                    self.visit_operand(loc, operand);
                }
            }
            StmtKind::CopyNonOverlapping(copy) => {
                let CopyNonOverlapping { dst, src, count } = copy.as_ref();
                self.visit_operand(loc, dst);
                self.visit_operand(loc, src);
                self.visit_operand(loc, count);
            }
            StmtKind::DropInPlace(drop) => {
                let DropInPlace { pointer_to_place } = drop.as_ref();
                self.visit_operand(loc, pointer_to_place);
            }
        }
    }
    fn super_visit_constant(&mut self, _loc: Location, _constant: &Constant) {}
    fn super_visit_terminator(&mut self, loc: Location, terminator: &Terminator) {
        match &terminator.kind {
            TerminatorKind::Goto(_)
            | TerminatorKind::Panic
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => (),
            TerminatorKind::Switch(operand, _) => self.visit_operand(loc, operand),
        }
    }
    fn super_visit_block(&mut self, id: BasicBlockId, info: &BasicBlock) {
        for (stmt_id, stmt) in info.stmts.iter_enumerated() {
            self.visit_stmt(
                Location {
                    block: id,
                    stmt: Some(stmt_id),
                },
                stmt,
            );
        }
        self.visit_terminator(
            Location {
                block: id,
                stmt: None,
            },
            info.expect_terminator(),
        );
    }
    fn super_visit_rvalue(&mut self, loc: Location, rvalue: &Rvalue) {
        match rvalue {
            Rvalue::DanglingPtr(_) => (),
            Rvalue::Discriminant(place) => self.visit_place(loc, place),
            Rvalue::Len(place) => self.visit_place(loc, place),
            Rvalue::Use(operand) => self.visit_operand(loc, operand),
            Rvalue::Aggregate(_, fields) => {
                for field in fields {
                    self.visit_operand(loc, field);
                }
            }
            Rvalue::Call(operand, operands) => {
                self.visit_operand(loc, operand);
                for operand in operands {
                    self.visit_operand(loc, operand);
                }
            }
            Rvalue::Binary(_, operands) => {
                let (left, right) = operands.as_ref();
                self.visit_operand(loc, left);
                self.visit_operand(loc, right);
            }
            Rvalue::Ref(_, _, place) | Rvalue::RawPtrTo(place) => {
                self.visit_place(loc, place);
            }
            Rvalue::Allocate { ty: _, count } => {
                self.visit_operand(loc, count);
            }
            Rvalue::Cast(_, operand) => {
                self.visit_operand(loc, operand);
            }
            Rvalue::DecodeUtf8(operand1, operand2) => {
                self.visit_operand(loc, operand1);
                self.visit_operand(loc, operand2);
            }
        }
    }
    fn super_visit_projection(&mut self, loc: Location, projection: PlaceProjection) {
        match projection {
            PlaceProjection::ConstantIndex(_) | PlaceProjection::Field(_) => (),
            PlaceProjection::Index(local) => self.visit_local(loc, local),
            PlaceProjection::Deref | PlaceProjection::CaseDowncast(..) => (),
        }
    }
    fn super_visit_local(&mut self, _loc: Location, _local: Local) {}
    fn super_visit_place(&mut self, loc: Location, place: &Place) {
        if let PlaceBase::Local(local) = place.base {
            self.visit_local(loc, local);
        }
        for projection in place.projections.iter() {
            self.visit_projection(loc, *projection);
        }
    }
    fn super_visit_operand(&mut self, loc: Location, operand: &Operand) {
        match operand {
            Operand::Load(place) => self.visit_place(loc, place),
            Operand::Constant(constant) => self.visit_constant(loc, constant),
        }
    }

    fn visit_stmt(&mut self, loc: Location, stmt: &Stmt) {
        self.super_visit_stmt(loc, stmt);
    }
    fn visit_operand(&mut self, loc: Location, operand: &Operand) {
        self.super_visit_operand(loc, operand);
    }
    fn visit_local(&mut self, loc: Location, local: Local) {
        self.super_visit_local(loc, local);
    }
    fn visit_place(&mut self, loc: Location, place: &Place) {
        self.super_visit_place(loc, place);
    }
    fn visit_projection(&mut self, loc: Location, projection: PlaceProjection) {
        self.super_visit_projection(loc, projection);
    }
    fn visit_constant(&mut self, loc: Location, constant: &Constant) {
        self.super_visit_constant(loc, constant);
    }
    fn visit_rvalue(&mut self, loc: Location, rvalue: &Rvalue) {
        self.super_visit_rvalue(loc, rvalue);
    }
    fn visit_terminator(&mut self, loc: Location, terminator: &Terminator) {
        self.super_visit_terminator(loc, terminator);
    }
    fn visit_block(&mut self, id: BasicBlockId, block: &BasicBlock) {
        self.super_visit_block(id, block)
    }
    fn visit_body(&mut self, body: &Body) {
        for (id, block) in body.blocks.iter_enumerated() {
            self.visit_block(id, block);
        }
    }
}

pub trait MutVisit {
    fn super_visit_stmt(&mut self, loc: Location, stmt: &mut Stmt) {
        match &mut stmt.kind {
            StmtKind::Noop => (),
            StmtKind::Assert(operand, _) | StmtKind::Deallocate(operand) => {
                self.visit_operand(loc, operand)
            }
            StmtKind::Assign(place, rvalue) => {
                self.visit_place(loc, place);
                self.visit_rvalue(loc, rvalue);
            }
            StmtKind::Print(operand) => {
                if let Some(operand) = operand {
                    self.visit_operand(loc, operand);
                }
            }
            StmtKind::CopyNonOverlapping(copy) => {
                let CopyNonOverlapping { dst, src, count } = copy.as_mut();
                self.visit_operand(loc, dst);
                self.visit_operand(loc, src);
                self.visit_operand(loc, count);
            }
            StmtKind::DropInPlace(drop) => {
                let DropInPlace { pointer_to_place } = drop.as_mut();
                self.visit_operand(loc, pointer_to_place);
            }
        }
    }
    fn super_visit_constant(&mut self, _loc: Location, _constant: &mut Constant) {}
    fn super_visit_terminator(&mut self, loc: Location, terminator: &mut Terminator) {
        match &mut terminator.kind {
            TerminatorKind::Goto(_)
            | TerminatorKind::Panic
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => (),
            TerminatorKind::Switch(operand, _) => self.visit_operand(loc, operand),
        }
    }
    fn super_visit_block(&mut self, id: BasicBlockId, info: &mut BasicBlock) {
        for (stmt_id, stmt) in info.stmts.iter_mut_enumerated() {
            self.visit_stmt(
                Location {
                    block: id,
                    stmt: Some(stmt_id),
                },
                stmt,
            );
        }
        self.visit_terminator(
            Location {
                block: id,
                stmt: None,
            },
            info.expect_terminator_mut(),
        );
    }
    fn super_visit_rvalue(&mut self, loc: Location, rvalue: &mut Rvalue) {
        match rvalue {
            Rvalue::DanglingPtr(_) => (),
            Rvalue::Discriminant(place) => self.visit_place(loc, place),
            Rvalue::Len(place) => self.visit_place(loc, place),
            Rvalue::Use(operand) => self.visit_operand(loc, operand),
            Rvalue::Aggregate(_, fields) => {
                for field in fields {
                    self.visit_operand(loc, field);
                }
            }
            Rvalue::Call(operand, operands) => {
                self.visit_operand(loc, operand);
                for operand in operands {
                    self.visit_operand(loc, operand);
                }
            }
            Rvalue::Binary(_, operands) => {
                let (left, right) = operands.as_mut();
                self.visit_operand(loc, left);
                self.visit_operand(loc, right);
            }
            Rvalue::Ref(_, _, place) | Rvalue::RawPtrTo(place) => {
                self.visit_place(loc, place);
            }
            Rvalue::Allocate { ty: _, count } => {
                self.visit_operand(loc, count);
            }
            Rvalue::Cast(_, operand) => {
                self.visit_operand(loc, operand);
            }
            Rvalue::DecodeUtf8(operand1, operand2) => {
                self.visit_operand(loc, operand1);
                self.visit_operand(loc, operand2);
            }
        }
    }
    fn super_visit_projection(&mut self, loc: Location, projection: PlaceProjection) {
        match projection {
            PlaceProjection::ConstantIndex(_) | PlaceProjection::Field(_) => (),
            PlaceProjection::Index(local) => self.visit_local(loc, local),
            PlaceProjection::Deref | PlaceProjection::CaseDowncast(..) => (),
        }
    }
    fn super_visit_local(&mut self, _loc: Location, _local: Local) {}
    fn super_visit_place(&mut self, loc: Location, place: &Place) {
        if let PlaceBase::Local(local) = place.base {
            self.visit_local(loc, local);
        }
        for projection in place.projections.iter() {
            self.visit_projection(loc, *projection);
        }
    }
    fn super_visit_operand(&mut self, loc: Location, operand: &mut Operand) {
        match operand {
            Operand::Load(place) => self.visit_place(loc, place),
            Operand::Constant(constant) => self.visit_constant(loc, constant),
        }
    }

    fn visit_stmt(&mut self, loc: Location, stmt: &mut Stmt) {
        self.super_visit_stmt(loc, stmt);
    }
    fn visit_operand(&mut self, loc: Location, operand: &mut Operand) {
        self.super_visit_operand(loc, operand);
    }
    fn visit_local(&mut self, loc: Location, local: Local) {
        self.super_visit_local(loc, local);
    }
    fn visit_place(&mut self, loc: Location, place: &mut Place) {
        self.super_visit_place(loc, place);
    }
    fn visit_projection(&mut self, loc: Location, projection: PlaceProjection) {
        self.super_visit_projection(loc, projection);
    }
    fn visit_constant(&mut self, loc: Location, constant: &mut Constant) {
        self.super_visit_constant(loc, constant);
    }
    fn visit_rvalue(&mut self, loc: Location, rvalue: &mut Rvalue) {
        self.super_visit_rvalue(loc, rvalue);
    }
    fn visit_terminator(&mut self, loc: Location, terminator: &mut Terminator) {
        self.super_visit_terminator(loc, terminator);
    }
    fn visit_block(&mut self, id: BasicBlockId, block: &mut BasicBlock) {
        self.super_visit_block(id, block)
    }
    fn visit_body(&mut self, body: &mut Body) {
        for (id, block) in body.blocks.iter_mut_enumerated() {
            self.visit_block(id, block);
        }
    }
}
