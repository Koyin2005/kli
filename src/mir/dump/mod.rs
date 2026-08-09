use crate::{
    Symbol,
    collect::{CtxtRef, TypeDefKind},
    index_vec::IndexVec,
    mir::{
        AggregateKind, AssertKind, BasicBlock, BasicBlockId, Body, BodySource, CastKind,
        ConstValue, LocalKind, Operand, Place, PlaceProjection, Rvalue, StmtKind, TerminatorKind,
    },
    typed_ast::FieldId,
    types,
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
        }
        writeln!(self.output, "() -> {}", body.return_type)?;
        for (local, info) in body.locals.iter_enumerated() {
            write!(self.output, " {:?}", local)?;
            match &info.kind {
                LocalKind::Param(var) => write!(
                    self.output,
                    " param {}",
                    if let Some(var) = var {
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
                PlaceProjection::Index(index) => {
                    let _ = write!(&mut output, ".[_{}]", index.0);
                }
                PlaceProjection::CaseDowncast(_, name) => {
                    let current = std::mem::take(&mut output);
                    let _ = write!(&mut output, "({} as {})", current, name);
                }
                PlaceProjection::Deref => {
                    _ = write!(&mut output, "^");
                }
            };
        }
        write!(self.output, "{}", output)
    }
    fn write_rvalue(&mut self, rvalue: &Rvalue) -> std::io::Result<()> {
        match rvalue {
            Rvalue::ReadLine => {
                write!(self.output, "read_line")?;
            }
            Rvalue::UninitZeroed(ty) => {
                write!(self.output, "uninit[{}]", ty)?;
            }
            Rvalue::Use(operand) => {
                self.write_operand(operand)?;
            }
            Rvalue::AllocateRawArray { ty, count } => {
                write!(self.output, "raw_array_alloc[{ty}]")?;
                write!(self.output, "(")?;
                self.write_operand(count)?;
                write!(self.output, ")")?;
            }
            Rvalue::AllocateArray(_, elements) => {
                write!(self.output, "[")?;
                self.write_with_coma_sep(elements, |this, element| this.write_operand(element))?;
                write!(self.output, "]")?;
            }
            Rvalue::AllocateBox(ty, operand) => {
                write!(self.output, "Box[{}](", ty)?;
                self.write_operand(operand)?;
                write!(self.output, ")")?;
            }
            Rvalue::Binary(op, operands) => {
                let (left, right) = &**operands;
                write!(self.output, "{:?}(", op)?;
                self.write_operand(left)?;
                write!(self.output, ",")?;
                self.write_operand(right)?;
                write!(self.output, ")")?;
            }
            Rvalue::Len(place) => {
                write!(self.output, "Len(")?;
                self.write_place(place)?;
                write!(self.output, ")")?;
            }
            Rvalue::Discriminant(place) => {
                write!(self.output, "Discriminant(")?;
                self.write_place(place)?;
                write!(self.output, ")")?;
            }
            Rvalue::Aggregate(kind, fields) => {
                match kind {
                    AggregateKind::Record { .. } | AggregateKind::Tuple => (),
                    AggregateKind::Variant(id, index, args) => {
                        let name = self.ctxt.type_def(*id).case(*index).name;
                        write!(self.output, "{}{}", name, args)?;
                    }
                    AggregateKind::NamedRecord(id, args) => {
                        let name = self.ctxt.type_def(*id).name;
                        write!(self.output, "{}{}", name, args)?;
                    }
                };
                let (open_bracket, close_bracket) = match kind {
                    AggregateKind::Variant(..) | AggregateKind::Tuple => ('(', ')'),
                    _ => ('{', '}'),
                };
                let ctxt = self.ctxt;
                let write_field_name = move |this: &mut MirDump<'_>, i: FieldId| match kind {
                    AggregateKind::Tuple => Ok(()),
                    AggregateKind::Record { field_names } => {
                        write!(this.output, "{} = ", field_names[i])
                    }
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
            Rvalue::Cast(cast, pointer) => {
                write!(self.output, "cast(")?;
                match cast {
                    CastKind::Transmute(to) => {
                        write!(self.output, "Transmute({})", to)?;
                    }
                    CastKind::IntegerCast(kind) => {
                        write!(self.output, "IntegerCast({:?})", kind)?;
                    }
                }
                write!(self.output, ")(")?;
                self.write_operand(pointer)?;
                write!(self.output, ")")?;
            }
            Rvalue::AddrOf(place) => {
                write!(self.output, "addr_of(")?;
                self.write_place(place)?;
                write!(self.output, ")")?;
            }
            Rvalue::Repeat { ty, value, count } => {
                write!(self.output, "repeat[{ty}](")?;
                self.write_operand(value)?;
                write!(self.output, ",")?;
                self.write_operand(count)?;
                write!(self.output, ")")?;
            }
        }
        Ok(())
    }
    fn write_constant(&mut self, ty: &types::TypeKind, value: &ConstValue) -> std::io::Result<()> {
        if let ConstValue::Named(id, args) = value {
            return write!(self.output, "{}{}", self.ctxt.display_path_for(*id), args);
        } else if let ConstValue::String(string) = value {
            return write!(self.output, "\"{string}\"");
        } else if let ConstValue::ZeroSized = value {
            return write!(self.output, "{ty}");
        }
        match ty {
            types::TypeKind::String => unreachable!(),
            types::TypeKind::Infer(_)
            | types::TypeKind::Param(..)
            | types::TypeKind::Unknown
            | types::TypeKind::Uninit(_) => {
                write!(self.output, "unknown of '{}'", ty)
            }
            types::TypeKind::Char => {
                let &ConstValue::Scalar(value) = value else {
                    unreachable!("can only be a scalar for char")
                };
                let Some(char) = value.try_into().ok().and_then(char::from_u32) else {
                    unreachable!("Scalar constant should be char")
                };
                write!(self.output, "'{char}'")
            }
            types::TypeKind::Int(_) => value
                .as_scalar()
                .map(|value| write!(self.output, "{}", value))
                .unwrap_or_else(|| write!(self.output, "unknown of '{}'", ty)),
            types::TypeKind::Bool => value
                .as_scalar()
                .and_then(|value| bool::try_from(value).ok())
                .map_or(Ok(()), |value| write!(self.output, "{}", value)),
            types::TypeKind::Never => unreachable!("already did zero sized types"),
            types::TypeKind::Function(_) => match value {
                ConstValue::Named(id, args) => {
                    write!(self.output, "{}{}", self.ctxt.display_path_for(*id), args)
                }
                _ => unreachable!("only values of function type"),
            },
            types::TypeKind::Tuple(_) => {
                let ConstValue::Record(field_consts) = value else {
                    unreachable!("should be a record")
                };
                let (fields, (open_bracket, closing_bracket)) = match ty {
                    types::TypeKind::Tuple(_) => {
                        (&IndexVec::<FieldId, types::RecordField>::new(), ('(', ')'))
                    }
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
            types::TypeKind::Array(_) | types::TypeKind::Box(_) => unimplemented!(),
            types::TypeKind::Named(def_id, name, args) => match self.ctxt.type_def(*def_id).kind {
                TypeDefKind::Record(fields) => match value {
                    ConstValue::Record(values) => {
                        write!(self.output, "{name}{}{{", args)?;
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
                        write!(self.output, "{name}{}", args)?;
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
                    self.write_operand(value)?;

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
                    write!(
                        self.output,
                        "assert({}",
                        if kind.negate() { "!" } else { "" }
                    )?;
                    self.write_operand(operand)?;
                    write!(self.output, ", ")?;
                    match kind {
                        AssertKind::InBounds => write!(self.output, "\"index out of bounds\"")?,
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
