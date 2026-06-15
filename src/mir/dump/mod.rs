use crate::mir::{
    AssertKind, BasicBlock, BasicBlockId, Body, BodySource, ConstantValue, Context, LocalKind,
    Operand, Place, PlaceBase, PlaceProjection, Rvalue, Stmt, Terminator,
};

pub struct MirDump<'ctxt> {
    output: Box<dyn std::io::Write>,
    ctxt: &'ctxt Context,
}
impl<'ctxt> MirDump<'ctxt> {
    pub fn new(output: impl std::io::Write + 'static, ctxt: &'ctxt Context) -> Self {
        Self {
            output: Box::new(output),
            ctxt,
        }
    }
    fn write_header(&mut self, body: &Body) -> std::io::Result<()> {
        match body.src {
            BodySource::Function(f) => {
                write!(self.output, "fun {}", self.ctxt.function_names[f].content)?;
            }
            BodySource::Lambda(lambda) => {
                write!(self.output, "lambda {}", lambda.into_usize())?;
            }
        }
        writeln!(self.output, "() -> {}", body.return_type)?;
        for (local, info) in body.locals.iter_enumerated() {
            write!(self.output, " {:?}", local)?;
            match &info.kind {
                LocalKind::Param(var) => write!(self.output, " param {}", var.0),
                LocalKind::Var(var) => write!(self.output, " var {}", var.0),
                LocalKind::DropFlag(_) => todo!("Handle drop flag"),
                LocalKind::Temp => write!(self.output, " temp {}", local.0),
            }?;
            writeln!(self.output, " : {}", info.ty)?;
        }
        Ok(())
    }
    fn write_place(&mut self, place: &Place) -> std::io::Result<()> {
        if !place.projections.is_empty() {
            let mut output = format!("{}", place.base);
            for projection in place.projections.iter() {
                output = match projection {
                    PlaceProjection::Field(field) => {
                        output.push_str(&format!(".{}", field.into_usize()));
                        output
                    }
                };
            }
            write!(self.output, "{}", output)?;
            return Ok(());
        }
        write!(self.output, "{}", place.base)
    }
    fn write_rvalue(&mut self, rvalue: &Rvalue) -> std::io::Result<()> {
        match rvalue {
            Rvalue::Use(operand) => {
                self.write_operand(operand)?;
            }
            Rvalue::Binary(op, operands) => {
                let (left, right) = &**operands;
                write!(self.output, "{:?}(", op)?;
                self.write_operand(left)?;
                write!(self.output, ",")?;
                self.write_operand(right)?;
                write!(self.output, ")")?;
            }
            Rvalue::Aggregate(..) => todo!("Aggregate"),
            Rvalue::Call(..) => todo!("Call"),
            Rvalue::Ref(..) => todo!("Ref"),
        }
        Ok(())
    }
    fn write_operand(&mut self, operand: &Operand) -> std::io::Result<()> {
        match operand {
            Operand::Load(place) => {
                write!(self.output, "load ")?;
                self.write_place(place)
            }
            Operand::Constant(constant) => match constant.value {
                ConstantValue::Int(value) => write!(self.output, "{}", value),
                ConstantValue::Bool(value) => write!(self.output, "{}", value),
                ConstantValue::ZeroSized => write!(self.output, "{}", constant.ty),
            },
        }?;
        Ok(())
    }
    fn write_block(&mut self, id: BasicBlockId, block: &BasicBlock) -> std::io::Result<()> {
        writeln!(self.output, " bb{}", id.into_usize())?;
        for stmt in &block.stmts {
            write!(self.output, "  ")?;
            match stmt {
                Stmt::Noop => writeln!(self.output, "noop")?,
                Stmt::Assign(place, value) => {
                    self.write_place(place)?;
                    write!(self.output, " = ")?;
                    self.write_rvalue(value)?;
                    writeln!(self.output)?;
                }
                Stmt::Assert(operand, kind) => {
                    write!(self.output, "assert(!")?;
                    self.write_operand(operand)?;
                    write!(self.output, ", ")?;
                    match kind {
                        AssertKind::Overflow(op) => {
                            write!(self.output, "\"Overflow in computing {op:?}\"")?
                        }
                        AssertKind::DivideOverflow => {
                            write!(self.output, "\"Overflow in computing division\"")?
                        }
                        AssertKind::DivideByZero => write!(self.output, "\"Divide by zero\"")?,
                    }
                    writeln!(self.output, ")")?
                }
            }
        }
        write!(self.output, "  ")?;
        match block.expect_terminator() {
            Terminator::Unreachable => {
                write!(self.output, "unreachable")?;
            }
            Terminator::Return => {
                write!(self.output, "return")?;
            }
            Terminator::Switch(..) => todo!("Switch"),
        }
        writeln!(self.output)
    }
    pub fn write_body(mut self, body: &Body) -> std::io::Result<()> {
        self.write_header(body)?;
        for (id, block) in body.blocks.iter_enumerated() {
            self.write_block(id, block)?;
        }
        writeln!(self.output, "end\n")?;
        Ok(())
    }
}
