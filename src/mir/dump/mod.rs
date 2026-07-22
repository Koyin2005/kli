use crate::{
    Symbol,
    collect::{CtxtRef, TypeDefKind},
    index_vec::IndexVec,
    mir::{
        AggregateKind, AssertKind, BasicBlock, BasicBlockId, Body, BodySource, CastKind,
        ConstValue, CopyNonOverlapping, DropInPlace, LocalKind, Operand, Place, PlaceProjection,
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
    fn write_with_coma_sep<T>(
        &mut self,
        elems: impl IntoIterator<Item = T>,
        mut f: impl FnMut(&mut Self, T) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut first = true;
        for value in elems {
            if !first {
                write!(self.output, ",")?;
            }
            f(self, value)?;
            first = false;
        }
        Ok(())
    }
    fn write_header(&mut self, body: &Body) -> std::io::Result<()> {
        match body.src {
            BodySource::Function(f) => {
                if let crate::resolved_ast::Node::Method(_) = self.ctxt.node(f) {
                    let ty_id = self.ctxt.expect_parent(self.ctxt.expect_parent(f));
                    write!(
                        self.output,
                        "fun {}.{}",
                        self.ctxt.display(ty_id),
                        self.ctxt.display(f)
                    )?;
                } else {
                    write!(self.output, "fun {}", self.ctxt.display(f))?;
                }
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
        if place.projections.is_empty() {
            return write!(self.output, "{}", place.base);
        }
        let mut output = format!("{}", place.base);
        for projection in place.projections.iter() {
            use std::fmt::Write;
            match projection {
                PlaceProjection::Field(field) => {
                    let _ = write!(&mut output, ".{}", field.into_usize());
                }
                PlaceProjection::ConstantIndex(index) => {
                    let _ = write!(&mut output, ".[{}]", index);
                }
                PlaceProjection::Deref => {
                    output.push('^');
                }
                PlaceProjection::Index(index) => {
                    let _ = write!(&mut output, ".[_{}]", index.0);
                }
                PlaceProjection::CaseDowncast(_, name) => {
                    let current = std::mem::take(&mut output);
                    let _ = write!(&mut output, "({} as {})", current, name);
                }
            };
        }
        write!(self.output, "{}", output)
    }
    fn write_rvalue(&mut self, rvalue: &Rvalue) -> std::io::Result<()> {
        match rvalue {
            Rvalue::DanglingPtr(ty) => {
                write!(self.output, "dangling_ptr[{}]", ty)?;
            }
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
                match kind {
                    AggregateKind::Array(..)
                    | AggregateKind::Record { .. }
                    | AggregateKind::Tuple => (),
                    AggregateKind::Closure(params, return_type) => {
                        write!(self.output, "Closure((")?;
                        self.write_with_coma_sep(params, |this, param| {
                            write!(this.output, "{}", param)
                        })?;
                        write!(self.output, ") -> {return_type})")?;
                    }
                    AggregateKind::Variant(id, index, args) => {
                        let name = self.ctxt.type_def(*id).case(*index).name;
                        write!(self.output, "{}{}", name, display_generic_args(args))?;
                    }
                    AggregateKind::NamedRecord(id, args) => {
                        let name = self.ctxt.type_def(*id).name;
                        write!(self.output, "{}{}", name, display_generic_args(args))?;
                    }
                };
                let (open_bracket, close_bracket) = match kind {
                    AggregateKind::Array(_, _) => ('[', ']'),
                    AggregateKind::Variant(..) | AggregateKind::Tuple => ('(', ')'),
                    _ => ('{', '}'),
                };
                let ctxt = self.ctxt;
                let write_field_name = move |this: &mut MirDump<'_>, i: FieldId| match kind {
                    AggregateKind::Tuple | AggregateKind::Array(..) => Ok(()),
                    AggregateKind::Record { field_names } => {
                        write!(this.output, "{} = ", field_names[i])
                    }
                    AggregateKind::Closure(..) => write!(
                        this.output,
                        "{} = ",
                        match i {
                            i if i == FieldId::FIRST_FIELD => "env",
                            i if i == FieldId::new(1) => "code",
                            _ => unreachable!("Should only have 2 fields"),
                        }
                    ),
                    AggregateKind::Variant(_, _, _) => write!(this.output, "{} = ", i.into_usize()),
                    AggregateKind::NamedRecord(id, ..) => {
                        write!(this.output, "{} = ", ctxt.type_def(*id).fields()[i].name)
                    }
                };
                write!(self.output, "{open_bracket}")?;
                self.write_with_coma_sep(fields.iter_enumerated(), |this, (i, operand)| {
                    write_field_name(this, i)?;
                    this.write_operand(operand)
                })?;
                write!(self.output, "{}", close_bracket)?;
            }
            Rvalue::Call(operand, args) => {
                self.write_operand(operand)?;
                write!(self.output, "(")?;
                self.write_with_coma_sep(args, |this, arg| this.write_operand(arg))?;
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
            Rvalue::RawPtrTo(place) => {
                write!(self.output, "raw_ptr_to(")?;
                self.write_place(place)?;
                write!(self.output, ")")?;
            }
        }
        Ok(())
    }
    fn write_constant(&mut self, ty: &types::Type, value: &ConstValue) -> std::io::Result<()> {
        if let ConstValue::Named(id, args) | ConstValue::ClosureShim(id, args) = value {
            return write!(
                self.output,
                "{}{}",
                self.ctxt.display_path_for(*id),
                display_generic_args(args)
            );
        } else if let ConstValue::String(string) = value {
            return write!(self.output, "\"{string}\"");
        } else if let ConstValue::ZeroSized = value {
            return write!(self.output, "{ty}");
        }
        match ty {
            types::Type::Infer(_) | types::Type::Param(..) | types::Type::Unknown => {
                write!(self.output, "unknown of '{}'", ty)
            }
            types::Type::Char => {
                let &ConstValue::Scalar(value) = value else {
                    unreachable!("can only be a scalar for char")
                };
                let Some(char) = value.try_into().ok().and_then(char::from_u32) else {
                    unreachable!("Scalar constant should be char")
                };
                write!(self.output, "'{char}'")
            }
            types::Type::Int(_) | types::Type::Byte => value
                .as_scalar()
                .map(|value| write!(self.output, "{}", value))
                .unwrap_or_else(|| write!(self.output, "unknown of '{}'", ty)),
            types::Type::Bool => value
                .as_scalar()
                .and_then(|value| bool::try_from(value).ok())
                .map_or(Ok(()), |value| write!(self.output, "{}", value)),
            types::Type::Never => unreachable!("already did zero sized types"),
             types::Type::RawPointer(_) => {
                write!(self.output, "unknown of '{}'", ty)
            }
            types::Type::Function(_) => match value {
                ConstValue::Named(id, args) => {
                    write!(
                        self.output,
                        "{}{}",
                        self.ctxt.display_path_for(*id),
                        display_generic_args(args)
                    )
                }
                ConstValue::ClosureShim(id, args) => {
                    write!(
                        self.output,
                        "closure_shim {}{}",
                        self.ctxt.display(*id),
                        display_generic_args(args)
                    )
                }
                _ => unreachable!("only values of function type"),
            },
            types::Type::Record(_) | types::Type::Tuple(_) => {
                let ConstValue::Record(field_consts) = value else {
                    unreachable!("should be a record")
                };
                let (fields, (open_bracket, closing_bracket)) = match ty {
                    types::Type::Tuple(_) => (&IndexVec::new(), ('(', ')')),
                    types::Type::Record(fields) => (fields, ('{', '}')),
                    _ => unreachable!(),
                };
                write!(self.output, "{}", open_bracket)?;
                self.write_with_coma_sep(
                    field_consts.iter().enumerate(),
                    move |this, (i, value)| {
                        let i = FieldId::new(i);
                        if let Some(field) = fields.get(i) {
                            write!(this.output, "{} = ", field.name)?;
                        }
                        this.write_constant(&value.ty, &value.value)
                    },
                )?;
                write!(self.output, "{}", closing_bracket)
            }
            types::Type::Array(..) => unimplemented!(),
            types::Type::Named(def_id, name, args) => match self.ctxt.type_def(*def_id).kind {
                TypeDefKind::Record(fields) => match value {
                    ConstValue::Record(values) => {
                        write!(self.output, "{name}{}{{", display_generic_args(args))?;
                        self.write_with_coma_sep(
                            values.iter().zip(fields),
                            |this, (value, field)| {
                                write!(this.output, "{} = ", field.name)?;
                                this.write_constant(&value.ty, &value.value)
                            },
                        )?;
                        write!(self.output, "}}")
                    }
                    _ => write!(self.output, "unknown value of {ty}"),
                },
                TypeDefKind::Variant(cases) => match value {
                    ConstValue::Variant(case, inner) => {
                        let name = cases[*case].name;
                        write!(self.output, "{name}{}", display_generic_args(args))?;
                        if let Some(inner) = inner {
                            write!(self.output, "(")?;
                            self.write_constant(&inner.ty, &inner.value)?;
                            write!(self.output, ")")?;
                        } else {
                            write!(self.output, "")?;
                        }
                        Ok(())
                    }
                    _ => write!(self.output, "unknown of '{}'", ty),
                },
            },
        }
    }
    fn write_operand(&mut self, operand: &Operand) -> std::io::Result<()> {
        match operand {
            Operand::Load(place) => {
                write!(self.output, "load ")?;
                self.write_place(place)
            }
            Operand::Constant(constant) => {
                write!(self.output, "const ")?;
                self.write_constant(&constant.ty, &constant.value)
            }
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
                    let DropInPlace { pointer_to_place } = drop.as_ref();
                    write!(self.output, "drop_in_place(")?;
                    self.write_operand(pointer_to_place)?;
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
                TerminatorKind::Assert(operand, kind, block) => {
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
                    write!(self.output, ") -> bb{}", block.0)?
                }
            }
        }
        writeln!(self.output)
    }
    pub fn write_body(mut self, body: &Body) -> std::io::Result<()> {
        self.write_header(body)?;
        for (id, block) in body.block_info.blocks().iter_enumerated() {
            self.write_block(id, block)?;
        }
        writeln!(self.output, "end\n")?;
        Ok(())
    }
}
