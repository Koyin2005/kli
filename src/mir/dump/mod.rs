use crate::{
    Symbol,
    collect::CtxtRef,
    mir::{
        AggregateKind, AssertKind, BasicBlock, BasicBlockId, Body, BodySource, CastKind,
        ConstantValue, CopyNonOverlapping, DropInPlace, LocalKind, Operand, Place, PlaceProjection,
        Rvalue, StmtKind, TerminatorKind,
    },
    typed_ast::FieldId,
    types::{self, display_generic_args},
};

pub struct MirDump<'ctxt> {
    output: Box<dyn std::io::Write>,
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> MirDump<'ctxt> {
    pub fn new(output: impl std::io::Write + 'static, ctxt: CtxtRef<'ctxt>) -> Self {
        Self {
            output: Box::new(output),
            ctxt,
        }
    }
    fn write_header(&mut self, body: &Body) -> std::io::Result<()> {
        match body.src {
            BodySource::Function(f) => {
                write!(self.output, "fun {}", self.ctxt.display(f))?;
            }
            BodySource::Lambda(lambda) => {
                write!(self.output, "lambda {}", self.ctxt.display(lambda))?;
            }
            BodySource::ClosureShim(lambda) => {
                write!(self.output, "lambda_shim {}", self.ctxt.display(lambda))?;
            }
        }
        writeln!(self.output, "() -> {}", body.return_type)?;
        for (local, info) in body.locals.iter_enumerated() {
            write!(self.output, " {:?}", local)?;
            match &info.kind {
                LocalKind::Param(var) => write!(
                    self.output,
                    " param {}",
                    if let Some(var) = *var {
                        var.0
                    } else {
                        Symbol::EMPTY_STRING
                    }
                ),
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
                    PlaceProjection::CaseDowncast(_, name) => {
                        format!("({} as {})", output, name)
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
            Rvalue::Allocate { ty, count } => {
                write!(self.output, "allocate[{ty}](")?;
                self.write_operand(count)?;
                write!(self.output, ")")?;
            }
            Rvalue::Len(place) => {
                write!(self.output, "len(")?;
                self.write_place(place)?;
                write!(self.output, ")")?;
            }
            Rvalue::Discriminant(place) => {
                write!(self.output, "discriminant(")?;
                self.write_place(place)?;
                write!(self.output, ")")?;
            }
            Rvalue::Aggregate(kind, fields) => {
                let name = match kind {
                    AggregateKind::Array(..) | AggregateKind::Record { .. } => "".to_string(),
                    AggregateKind::Closure(params, return_type) => {
                        let mut first = true;
                        let mut output = "Closure((".to_string();
                        for param in params {
                            if !first {
                                output.push(',');
                            }
                            output.push_str(&param.to_string());
                            first = false;
                        }
                        output.push_str(") -> ");
                        output.push_str(&return_type.to_string());
                        output.push(')');
                        output
                    }
                    AggregateKind::ArrayList(ty) => format!("array_list[{}]", ty),
                    AggregateKind::String => "string".to_string(),
                    AggregateKind::Variant(id, index, args) => {
                        let name = self.ctxt.type_def(*id).case(*index).name;
                        format!("{}{}", name, display_generic_args(args))
                    }
                    AggregateKind::NamedRecord(id, args) => {
                        let name = self.ctxt.type_def(*id).name;
                        format!("{}{}", name, display_generic_args(args))
                    }
                };
                let (open_bracket, close_bracket) = match kind {
                    AggregateKind::Array(_, _) => ('[', ']'),
                    AggregateKind::Variant(..) => ('(', ')'),
                    _ => ('{', '}'),
                };
                let ctxt = self.ctxt;
                let field_name = move |i: FieldId| match kind {
                    AggregateKind::ArrayList(_) | AggregateKind::String => Some(
                        match i {
                            types::LIST_PTR_FIELD => "ptr",
                            types::LIST_CAPICITY_FIELD => "cap",
                            types::LIST_LEN_FIELD => "len",
                            _ => unreachable!("Should only have 3 fields"),
                        }
                        .to_string(),
                    ),
                    AggregateKind::Array(..) => None,
                    AggregateKind::Record { field_names } => Some(field_names[i].to_string()),
                    AggregateKind::Closure(..) => Some(match i {
                        i if i == FieldId::FIRST_FIELD => "env".to_string(),
                        i if i == FieldId::new(1) => "code".to_string(),
                        _ => unreachable!("Should only have 2 fields"),
                    }),
                    AggregateKind::Variant(_, _, _) => Some(match i {
                        FieldId::FIRST_FIELD => "0".to_string(),
                        _ => unreachable!("Should only have one field"),
                    }),
                    AggregateKind::NamedRecord(id, ..) => {
                        Some(ctxt.type_def(*id).fields()[i].name.to_string())
                    }
                };
                write!(self.output, "{name}{open_bracket}")?;
                let mut first = true;
                for (i, operand) in fields.iter_enumerated() {
                    if !first {
                        write!(self.output, ", ")?;
                    }
                    if let Some(name) = field_name(i) {
                        write!(self.output, "{} = ", name)?;
                    }
                    self.write_operand(operand)?;
                    first = false;
                }
                write!(self.output, "{}", close_bracket)?;
            }
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
            Rvalue::Ref(mutable, region, place) => {
                write!(self.output, "ref {} [{}]", mutable, region)?;
                self.write_place(place)?;
            }
            Rvalue::Cast(cast, pointer) => {
                write!(self.output, "cast(")?;
                match cast {
                    CastKind::Transmute(to) => {
                        write!(self.output, "Transmute({})", to)?;
                    }
                    CastKind::PointerCast(cast) => match cast {
                        super::PointerCast::RawToRaw(to) => {
                            write!(self.output, "RawToRaw({})", to)?
                        }
                    },
                }
                write!(self.output, ")(")?;
                self.write_operand(pointer)?;
                write!(self.output, ")")?;
            }
            Rvalue::DecodeUtf8(ptr, index) => {
                write!(self.output, "decode_utf8(")?;
                self.write_operand(ptr)?;
                write!(self.output, ",")?;
                self.write_operand(index)?;
                write!(self.output, ")")?;
            }
            Rvalue::RawPtrTo(place) => {
                write!(self.output, "raw_ptr_to(")?;
                self.write_place(place)?;
                write!(self.output, ")")?;
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
                ConstantValue::NamedConst(id, ref args) => {
                    write!(self.output, "{}", self.ctxt.display(id))?;
                    if !args.is_empty() {
                        write!(self.output, "{}", display_generic_args(args))?;
                    }
                    Ok(())
                }
                ConstantValue::ClosureShim(id, ref args) => {
                    write!(self.output, "closure shim ({})", self.ctxt.display(id))?;
                    if !args.is_empty() {
                        write!(self.output, "{}", display_generic_args(args))?;
                    }
                    Ok(())
                }
            },
        }
    }
    fn write_block(&mut self, id: BasicBlockId, block: &BasicBlock) -> std::io::Result<()> {
        writeln!(self.output, " bb{}", id.into_usize())?;
        for stmt in &block.stmts {
            write!(self.output, "  ")?;
            match &stmt.kind {
                StmtKind::Print(value) => {
                    write!(self.output, "print(")?;
                    if let Some(value) = value {
                        self.write_operand(value)?;
                    }
                    writeln!(self.output, ")")?;
                }
                StmtKind::Deallocate(value) => {
                    write!(self.output, "deallocate(")?;
                    self.write_operand(value)?;
                    writeln!(self.output, ")")?;
                }
                StmtKind::DropInPlace(drop) => {
                    let DropInPlace {
                        pointer_to_place,
                        count,
                    } = drop.as_ref();
                    write!(self.output, "copy_non_overlapping(")?;
                    self.write_operand(pointer_to_place)?;
                    write!(self.output, ",")?;
                    self.write_operand(count)?;
                    writeln!(self.output, ")")?;
                }
                StmtKind::CopyNonOverlapping(copy) => {
                    let CopyNonOverlapping { dst, src, count } = copy.as_ref();

                    write!(self.output, "copy_non_overlapping(")?;
                    self.write_operand(dst)?;
                    write!(self.output, ",")?;
                    self.write_operand(src)?;
                    write!(self.output, ",")?;
                    self.write_operand(count)?;
                    writeln!(self.output, ")")?;
                }
                StmtKind::Noop => writeln!(self.output, "noop")?,
                StmtKind::Assign(place, value) => {
                    self.write_place(place)?;
                    write!(self.output, " = ")?;
                    self.write_rvalue(value)?;
                    writeln!(self.output)?;
                }
                StmtKind::Assert(operand, kind) => {
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
        if let Some(ref terminator) = block.terminator {
            match &terminator.kind {
                TerminatorKind::Unreachable => {
                    write!(self.output, "unreachable")?;
                }
                TerminatorKind::Return => {
                    write!(self.output, "return")?;
                }
                TerminatorKind::Switch(operand, targets) => {
                    write!(self.output, "switch ")?;
                    self.write_operand(operand)?;
                    write!(self.output, " ")?;
                    for target in &targets.targets {
                        write!(self.output, "{} -> bb{}, ", target.value, target.target.0)?;
                    }
                    write!(self.output, "otherwise -> bb{}", targets.otherwise.0)?;
                }
                TerminatorKind::Goto(block) => write!(self.output, "goto bb{}", block.0)?,
                TerminatorKind::Panic => write!(self.output, "panic")?,
            }
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
