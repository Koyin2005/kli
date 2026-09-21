use crate::ir::{AggregateKind, Body, Call, Constant, Expr, ExprKind, Place, Program, Stmt};

pub struct Print<'a> {
    program: &'a Program,
    output: Box<dyn std::io::Write>,
    indent: usize,
}
impl<'a> Print<'a> {
    pub fn new(program: &'a Program, output: impl std::io::Write + 'static) -> Self {
        Self {
            program,
            output: Box::new(output),
            indent: 0,
        }
    }

    fn format_place(&self, place: &Place) -> String {
        match place {
            Place::Local(local) => format!("t{}", local.into_usize()),
            Place::Field(base, index) => {
                format!("{}.{}", self.format_place(base), index.into_usize())
            }
            Place::Downcast(place, case) => {
                format!("({} as {})", self.format_place(place), case.into_usize())
            }
            Place::Deref(place) => format!("{}^", self.format_place(place)),
        }
    }
    fn format_value(&self, value: &Expr) -> String {
        match &value.kind {
            ExprKind::Load(place) => self.format_place(place),
            ExprKind::Aggregate(kind, fields) => match kind {
                AggregateKind::Tuple => {
                    let mut output = "(".to_string();
                    for (i, value) in fields.iter().enumerate() {
                        if i > 0 {
                            output.push_str(",");
                        }
                        output.push_str(&self.format_value(value));
                    }
                    output.push_str(")");
                    output
                }
            },
            ExprKind::Constant(value) => match value {
                Constant::Bool(value) => value.to_string(),
                Constant::Function(id) => self.program.bodies[*id].name.clone(),
                Constant::Int(value) => value.to_string(),
                Constant::String(s) => s.with_str(|s| format!("\"{}\"", s.escape_debug())),
            },
            ExprKind::Discriminant(_) => todo!(),
        }
    }
    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.write(' ');
        }
    }
    fn write(&mut self, f: impl std::fmt::Display) {
        write!(self.output, "{}", f).unwrap();
    }
    fn write_newline_after(&mut self, f: impl FnOnce(&mut Self)) {
        f(self);
        self.write("\n");
    }
    fn print_stmt(&mut self, stmt: &Stmt) {
        self.write_indent();
        match stmt {
            Stmt::Assign(place, value) => {
                self.write(format!(
                    "{} = {}\n",
                    self.format_place(place),
                    self.format_value(value)
                ));
            }
            Stmt::Panic => todo!(),
            Stmt::Print { value, is_err } => {
                self.write(if *is_err { "eprint " } else { "print " });
                self.write(self.format_value(value));
                self.write("\n");
            }
            Stmt::Block(stmts) => todo!(),
            Stmt::Loop(loop_label, stmts) => todo!(),
            Stmt::Break(loop_label) => todo!(),
            Stmt::If(condition, then_branch, else_branch) => {
                self.write("if ");
                self.write(self.format_value(condition));
                self.write(":\n");
                self.write_indent();
                self.write("then:\n");
                self.indent += 1;

                for stmt in then_branch {
                    self.print_stmt(stmt);
                }
                self.indent -= 1;
                self.write_indent();
                self.write("else:\n");
                self.indent += 1;

                for stmt in else_branch {
                    self.print_stmt(stmt);
                }
                self.indent -= 1;
            }
            Stmt::Match(_) => todo!(),
            Stmt::Call(Call {
                return_place,
                callee,
                args,
            }) => {
                self.write_newline_after(|this| {
                    this.write(this.format_place(return_place));
                    this.write(" = ");
                    this.write(this.format_value(callee));
                    this.write("(");
                    for (i, arg) in args.iter().enumerate() {
                        if i > 0 {
                            this.write(",");
                        }
                        this.write(this.format_value(arg));
                    }
                    this.write(")");
                });
            }
            Stmt::Return(expr) => {
                self.write(format!("return {}\n", self.format_value(expr)));
            }
        }
    }
    pub fn print_body(mut self, body: &Body) {
        self.write("(");
        for (i, param) in body.locals.as_slice()[0..body.param_count as usize]
            .iter()
            .enumerate()
        {
            if i > 0 {
                self.write(",");
            }
            self.write(format!("{:?}", param.ty));
        }
        self.write(") -> ");

        self.write(format!("{:?}:\n", body.return_ty));
        self.indent += 1;
        for stmt in &body.body {
            self.print_stmt(stmt);
        }
        self.write("\n");
    }
}
