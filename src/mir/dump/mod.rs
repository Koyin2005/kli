use std::fmt::Write;

use crate::{
    Symbol,
    collect::CtxtRef,
    def_ids::DefId,
    mir::{
        self, AggregateKind, BasicBlock, BasicBlockId, Body, BodySource, LocalKind, Operation,
        Place, PlaceProjection, StmtKind, TerminatorKind, Value,
    },
    typed_ast::FieldId,
    types::GenericArgs,
};
#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Fmt(core::fmt::Error),
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<core::fmt::Error> for Error {
    fn from(value: core::fmt::Error) -> Self {
        Self::Fmt(value)
    }
}

type WriteResult = Result<(), Error>;
pub struct MirDump<'ctxt> {
    output: String,
    ctxt: CtxtRef<'ctxt>,
}
impl<'ctxt> MirDump<'ctxt> {
    pub fn new(ctxt: CtxtRef<'ctxt>) -> Self {
        Self {
            output: String::new(),
            ctxt,
        }
    }
    fn write_fmt_path(&mut self, id: DefId, args: &GenericArgs<'ctxt>) -> WriteResult {
        Ok(write!(
            &mut self.output,
            "{}{}",
            self.ctxt.display_path_for(id),
            args
        )?)
    }
    fn write_fmt(&mut self, f: impl std::fmt::Display) -> WriteResult {
        Ok(write!(&mut self.output, "{f}")?)
    }
    fn write_fmt_dbg(&mut self, f: impl std::fmt::Debug) -> WriteResult {
        Ok(write!(&mut self.output, "{f:?}")?)
    }
    fn writeln_fmt(&mut self, f: impl std::fmt::Display) -> WriteResult {
        Ok(writeln!(&mut self.output, "{f}")?)
    }
    fn write_with_coma_sep<T>(
        &mut self,
        elems: impl IntoIterator<Item = T>,
        mut f: impl FnMut(&mut Self, T) -> WriteResult,
    ) -> WriteResult {
        let mut first = true;
        for value in elems {
            if !first {
                self.output.push(',');
            }
            f(self, value)?;
            first = false;
        }
        Ok(())
    }
    fn write_header(&mut self, body: &Body) -> WriteResult {
        match body.src {
            BodySource::Function(f) => {
                if let crate::resolved_ast::Node::Method(_) = self.ctxt.node(f) {
                    let ty_id = self.ctxt.expect_parent(self.ctxt.expect_parent(f));
                    self.write_fmt("fun ")?;
                    self.write_fmt_path(ty_id, &GenericArgs::new())?;
                    self.write_fmt(".")?;
                    self.write_fmt_path(f, &GenericArgs::new())?;
                } else {
                    self.write_fmt("fun ")?;
                    self.write_fmt_path(f, &GenericArgs::new())?;
                }
            }
        }
        self.write_fmt("(")?;
        self.write_with_coma_sep(body.param_locals_iter(), |this, param| {
            let index = param.into_usize();
            this.write_fmt("_")?;
            this.write_fmt(index)
        })?;
        self.write_fmt(") -> ")?;
        self.writeln_fmt(body.return_type)?;
        for (local, info) in body.locals.iter_enumerated() {
            self.write_fmt(" ")?;
            self.write_fmt_dbg(local)?;
            match &info.kind {
                LocalKind::Param(var) => {
                    self.write_fmt(" param ")?;
                    self.write_fmt(if let Some(var) = var {
                        var.0
                    } else {
                        Symbol::EMPTY_STRING
                    })?;
                }
                LocalKind::Var(var) => {
                    self.write_fmt(" var ")?;
                    self.write_fmt(var.0)?;
                }
                LocalKind::Temp => {
                    self.write_fmt(" temp ")?;
                    self.write_fmt(local.0)?;
                }
                LocalKind::Env => self.write_fmt("env")?,
            };
            self.write_fmt(" : ")?;
            self.writeln_fmt(info.ty)?;
        }
        Ok(())
    }
    fn write_place(&mut self, place: &Place<'ctxt>) -> WriteResult {
        let old_output = std::mem::take(&mut self.output);
        match place.base {
            mir::PlaceBase::Local(local) => self.write_fmt(local)?,
            mir::PlaceBase::ArrayElement(mir::ArrayElement {
                ref base,
                ref index,
            }) => {
                self.write_value(&base)?;
                self.write_fmt(".[")?;
                self.write_value(index)?;
                self.write_fmt("]")?;
            }
        }
        for projection in place.projections.iter() {
            match projection {
                PlaceProjection::Field(field) => {
                    self.write_fmt(".")?;
                    self.write_fmt(field.into_usize())?;
                }
                PlaceProjection::ConstantIndex(index) => {
                    write!(self.output, ".[{}]", index)?;
                }
                PlaceProjection::Index(index) => {
                    self.write_fmt(".[")?;
                    self.write_fmt(index)?;
                    self.write_fmt("]")?;
                }
                PlaceProjection::CaseDowncast(_, name) => {
                    let current = std::mem::take(&mut self.output);
                    self.write_fmt("(")?;
                    self.write_fmt(current)?;
                    self.write_fmt(" as ")?;
                    self.write_fmt(name)?;
                    self.write_fmt(")")?;
                }
                PlaceProjection::Deref => {
                    self.write_fmt("^")?;
                }
            };
        }
        let output = std::mem::replace(&mut self.output, old_output);
        self.write_fmt(output)
    }
    fn write_value(&mut self, value: &Value<'ctxt>) -> WriteResult {
        match value {
            Value::Reg(reg) => self.write_fmt(reg),
            Value::Unit => self.write_fmt("()"),
            Value::Bool(value) => self.write_fmt(value),
            Value::Int(value) => self.write_fmt(value),
            Value::Unknown(ty) => {
                self.write_fmt("unknown[")?;
                self.write_fmt(ty)?;
                self.write_fmt("]")
            }
            Value::Function(id, args) => self.write_fmt_path(*id, args),
            Value::Lambda(_, id, args) => self.write_fmt_path(*id, args),
            Value::Char(char) => self.write_fmt(char),
            Value::String(string) => {
                self.write_fmt("\"")?;
                self.write_fmt(string)?;
                self.write_fmt("\"")
            }
        }
    }
    fn write_operation(&mut self, operation: &Operation<'ctxt>) -> WriteResult {
        match operation {
            Operation::InBounds(base, index) => {
                self.write_fmt("in_bounds ")?;
                self.write_value(base)?;
                self.write_fmt(",")?;
                self.write_value(index)
            }
            Operation::Not(value) => {
                self.write_fmt("not ")?;
                self.write_value(value)
            }
            Operation::Discriminant(value) => {
                self.write_fmt("discriminant ")?;
                self.write_value(value)
            }
            Operation::ExtractPayload(value, case) => {
                self.write_fmt("extract_payload ")?;
                self.write_value(value)?;
                self.write_fmt(" as ")?;
                self.write_fmt(case.into_usize())
            }
            Operation::Len(array) => {
                self.write_fmt("len ")?;
                self.write_value(array)
            }
            Operation::Load(place) => {
                self.write_fmt("load ")?;
                self.write_place(place)
            }
            Operation::ExtractElement(array, index) => {
                self.write_fmt("extract_element ")?;
                self.write_value(array)?;
                self.write_fmt(",")?;
                self.write_value(index)
            }
            Operation::AllocArray(ty, fields) => {
                self.write_fmt("alloc_array[")?;
                self.write_fmt(ty)?;
                self.write_fmt("] ")?;
                self.write_with_coma_sep(fields, |this, field| this.write_value(field))
            }
            Operation::Cmp(cmp, left, right) => {
                self.write_fmt("cmp.")?;
                self.write_fmt(match cmp {
                    mir::Comparison::Equals => "eq",
                    mir::Comparison::Greater => "gt",
                    mir::Comparison::Lesser => "lt",
                })?;
                self.write_value(left)?;
                self.write_fmt(",")?;
                self.write_value(right)
            }
            Operation::Arith(op, left, right) => {
                self.write_fmt(match op {
                    mir::ArithOp::Add => "add",
                    mir::ArithOp::AddOverflow => "add_overflow",
                    mir::ArithOp::Sub => "sub",
                    mir::ArithOp::SubOverflow => "sub_overflow",
                    mir::ArithOp::Mul => "mul",
                    mir::ArithOp::MulOverflow => "mul_overflow",
                })?;
                self.write_fmt(" ")?;
                self.write_value(left)?;
                self.write_fmt(", ")?;
                self.write_value(right)
            }
            Operation::ExtractField(value, field) => {
                self.write_fmt("extract_field ")?;
                self.write_value(value)?;
                self.write_fmt(", ")?;
                self.write_fmt(field.into_usize())
            }
            Operation::Call(callee, args) => {
                self.write_fmt("call ")?;
                self.write_value(callee)?;
                self.write_fmt("(")?;
                self.write_with_coma_sep(args, |this, arg| this.write_value(arg))?;
                self.write_fmt(")")
            }
            Operation::Aggregate(kind, fields) => {
                match kind {
                    AggregateKind::Tuple => (),
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
                    AggregateKind::Variant(_, _, _) => write!(this.output, "{} = ", i.into_usize()),
                    AggregateKind::NamedRecord(id, ..) => {
                        write!(this.output, "{} = ", ctxt.type_def(*id).fields()[i].name)
                    }
                    _ => Ok(()),
                };
                self.write_fmt(open_bracket)?;
                self.write_with_coma_sep(fields.iter_enumerated(), |this, (i, operand)| {
                    write_field_name(this, i)?;
                    this.write_value(operand)
                })?;
                self.write_fmt(close_bracket)
            }
        }
    }
    fn write_block(&mut self, id: BasicBlockId, block: &BasicBlock<'ctxt>) -> WriteResult {
        self.write_fmt(id)?;
        if !block.args.is_empty() {
            self.write_fmt("(")?;
            self.write_with_coma_sep(block.args.iter(), |this, arg| this.write_fmt(arg))?;
            self.write_fmt(")")?;
        }
        self.writeln_fmt("")?;
        for stmt in &block.stmts {
            self.write_fmt("  ")?;
            match &stmt.kind {
                StmtKind::Store(place, value) => {
                    self.write_fmt("store ")?;
                    self.write_place(place)?;
                    write!(self.output, " = ")?;
                    self.write_value(value)?;
                    writeln!(self.output)?;
                }
                StmtKind::Assign(reg, operation) => {
                    write!(self.output, "%{} = ", reg.0)?;
                    self.write_operation(operation)?;
                    writeln!(self.output)?;
                }
                StmtKind::Print { err, .. } => {
                    write!(self.output, "{}print ", if *err { "e" } else { "" })?;
                    todo!("Handle print");
                }
                StmtKind::Noop => self.write_fmt("noop")?,
                StmtKind::PanicIf(value) => {
                    self.write_fmt("panic_if ")?;
                    self.write_value(value)?;
                    self.writeln_fmt("")?;
                }
                StmtKind::OldStore(place, _) => {
                    self.write_place(place)?;
                    write!(self.output, " = ")?;
                    todo!("remove rvalues");
                }
            }
        }
        self.write_fmt(" ")?;
        if let Some(ref terminator) = block.terminator {
            match &terminator.kind {
                TerminatorKind::Switch(value, targets) => {
                    self.write_fmt("switch ")?;
                    self.write_value(value)?;
                    self.writeln_fmt("")?;
                    for target in &targets.targets {
                        writeln!(self.output, "   {} -> {}", target.value, target.target)?;
                    }
                    write!(self.output, "   otherwise -> {}", targets.otherwise)?;
                }
                TerminatorKind::Unreachable => {
                    write!(self.output, "unreachable")?;
                }
                TerminatorKind::OldReturn(..) => {
                    todo!("Ignored")
                }
                TerminatorKind::Return(value) => {
                    write!(self.output, "return ")?;
                    self.write_value(value)?;
                }
                TerminatorKind::OldSwitch(..) => {
                    todo!("Ignore me")
                }
                TerminatorKind::Goto(block, args) => {
                    write!(self.output, "goto {}", block)?;
                    if !args.is_empty() {
                        write!(self.output, "(")?;
                        self.write_with_coma_sep(args, |this, arg| this.write_value(arg))?;
                        write!(self.output, ")")?;
                    }
                }
                TerminatorKind::Panic => self.write_fmt("panic")?,
                TerminatorKind::OldAssert(..) => {
                    todo!("ignored")
                }
            }
        }
        self.writeln_fmt("")
    }
    pub fn write_body(mut self, body: &Body<'ctxt>) -> WriteResult {
        use std::io::Write;
        self.write_header(body)?;
        for (id, block) in body.block_info.blocks().iter_enumerated() {
            self.write_block(id, block)?;
        }
        self.write_fmt("end\n\n")?;
        std::io::stdout()
            .write_all(self.output.as_bytes())
            .map_err(Error::Io)
    }
}
