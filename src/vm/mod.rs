use std::cell::Cell;

use crate::{
    define_id,
    index_vec::IndexVec,
    mir::{
        AggregateKind, BasicBlockId, BinaryOp, Body, BodySource, ConstValue, Constant, Context,
        Local, Operand, Place, Rvalue, Stmt, StmtId, StmtKind, SwitchTarget, Terminator,
        TerminatorKind,
    },
};
#[derive(Debug)]
enum RuntimeError {
    ArithmeticOverflow,
}
enum MachineStatus {
    Continue,
    Exit,
}
pub enum Object {
    String(String),
    Tuple(Box<[Value]>),
}
define_id!(ObjectId);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Char(char),
    Function(BodySource),
    String(ObjectId),
    Tuple(ObjectId),
    Variant(u32, ObjectId),
    Unit,
}
impl Value {
    #[track_caller]
    pub fn expect_int(self) -> i64 {
        let Value::Int(value) = self else {
            panic!("should be an int value but got {:?}", self)
        };
        value
    }
}
struct GcHeap {
    objects: IndexVec<ObjectId, Object>,
}
impl GcHeap {
    fn new() -> Self {
        Self {
            objects: IndexVec::new(),
        }
    }
    fn data(&self, object: ObjectId) -> &Object {
        &self.objects[object]
    }
    fn data_mut(&mut self, object: ObjectId) -> &mut Object {
        &mut self.objects[object]
    }
    fn alloc_object(&mut self, o: Object) -> ObjectId {
        let id = self.objects.len().try_into().expect("too many objects");
        self.objects.push(o);
        ObjectId(id)
    }
}
pub(super) struct StackFrame<'ctxt, 'mir> {
    body: &'mir Body<'ctxt>,
    stmt: Cell<StmtId>,
    block: Cell<BasicBlockId>,
    locals: IndexVec<Local, Value>,
    return_local: Local,
}
impl StackFrame<'_, '_> {
    pub fn store_local(&mut self, local: Local, value: Value) {
        self.locals[local] = value;
    }
}
pub struct Machine<'ctxt, 'mir> {
    frames: Vec<StackFrame<'ctxt, 'mir>>,
    mir_ctxt: &'mir Context<'ctxt>,
    unit_object: ObjectId,
    heap: GcHeap,
    stdout: String,
    stderr: String,
}
impl<'ctxt, 'mir> Machine<'ctxt, 'mir> {
    pub fn new(mir_ctxt: &'mir Context<'ctxt>) -> Self {
        let mut heap = GcHeap::new();
        Self {
            frames: Vec::new(),
            mir_ctxt,
            stderr: String::new(),
            stdout: String::new(),
            unit_object: heap.alloc_object(Object::Tuple(Box::new([]))),
            heap,
        }
    }
    pub fn alloc_string(&mut self, s: String) -> ObjectId {
        self.heap.alloc_object(Object::String(s))
    }
    pub fn alloc_tuple(&mut self, values: impl IntoIterator<Item = Value>) -> ObjectId {
        let values: Box<[_]> = values.into_iter().collect();
        if values.is_empty() {
            return self.unit_object;
        }
        self.heap.alloc_object(Object::Tuple(values))
    }
    fn push_frame(
        &mut self,
        return_local: Local,
        body: &'mir Body<'ctxt>,
    ) -> &mut StackFrame<'ctxt, 'mir> {
        self.frames.push_mut(StackFrame {
            return_local,
            locals: IndexVec::from_value(body.locals.len(), Value::Int(0)),
            body,
            block: Cell::new(BasicBlockId::ENTRY),
            stmt: Cell::new(StmtId::new(0)),
        })
    }
    fn pop_frame(&mut self) -> Option<StackFrame<'ctxt, 'mir>> {
        self.frames.pop()
    }
    fn load_place(&mut self, place: &Place) -> Value {
        self.current_frame().locals[place.local]
    }
    fn eval_constant(&mut self, constant: &Constant) -> Value {
        match constant.value {
            ConstValue::ZeroSized => Value::Unit,
            ConstValue::Function(def_id, ..) => Value::Function(BodySource::Function(def_id)),
            ConstValue::Int(value) => Value::Int(value),
            ConstValue::Bool(value) => Value::Bool(value),
            ConstValue::Char(value) => Value::Char(value),
            ConstValue::String(symbol) => Value::String(self.alloc_string(symbol.to_string())),
        }
    }
    fn eval_operand(&mut self, operand: &Operand) -> Value {
        match operand {
            Operand::Load(place) => self.load_place(place),
            Operand::Constant(constant) => self.eval_constant(constant),
        }
    }
    fn call(&mut self, return_local: Local, callee: Value, arguments: Vec<Value>) {
        let Value::Function(src) = callee else {
            panic!("should be a function")
        };
        let body = self.mir_ctxt.expect_body(src);
        let frame = self.push_frame(return_local, body);
        for (local, value) in body.params_iter().into_iter().zip(arguments) {
            frame.store_local(local, value);
        }
    }
    fn eval_rvalue(&mut self, local: Local, rvalue: &Rvalue) -> Result<(), RuntimeError> {
        match rvalue {
            Rvalue::ReadLine => {
                let mut output = String::new();
                let _ = std::io::stdin().read_line(&mut output);
                let s = self.alloc_string(output);
                self.store_local(local, Value::String(s));
            }
            Rvalue::UninitZeroed(_) => todo!(),
            Rvalue::Aggregate(aggregate_kind, fields) => {
                let fields = fields
                    .into_iter()
                    .map(|field| self.eval_operand(field))
                    .collect::<Vec<_>>();
                let value = match aggregate_kind {
                    AggregateKind::Tuple => Value::Tuple(self.alloc_tuple(fields)),
                    AggregateKind::NamedRecord(..) => Value::Tuple(self.alloc_tuple(fields)),
                    AggregateKind::Variant(_, case_id, _) => {
                        Value::Variant(case_id.into_usize() as u32, self.alloc_tuple(fields))
                    }
                };
                self.store_local(local, value);
            }
            Rvalue::AllocateRawArray { .. } => todo!(),
            Rvalue::AllocateArray(..) => todo!(),
            Rvalue::AllocateBox(..) => todo!(),
            Rvalue::Use(operand) => {
                let value = self.eval_operand(operand);
                self.store_local(local, value);
            }
            Rvalue::Call(operand, operands) => {
                let callee = self.eval_operand(operand);
                let arguments = operands
                    .iter()
                    .map(|operand| self.eval_operand(operand))
                    .collect::<Vec<_>>();
                self.call(local, callee, arguments);
            }
            Rvalue::Binary(op, operands) => {
                let (left, right) = operands.as_ref();
                let left_value = self.eval_operand(left);
                let right_value = self.eval_operand(right);
                let result_value = match op {
                    BinaryOp::Add => Value::Int(
                        left_value
                            .expect_int()
                            .checked_add(right_value.expect_int())
                            .ok_or(RuntimeError::ArithmeticOverflow)?,
                    ),
                    BinaryOp::Subtract => Value::Int(
                        left_value
                            .expect_int()
                            .checked_sub(right_value.expect_int())
                            .ok_or(RuntimeError::ArithmeticOverflow)?,
                    ),
                    BinaryOp::Multiply => Value::Int(
                        left_value
                            .expect_int()
                            .checked_mul(right_value.expect_int())
                            .ok_or(RuntimeError::ArithmeticOverflow)?,
                    ),
                    BinaryOp::Divide => Value::Int(
                        left_value
                            .expect_int()
                            .checked_div(right_value.expect_int())
                            .ok_or(RuntimeError::ArithmeticOverflow)?,
                    ),
                    BinaryOp::Greater => {
                        Value::Bool(left_value.expect_int() > right_value.expect_int())
                    }
                    BinaryOp::Equals => Value::Bool(left_value == right_value),
                    BinaryOp::Lesser => {
                        Value::Bool(left_value.expect_int() < right_value.expect_int())
                    }
                    BinaryOp::BitwiseAnd => match (left_value, right_value) {
                        (Value::Int(left), Value::Int(right)) => Value::Int(left & right),
                        (Value::Bool(left), Value::Bool(right)) => Value::Bool(left & right),
                        _ => panic!("Cannot perform anding {left_value:?} and {right_value:?}"),
                    },
                    BinaryOp::BitwiseOr => match (left_value, right_value) {
                        (Value::Int(left), Value::Int(right)) => Value::Int(left | right),
                        (Value::Bool(left), Value::Bool(right)) => Value::Bool(left | right),
                        _ => panic!("Cannot perform anding {left_value:?} and {right_value:?}"),
                    },
                    BinaryOp::ShiftLeft => {
                        Value::Int(left_value.expect_int() << right_value.expect_int())
                    }
                    BinaryOp::ShiftRight => {
                        Value::Int(left_value.expect_int() >> right_value.expect_int())
                    }
                    BinaryOp::Offset => todo!("get rid of me"),
                };
                self.store_local(local, result_value);
            }
            Rvalue::AddrOf(_) => todo!("get rid of me"),
            Rvalue::Cast(..) => todo!(),
            Rvalue::Len(_) => todo!(),
            Rvalue::Discriminant(place) => {
                let Value::Variant(discrim, _) = self.load_place(place) else {
                    panic!("Should be a variant value")
                };
                self.store_local(local, Value::Int(discrim.into()));
            }
            Rvalue::LoadIndex(..) => todo!(),
            Rvalue::LoadField(place, field_id) => {
                let Value::Tuple(tuple) = self.load_place(place) else {
                    panic!("Should be a tuple value")
                };
                let Object::Tuple(tuple) = self.heap.data(tuple) else {
                    panic!("Should be a tuple")
                };
                self.store_local(local, tuple[field_id.into_usize()]);
            }
            Rvalue::LoadPayload(place, _) => {
                let Value::Variant(_, object) = self.load_place(place) else {
                    panic!("Should be a variant value")
                };
                self.store_local(local, Value::Tuple(object));
            }
            Rvalue::Unbox(_) => todo!("Unbox"),
            Rvalue::GcAlloc(..) => todo!("Get rid of me"),
        }
        Ok(())
    }
    fn store_local(&mut self, local: Local, value: Value) {
        self.current_frame_mut().store_local(local, value);
    }
    fn eval_stmt(&mut self, stmt: &Stmt) -> Result<(), RuntimeError> {
        {
            let stmt = &self.current_frame().stmt;
            stmt.set(stmt.get().next());
        }
        match &stmt.kind {
            StmtKind::Noop => (),
            StmtKind::AssignBox(..) => todo!(),
            StmtKind::AssignField(place, field_id, operand) => {
                let Value::Tuple(tuple) = self.load_place(place) else {
                    panic!("Should be a tuple")
                };
                let value = self.eval_operand(operand);
                let Object::Tuple(fields) = self.heap.data_mut(tuple) else {
                    panic!("Should be a tuple")
                };
                fields[field_id.into_usize()] = value;
            }
            StmtKind::AssignIndex(..) => todo!(),
            StmtKind::Assign(place, rvalue) => {
                self.eval_rvalue(place.local, rvalue)?;
            }
            StmtKind::Print { value, err } => {
                let value = self.eval_operand(value);
                let Value::String(s) = value else {
                    panic!("Expected a string got {value:?}")
                };
                let Object::String(s) = &self.heap.data(s) else {
                    panic!("Expected a string")
                };
                let (output_buf, out) = if *err {
                    (
                        &mut self.stderr,
                        &mut std::io::stdout() as &mut dyn std::io::Write,
                    )
                } else {
                    (
                        &mut self.stdout,
                        &mut std::io::stderr() as &mut dyn std::io::Write,
                    )
                };
                let original = s.as_str();
                if !original.contains('\n') {
                    output_buf.push_str(original);
                } else {
                    for s in original.split('\n') {
                        if s.is_empty() && output_buf.is_empty() {
                            continue;
                        }
                        output_buf.push_str(s);
                        let _ = writeln!(out, "{}", output_buf);
                        output_buf.clear();
                    }
                }
            }
            StmtKind::Copy { .. } => todo!(),
        }
        Ok(())
    }
    fn eval_terminator(&mut self, term: &Terminator) -> MachineStatus {
        match term.kind {
            TerminatorKind::Goto(new_block) => {
                let frame = self.current_frame_mut();
                frame.block.set(new_block);
                frame.stmt.set(StmtId::new(0));
            }
            TerminatorKind::Return(ref value) => {
                let return_value = self.eval_operand(value);
                let stack_frame = self.pop_frame();
                if self.frames.is_empty() {
                    return MachineStatus::Exit;
                }
                let return_local = if let Some(old_frame) = stack_frame {
                    old_frame.return_local
                } else {
                    unreachable!("can never have no frames")
                };

                let frame = self.current_frame_mut();
                frame.store_local(return_local, return_value);
            }
            TerminatorKind::Assert(..) => todo!(),
            TerminatorKind::Switch(ref operand, ref switch_targets) => {
                let scalar_value: i128 = match self.eval_operand(operand) {
                    Value::Int(value) => value.into(),
                    Value::Bool(value) => value.into(),
                    Value::Char(c) => u32::from(c).into(),
                    value => panic!("Can not get the value of {:?}", value),
                };
                let frame = self.current_frame_mut();
                for &SwitchTarget { value, target } in switch_targets.targets.iter() {
                    if value == scalar_value {
                        frame.block.set(target);
                        frame.stmt.set(StmtId::new(0));
                        return MachineStatus::Continue;
                    }
                }
                frame.block.set(switch_targets.otherwise);
                frame.stmt.set(StmtId::new(0));
            }
            TerminatorKind::Unreachable => todo!(),
            TerminatorKind::Panic => todo!(),
        }
        MachineStatus::Continue
    }
    fn current_frame(&self) -> &StackFrame<'ctxt, 'mir> {
        self.frames.last().unwrap()
    }
    fn current_frame_mut(&mut self) -> &mut StackFrame<'ctxt, 'mir> {
        self.frames.last_mut().unwrap()
    }
    pub fn run(mut self, entry_point: &'mir Body<'ctxt>) {
        self.push_frame(Local::new(0), entry_point);
        let err = loop {
            let frame = self.current_frame();
            let block = frame.block.get();
            let stmt = frame.stmt.get();
            let block = &frame.body.block_info.blocks()[block];
            if let Some(stmt) = block.stmts.get(stmt) {
                match self.eval_stmt(stmt) {
                    Ok(()) => (),
                    Err(err) => break err,
                }
            } else if let MachineStatus::Exit = self.eval_terminator(block.expect_terminator()) {
                return;
            }
        };
        match err {
            RuntimeError::ArithmeticOverflow => {
                eprintln!("failed due to arithmetic overflow")
            }
        }
    }
}
