use crate::{
    typecheck::infer::TypeInfer,
    typed_ast::{Expr, ExprKind, Function, Pattern, PatternKind, Place, PlaceKind, Stmt, StmtKind},
    types::{FunctionType, GenericArg, Region, Type},
};

pub struct TypeSubst<'a> {
    infer: &'a mut TypeInfer,
}
impl<'a> TypeSubst<'a> {
    pub fn new(infer: &'a mut TypeInfer) -> Self {
        Self { infer }
    }
    pub fn subst_type(&mut self, ty: &mut Type) {
        match ty {
            Type::Bool
            | Type::Int(_)
            | Type::Unknown
            | Type::Char
            | Type::Param(..)
            | Type::Never
            | Type::Byte => (),
            Type::Array(ty, _) | Type::RawPointer(ty) => self.subst_type(ty),
            Type::Imm(region, ty) | Type::Mut(region, ty) => {
                self.subst_region(region);
                self.subst_type(ty);
            }
            Type::Function(FunctionType {
                resource: _,
                params,
                return_type,
            }) => {
                for param in params {
                    self.subst_type(param);
                }
                self.subst_type(return_type);
            }
            Type::Record(fields) => {
                for field in fields {
                    self.subst_type(&mut field.ty);
                }
            }
            Type::Tuple(fields) => {
                for field in fields {
                    self.subst_type(field);
                }
            }
            Type::Infer(var) => *ty = self.infer.simplify_type(Type::Infer(*var)),
            Type::Named(_, _, args) => {
                for arg in args {
                    self.subst_generic_arg(arg);
                }
            }
        }
    }
    pub fn subst_region(&mut self, region: &mut Region) {
        match region {
            Region::Static | Region::Unknown | Region::Param(..) => (),
            Region::Infer(var) => *region = self.infer.simplify_region(Region::Infer(*var)),
        }
    }
    pub fn subst_generic_args(&mut self, args: &mut [GenericArg]) {
        for arg in args {
            self.subst_generic_arg(arg);
        }
    }
    pub fn subst_generic_arg(&mut self, arg: &mut GenericArg) {
        match arg {
            GenericArg::Region(region) => {
                self.subst_region(region);
            }
            GenericArg::Type(ty) => {
                self.subst_type(ty);
            }
        }
    }
    pub fn subst_pattern(&mut self, pattern: &mut Pattern) {
        match &mut pattern.kind {
            PatternKind::Bool(_) | PatternKind::Int(_) | PatternKind::Err | PatternKind::Unit => (),
            PatternKind::Ref(pattern) => self.subst_pattern(pattern),
            PatternKind::Binding(.., ty) => self.subst_type(ty),
            PatternKind::Case(.., args, _, inner) => {
                self.subst_generic_args(args);
                if let Some(inner) = inner {
                    self.subst_pattern(inner);
                }
            }
            PatternKind::Record(fields) => {
                for field in fields {
                    self.subst_pattern(&mut field.pattern);
                }
            }
        }
        self.subst_type(&mut pattern.ty);
    }
    pub fn subst_place(&mut self, place: &mut Place) {
        match &mut place.kind {
            PlaceKind::Deref(expr) => self.subst_expr(expr),
            PlaceKind::Field(place, _) => self.subst_place(place),
            PlaceKind::Var(..) | PlaceKind::Upvar(..) | PlaceKind::Invalid => (),
        }
        self.subst_type(&mut place.ty);
    }
    pub fn subst_stmt(&mut self, stmt: &mut Stmt) {
        match &mut stmt.kind {
            StmtKind::Expr(expr) => self.subst_expr(expr),
            StmtKind::Let(let_binding) => {
                self.subst_pattern(&mut let_binding.pattern);
                self.subst_expr(&mut let_binding.value);
            }
        }
    }
    pub fn subst_expr(&mut self, expr: &mut Expr) {
        match &mut expr.kind {
            ExprKind::Return(value) | ExprKind::Unsafe(value) => {
                self.subst_expr(value);
            }
            ExprKind::Block(block) => {
                for stmt in &mut block.stmts {
                    self.subst_stmt(stmt);
                }
                self.subst_expr(&mut block.expr);
            }
            ExprKind::Const(_, args) => {
                for arg in args {
                    self.subst_generic_arg(arg);
                }
            }
            ExprKind::NeverToAny(expr) => {
                self.subst_expr(expr);
            }
            ExprKind::Bool(_)
            | ExprKind::Err
            | ExprKind::Unit
            | ExprKind::Int(_)
            | ExprKind::String(_)
            | ExprKind::Panic => (),
            ExprKind::AddressOf(place) => {
                self.subst_place(place);
            }
            ExprKind::Binary(_, first, second)
            | ExprKind::While(first, second)
            | ExprKind::Logic(_, first, second) => {
                self.subst_expr(first);
                self.subst_expr(second);
            }
            ExprKind::Print(expr) => {
                if let Some(expr) = expr {
                    self.subst_expr(expr);
                }
            }
            ExprKind::VariantInit(.., args, expr) => {
                for arg in args {
                    self.subst_generic_arg(arg);
                }
                if let Some(expr) = expr {
                    self.subst_expr(expr)
                }
            }
            ExprKind::Call(callee, args) => {
                self.subst_expr(callee);
                for arg in args {
                    self.subst_expr(arg);
                }
            }
            ExprKind::Load(place) => {
                self.subst_place(place);
            }
            ExprKind::For { iterator_type, .. } => match *iterator_type {},
            ExprKind::Assign(place, expr) => {
                self.subst_place(place);
                self.subst_expr(expr);
            }
            ExprKind::Borrow { place, region, .. } => {
                self.subst_place(place);
                self.subst_region(region);
            }
            ExprKind::Case(matchee, arms) => {
                self.subst_expr(matchee);
                for arm in arms {
                    self.subst_pattern(&mut arm.pattern);
                    self.subst_expr(&mut arm.body);
                }
            }
            ExprKind::Function(.., args) => {
                for arg in args {
                    self.subst_generic_arg(arg);
                }
            }
            ExprKind::BuiltinCall(_, generic_args, args) => {
                for arg in generic_args {
                    self.subst_generic_arg(arg);
                }
                for expr in args {
                    self.subst_expr(expr);
                }
            }
            ExprKind::Lambda(lambda) => {
                for capture in lambda.captures.iter_mut() {
                    self.subst_type(&mut capture.ty);
                }
                for ty in lambda.param_tys.iter_mut() {
                    self.subst_type(ty);
                }
                self.subst_type(&mut lambda.return_type);
            }
            ExprKind::Record(fields) => {
                for field in fields {
                    self.subst_expr(&mut field.value);
                }
            }
            ExprKind::Tuple(fields) => {
                for field in fields {
                    self.subst_expr(field);
                }
            }
            ExprKind::NamedRecord(_, args, fields) => {
                self.subst_generic_args(args);
                for field in fields {
                    self.subst_expr(&mut field.value);
                }
            }
        }
        self.subst_type(&mut expr.ty);
    }
    pub fn subst_function(&mut self, function: &mut Function) {
        for param in function.params.iter_mut() {
            self.subst_type(&mut param.ty);
        }
        self.subst_type(&mut function.return_type);
        if let Some(body) = function.body.as_mut() {
            self.subst_expr(body);
        }
    }
}
