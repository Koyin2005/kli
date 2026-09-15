use crate::{
    CtxtRef,
    mir::{
        BasicBlock, BasicBlockId, Body, Constant, Local, Location, Operand, Operation, Place,
        PlaceBase, PlaceProjection, Reg, Rvalue, Stmt, StmtKind, Terminator, TerminatorKind, Value,
    },
};
pub enum PlaceCtxt {
    Read,
    Write,
}
pub trait Visit<'ctxt> {
    #[track_caller]
    fn ctxt(&self) -> CtxtRef<'ctxt> {
        unimplemented!("not implemented")
    }
    fn visit_assign(&mut self, loc: Location, place: &Place<'ctxt>, rvalue: &Rvalue<'ctxt>) {
        self.visit_place(PlaceCtxt::Write, loc, place);
        self.visit_rvalue(loc, rvalue);
    }
    fn visit_reg(&mut self, loc: Location, reg: Reg, ctxt: PlaceCtxt) {
        _ = loc;
        _ = reg;
        _ = ctxt;
    }
    fn super_visit_stmt(&mut self, loc: Location, stmt: &Stmt<'ctxt>) {
        match &stmt.kind {
            StmtKind::Noop => (),
            StmtKind::Store(place, value) => {
                self.visit_place(PlaceCtxt::Write, loc, place);
                self.visit_value(loc, value);
            }
            StmtKind::PanicIf(value) => {
                self.visit_value(loc, value);
            }
            StmtKind::OldStore(place, rvalue) => {
                self.visit_assign(loc, place, rvalue);
            }
            StmtKind::Print { value, .. } => {
                self.visit_value(loc, value);
            }
            StmtKind::Assign(dst, operation) => {
                self.visit_reg(loc, *dst, PlaceCtxt::Write);
                self.visit_operation(loc, operation);
            }
        }
    }
    fn super_visit_constant(&mut self, _loc: Location, _constant: &Constant<'ctxt>) {}
    fn super_visit_terminator(&mut self, loc: Location, terminator: &Terminator<'ctxt>) {
        match &terminator.kind {
            TerminatorKind::Goto(_, args) => {
                for arg in args {
                    self.visit_value(loc, arg);
                }
            }
            TerminatorKind::Panic | TerminatorKind::Unreachable => (),
            TerminatorKind::OldSwitch(operand, _) => {
                self.visit_operand(loc, operand)
            }
            TerminatorKind::Return(value) | TerminatorKind::Switch(value, _) => {
                self.visit_value(loc, value)
            }
        }
    }
    fn super_visit_block(&mut self, id: BasicBlockId, info: &BasicBlock<'ctxt>) {
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
    fn super_visit_rvalue(&mut self, loc: Location, rvalue: &Rvalue<'ctxt>) {
        match rvalue {
            Rvalue::AllocArray(_, elements) => {
                for element in elements {
                    self.visit_operand(loc, element);
                }
            }
            Rvalue::ReadLine => (),
            Rvalue::Discriminant(place) => self.visit_place(PlaceCtxt::Read, loc, place),
            Rvalue::Len(place) => self.visit_place(PlaceCtxt::Read, loc, place),
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
        }
    }
    fn super_visit_projection(&mut self, loc: Location, projection: PlaceProjection) {
        match projection {
            PlaceProjection::ConstantIndex(_) | PlaceProjection::Field(_) => (),
            PlaceProjection::Index(local) => self.visit_local(PlaceCtxt::Read, loc, local),
            PlaceProjection::CaseDowncast(..) => (),
            PlaceProjection::Deref => (),
        }
    }
    fn super_visit_local(&mut self, _: PlaceCtxt, _loc: Location, _local: Local) {}
    fn super_visit_place(&mut self, ctxt: PlaceCtxt, loc: Location, place: &Place<'ctxt>) {
        match &place.base {
            &PlaceBase::Local(local) => {
                self.visit_local(ctxt, loc, local);
            }
            PlaceBase::ArrayElement(array_element) => {
                self.visit_value(loc, &array_element.base);
                self.visit_value(loc, &array_element.index);
            }
        }
        for projection in place.projections.iter() {
            self.visit_projection(loc, *projection);
        }
    }
    fn super_visit_operand(&mut self, loc: Location, operand: &Operand<'ctxt>) {
        match operand {
            Operand::Load(place) => self.visit_place(PlaceCtxt::Read, loc, place),
            Operand::Constant(constant) => self.visit_constant(loc, constant),
        }
    }

    fn visit_stmt(&mut self, loc: Location, stmt: &Stmt<'ctxt>) {
        self.super_visit_stmt(loc, stmt);
    }
    fn visit_operand(&mut self, loc: Location, operand: &Operand<'ctxt>) {
        self.super_visit_operand(loc, operand);
    }
    fn visit_local(&mut self, ctxt: PlaceCtxt, loc: Location, local: Local) {
        self.super_visit_local(ctxt, loc, local);
    }
    fn visit_place(&mut self, ctxt: PlaceCtxt, loc: Location, place: &Place<'ctxt>) {
        self.super_visit_place(ctxt, loc, place);
    }
    fn visit_projection(&mut self, loc: Location, projection: PlaceProjection) {
        self.super_visit_projection(loc, projection);
    }
    fn visit_constant(&mut self, loc: Location, constant: &Constant<'ctxt>) {
        self.super_visit_constant(loc, constant);
    }
    fn visit_rvalue(&mut self, loc: Location, rvalue: &Rvalue<'ctxt>) {
        self.super_visit_rvalue(loc, rvalue);
    }
    fn visit_terminator(&mut self, loc: Location, terminator: &Terminator<'ctxt>) {
        self.super_visit_terminator(loc, terminator);
    }
    fn visit_block(&mut self, id: BasicBlockId, block: &BasicBlock<'ctxt>) {
        self.super_visit_block(id, block)
    }
    fn visit_value(&mut self, loc: Location, value: &Value) {
        match value {
            Value::Reg(reg) => self.visit_reg(loc, *reg, PlaceCtxt::Read),
            Value::Unit
            | Value::Int(_)
            | Value::Unknown(_)
            | Value::Function(..)
            | Value::Bool(_)
            | Value::Char(_)
            | Value::String(_)
            | Value::Lambda(..) => (),
        }
    }
    fn super_visit_operation(&mut self, loc: Location, operation: &Operation<'ctxt>) {
        match operation {
            Operation::ReadLine => (),
            Operation::Len(value) | Operation::Discriminant(value) | Operation::Not(value) => {
                self.visit_value(loc, value);
            }
            Operation::Load(place) => {
                self.visit_place(PlaceCtxt::Read, loc, place);
            }
            Operation::Cmp(_, left, right)
            | Operation::Arith(_, left, right)
            | Operation::ExtractElement(left, right)
            | Operation::InBounds(left, right)
            | Operation::Bitwise(_, left, right) => {
                self.visit_value(loc, left);
                self.visit_value(loc, right);
            }
            Operation::ExtractField(value, _) | Operation::ExtractPayload(value, _) => {
                self.visit_value(loc, value);
            }
            Operation::Call(callee, args) => {
                self.visit_value(loc, callee);
                for arg in args {
                    self.visit_value(loc, arg);
                }
            }
            Operation::Aggregate(_, fields) => {
                for field in fields {
                    self.visit_value(loc, field);
                }
            }
            Operation::AllocArray(_, fields) => {
                for field in fields {
                    self.visit_value(loc, field);
                }
            }
        }
    }
    fn visit_operation(&mut self, loc: Location, operation: &Operation<'ctxt>) {
        self.super_visit_operation(loc, operation);
    }
    fn visit_body(&mut self, body: &Body<'ctxt>) {
        for (id, block) in body.block_info.blocks().iter_enumerated() {
            self.visit_block(id, block);
        }
    }
}

pub trait MutVisit<'ctxt> {
    fn visit_assign(
        &mut self,
        loc: Location,
        place: &mut Place<'ctxt>,
        rvalue: &mut Rvalue<'ctxt>,
    ) {
        self.visit_place(loc, place);
        self.visit_rvalue(loc, rvalue);
    }
    fn visit_value(&mut self, loc: Location, value: &mut Value<'ctxt>) {
        match value {
            Value::Reg(reg) => self.visit_reg(loc, reg),
            Value::Unit
            | Value::Int(_)
            | Value::Unknown(_)
            | Value::Function(..)
            | Value::Bool(_)
            | Value::Char(_)
            | Value::String(_)
            | Value::Lambda(..) => (),
        }
    }
    fn visit_operation(&mut self, loc: Location, operation: &mut Operation<'ctxt>) {
        match operation {
            Operation::ReadLine => (),
            Operation::Len(value) | Operation::Discriminant(value) | Operation::Not(value) => {
                self.visit_value(loc, value);
            }
            Operation::Load(place) => {
                self.visit_place(loc, place);
            }
            Operation::Cmp(_, left, right)
            | Operation::Arith(_, left, right)
            | Operation::ExtractElement(left, right)
            | Operation::InBounds(left, right)
            | Operation::Bitwise(_, left, right) => {
                self.visit_value(loc, left);
                self.visit_value(loc, right);
            }
            Operation::ExtractField(value, _) | Operation::ExtractPayload(value, _) => {
                self.visit_value(loc, value);
            }
            Operation::Call(callee, args) => {
                self.visit_value(loc, callee);
                for arg in args {
                    self.visit_value(loc, arg);
                }
            }
            Operation::Aggregate(_, fields) => {
                for field in fields {
                    self.visit_value(loc, field);
                }
            }
            Operation::AllocArray(_, fields) => {
                for field in fields {
                    self.visit_value(loc, field);
                }
            }
        }
    }
    fn visit_reg(&mut self, loc: Location, reg: &mut Reg) {
        _ = loc;
        _ = reg;
    }
    fn super_visit_stmt(&mut self, loc: Location, stmt: &mut Stmt<'ctxt>) {
        match &mut stmt.kind {
            StmtKind::Noop => (),
            StmtKind::Store(place, value) => {
                self.visit_place(loc, place);
                self.visit_value(loc, value);
            }
            StmtKind::PanicIf(value) => {
                self.visit_value(loc, value);
            }
            StmtKind::OldStore(place, rvalue) => {
                self.visit_assign(loc, place, rvalue);
            }
            StmtKind::Assign(dst, operation) => {
                self.visit_reg(loc, dst);
                self.visit_operation(loc, operation);
            }
            StmtKind::Print { value, err: _ } => {
                self.visit_value(loc, value);
            }
        }
    }
    fn super_visit_constant(&mut self, _loc: Location, _constant: &mut Constant<'ctxt>) {}
    fn super_visit_terminator(&mut self, loc: Location, terminator: &mut Terminator<'ctxt>) {
        match &mut terminator.kind {
            TerminatorKind::Goto(_, args) => {
                for arg in args {
                    self.visit_value(loc, arg);
                }
            }
            TerminatorKind::Panic | TerminatorKind::Unreachable => (),
            TerminatorKind::OldSwitch(operand, _) => {
                self.visit_operand(loc, operand)
            }
            TerminatorKind::Return(value) | TerminatorKind::Switch(value, _) => {
                self.visit_value(loc, value)
            }
        }
    }
    fn super_visit_block(&mut self, id: BasicBlockId, info: &mut BasicBlock<'ctxt>) {
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
    fn super_visit_rvalue(&mut self, loc: Location, rvalue: &mut Rvalue<'ctxt>) {
        match rvalue {
            Rvalue::AllocArray(_, elements) => {
                for element in elements {
                    self.visit_operand(loc, element);
                }
            }
            Rvalue::ReadLine => (),
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
        }
    }
    fn super_visit_projection(&mut self, loc: Location, projection: &mut PlaceProjection) {
        match projection {
            PlaceProjection::ConstantIndex(_) | PlaceProjection::Field(_) => (),
            PlaceProjection::Index(local) => self.visit_local(loc, local),
            PlaceProjection::CaseDowncast(..) => (),
            PlaceProjection::Deref => (),
        }
    }
    fn super_visit_local(&mut self, _loc: Location, _local: &mut Local) {}
    fn super_visit_place(&mut self, loc: Location, place: &mut Place<'ctxt>) {
        match &mut place.base {
            PlaceBase::Local(local) => {
                self.visit_local(loc, local);
            }
            PlaceBase::ArrayElement(array_element) => {
                self.visit_value(loc, &mut array_element.base);
                self.visit_value(loc, &mut array_element.index);
            }
        }
        for projection in place.projections.iter_mut() {
            self.visit_projection(loc, projection);
        }
    }
    fn super_visit_operand(&mut self, loc: Location, operand: &mut Operand<'ctxt>) {
        match operand {
            Operand::Load(place) => self.visit_place(loc, place),
            Operand::Constant(constant) => self.visit_constant(loc, constant),
        }
    }

    fn visit_stmt(&mut self, loc: Location, stmt: &mut Stmt<'ctxt>) {
        self.super_visit_stmt(loc, stmt);
    }
    fn visit_operand(&mut self, loc: Location, operand: &mut Operand<'ctxt>) {
        self.super_visit_operand(loc, operand);
    }
    fn visit_local(&mut self, loc: Location, local: &mut Local) {
        self.super_visit_local(loc, local);
    }
    fn visit_place(&mut self, loc: Location, place: &mut Place<'ctxt>) {
        self.super_visit_place(loc, place);
    }
    fn visit_projection(&mut self, loc: Location, projection: &mut PlaceProjection) {
        self.super_visit_projection(loc, projection);
    }
    fn visit_constant(&mut self, loc: Location, constant: &mut Constant<'ctxt>) {
        self.super_visit_constant(loc, constant);
    }
    fn visit_rvalue(&mut self, loc: Location, rvalue: &mut Rvalue<'ctxt>) {
        self.super_visit_rvalue(loc, rvalue);
    }
    fn visit_terminator(&mut self, loc: Location, terminator: &mut Terminator<'ctxt>) {
        self.super_visit_terminator(loc, terminator);
    }
    fn visit_block(&mut self, id: BasicBlockId, block: &mut BasicBlock<'ctxt>) {
        self.super_visit_block(id, block)
    }
    fn visit_body(&mut self, body: &mut Body<'ctxt>) {
        for (id, block) in body.block_info.blocks_mut().iter_mut_enumerated() {
            self.visit_block(id, block);
        }
    }
    fn visit_body_no_invalidate(&mut self, body: &mut Body<'ctxt>) {
        for (id, block) in body
            .block_info
            .blocks_mut_dont_dirty()
            .iter_mut_enumerated()
        {
            self.visit_block(id, block);
        }
    }
}
