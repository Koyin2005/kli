use crate::{
    mir::{
        AggregateKind, AssertKind, BasicBlock, BasicBlockId, Body, BodySource, ConstantValue,
        Context, LocalKind, Operand, Place, PlaceProjection, Rvalue, Stmt, Terminator,
    },
    typed_ast::FieldId,
    types::{self, DisplayGenericArgs},
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
                LocalKind::Temp => write!(self.output, " temp {}", local.0),
                LocalKind::Env => write!(self.output, " env"),
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
                    PlaceProjection::DowncastSome => {
                        format!("({} as Some)", output)
                    }
                    PlaceProjection::DerefAs(ty) => {
                        format!("({}:{})^", output, ty)
                    }
                    PlaceProjection::Field(field) => {
                        output.push_str(&format!(".{}", field.into_usize()));
                        output
                    }
                    PlaceProjection::ConstantIndex(index) => {
                        output.push_str(".[");
                        output.push_str(&format!("{}", index));
                        output.push(']');
                        output
                    }
                    PlaceProjection::Deref => {
                        output.push('^');
                        output
                    }
                    PlaceProjection::Index(index) => {
                        output.push_str(".[");
                        output.push_str(&format!("_{}", index.0));
                        output.push(']');
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
            Rvalue::Allocate { size, count } => {
                write!(self.output, "allocate(")?;
                self.write_operand(size)?;
                write!(self.output, ",")?;
                self.write_operand(count)?;
                write!(self.output, ")")?;
            }
            Rvalue::Aggregate(kind, fields) => match kind {
                AggregateKind::ArrayList(ty) => {
                    let mut first = true;
                    write!(self.output, "arraylist[{}]{{", ty)?;
                    let name = |i| match FieldId::new(i) {
                        types::LIST_PTR_FIELD => "ptr",
                        types::LIST_CAPICITY_FIELD => "cap",
                        types::LIST_LEN_FIELD => "len",
                        _ => unreachable!("Should only have 3 fields"),
                    };
                    for (i, operand) in fields.iter().enumerate() {
                        if !first {
                            write!(self.output, ",")?;
                        }
                        write!(self.output, "{} = ", name(i))?;
                        self.write_operand(operand)?;
                        first = false;
                    }
                    write!(self.output, "}}")?;
                }
                AggregateKind::Option { inner, is_some } => {
                    if *is_some {
                        let field = &fields[FieldId::zero()];
                        write!(self.output, "Some[{}]{{", inner)?;
                        self.write_operand(field)?;
                        write!(self.output, "}}")?;
                    } else {
                        write!(self.output, "None[{}]{{}}", inner)?;
                    }
                }
                AggregateKind::Record { field_names } => {
                    let mut first = true;
                    write!(self.output, "{{")?;
                    for (name, operand) in field_names.iter().zip(fields) {
                        if !first {
                            write!(self.output, ",")?;
                        }
                        write!(self.output, "{} = ", name)?;
                        self.write_operand(operand)?;
                        first = false;
                    }
                    write!(self.output, "}}")?;
                }
                AggregateKind::Closure => {
                    let mut first = true;
                    write!(self.output, "Closure {{")?;
                    let name = |i: usize| if i == 0 { "env" } else { "code" };
                    for (i, operand) in fields.iter_enumerated() {
                        if !first {
                            write!(self.output, ",")?;
                        }
                        let name = name(i.into_usize());
                        write!(self.output, "{} = ", name)?;
                        self.write_operand(operand)?;
                        first = false;
                    }
                    write!(self.output, "}}")?;
                }
            },
            Rvalue::Call(operand, args) => {
                self.write_operand(operand)?;
                write!(self.output, "(")?;
                let mut first = true;
                for arg in args {
                    if !first {
                        write!(self.output, ",")?;
                    }
                    self.write_operand(arg)?;
                    first = false;
                }
                write!(self.output, ")")?;
            }
            Rvalue::Ref(mutable, place) => {
                write!(self.output, "ref {} ", mutable)?;
                self.write_place(place)?;
            }
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
                ConstantValue::Function(id, ref args) => {
                    write!(self.output, "{}", self.ctxt.function_names[id].content)?;
                    if !args.is_empty() {
                        write!(self.output, "{}", DisplayGenericArgs(args))?;
                    }
                    Ok(())
                }
                ConstantValue::Lambda(id, ref args) => {
                    write!(self.output, "lambda {}", id.into_usize())?;
                    if !args.is_empty() {
                        write!(self.output, "{}", DisplayGenericArgs(args))?;
                    }
                    Ok(())
                }
                ConstantValue::Sizeof(ref ty) => write!(self.output, "sizeof({})", ty),
            },
        }
    }
    fn write_block(&mut self, id: BasicBlockId, block: &BasicBlock) -> std::io::Result<()> {
        writeln!(self.output, " bb{}", id.into_usize())?;
        for stmt in &block.stmts {
            write!(self.output, "  ")?;
            match stmt {
                Stmt::Print(value) => {
                    write!(self.output, "print(")?;
                    if let Some(value) = value {
                        self.write_operand(value)?;
                    }
                    writeln!(self.output, ")")?;
                }
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
            Terminator::Switch(operand, targets) => {
                write!(self.output, "switch ")?;
                self.write_operand(operand)?;
                write!(self.output, " ")?;
                for target in &targets.targets {
                    write!(self.output, "{} -> bb{}, ", target.value, target.target.0)?;
                }
                write!(self.output, "otherwise -> bb{}", targets.otherwise.0)?;
            }
            Terminator::Goto(block) => write!(self.output, "goto bb{}", block.0)?,
            Terminator::Panic => write!(self.output, "panic")?,
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
