use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{BorrowExpr, GenericArgs, ModuleId, Path, StmtKind};
use crate::diagnostics::DiagnosticReporter;
use crate::ident::Ident;
use crate::index_vec::IndexVec;
use crate::resolved_ast::{Builtin, FunctionId, GenericKind, LambdaId, LocalRegionId, VarId};
use crate::src_loc::SrcLoc;
use crate::{ast, names, resolved_ast as res};

pub struct ResolveErrored;

enum NameResolutionError {
    NotInScope,
    InvalidPathStart,
}
struct ModuleInfo {
    env: Scope,
}

#[derive(Clone, Copy, Debug)]
enum TypeAlias {
    Ptr,
}
type Scope = HashMap<Rc<str>, Res>;
#[derive(Clone, Copy, Debug)]
enum Res {
    LocalRegion(LocalRegionId),
    Param(usize),
    Builtin(Builtin),
    Function(FunctionId),
    Var(VarId),
    Module(ModuleId),
    TypeAlias(TypeAlias),
}
pub struct Resolve {
    modules: HashMap<ModuleId, ModuleInfo>,
    env: Scope,
    prev_envs: Vec<Scope>,
    functions: Vec<Option<res::Function>>,
    vars: usize,
    regions: usize,
    generics: usize,
    prev_kinds: Vec<HashMap<Rc<str>, GenericKind>>,
    generic_kinds: HashMap<Rc<str>, GenericKind>,
    lambdas: usize,
    diag: DiagnosticReporter,
}
impl Default for Resolve {
    fn default() -> Self {
        Self::new()
    }
}
impl Resolve {
    pub fn new() -> Self {
        let builtins: [(String, Builtin); Builtin::COUNT] = [
            (names::ALLOC_BOX.to_string(), Builtin::AllocBox),
            (names::DEALLOC_BOX.into(), Builtin::DeallocBox),
            (names::DEREF_BOX.into(), Builtin::DerefBox),
            (names::DEREF_BOX_MUT.into(), (Builtin::DerefBoxMut)),
            (names::FREEZE.into(), Builtin::Freeze),
            (names::REPLACE.into(), Builtin::Replace),
            (names::SWAP.into(), Builtin::Swap),
        ];
        let env = Scope::from_iter(
            builtins
                .into_iter()
                .map(|(name, builtin)| (name.into(), Res::Builtin(builtin)))
                .chain([("ptr".into(), Res::TypeAlias(TypeAlias::Ptr))]),
        );
        Self {
            modules: HashMap::new(),
            prev_envs: Vec::new(),
            env,
            vars: 0,
            regions: 0,
            functions: Vec::new(),
            generics: 0,
            diag: DiagnosticReporter::new(),
            generic_kinds: HashMap::new(),
            prev_kinds: Vec::new(),
            lambdas: 0,
        }
    }
    fn resolve_name(&self, name: &str) -> Option<Res> {
        if let Some(res) = self.env.get(name).copied() {
            return Some(res);
        }

        for env in self.prev_envs.iter().rev() {
            if let Some(res) = env.get(name).copied() {
                return Some(res);
            }
        }

        None
    }
    fn invalid_path_start_error(&mut self, path: &Path, loc: SrcLoc) {
        self.diag
            .add_diagnostic(format!("Invalid path '{}'", path.display()), loc);
    }
    fn path_not_in_scope_error(&mut self, path: &Path, loc: SrcLoc) {
        self.diag
            .add_diagnostic(format!("'{}' not in scope", path.display()), loc);
    }
    fn not_in_scope_error(&mut self, name: &str, loc: SrcLoc) {
        self.diag
            .add_diagnostic(format!("'{}' not in scope", name), loc);
    }
    fn cannot_use_as_error(&mut self, name: &str, expected: &str, loc: SrcLoc) {
        self.diag
            .add_diagnostic(format!("Cannot use '{}' as {}", name, expected), loc);
    }
    fn declare_function(&mut self, name: Ident) -> FunctionId {
        let function = FunctionId::new(self.functions.len());
        self.functions.push(None);
        match self.env.entry(name.content) {
            std::collections::hash_map::Entry::Occupied(mut occupied) => {
                if let Res::Function(_) = occupied.get() {
                    self.diag.add_diagnostic(
                        format!("Cannot redeclare function '{}'", occupied.key()),
                        name.loc.clone(),
                    );
                } else {
                    occupied.insert(Res::Function(function));
                }
            }
            std::collections::hash_map::Entry::Vacant(vacant) => {
                vacant.insert(Res::Function(function));
            }
        }
        function
    }
    fn resolve_region(&mut self, region: ast::Region) -> res::Region {
        match region {
            ast::Region::Named(name) => res::Region {
                loc: name.loc.clone(),
                kind: match self.resolve_name(&name.content) {
                    None => {
                        self.not_in_scope_error(&name.content, name.loc);
                        res::RegionKind::Unknown
                    }
                    Some(Res::LocalRegion(region)) => res::RegionKind::Local(name.content, region),
                    Some(Res::Param(index)) => {
                        if self
                            .generic_kinds
                            .insert(name.content.clone(), res::GenericKind::Region)
                            .is_some_and(|kind| kind != res::GenericKind::Region)
                        {
                            self.diag.add_diagnostic(
                                format!("Generic kind mismatch for '{}'", name.content),
                                name.loc,
                            );
                        }
                        res::RegionKind::Param(name.content, index)
                    }
                    Some(
                        Res::TypeAlias(_)
                        | Res::Builtin(_)
                        | Res::Function(_)
                        | Res::Var(_)
                        | Res::Module(_),
                    ) => {
                        self.cannot_use_as_error(&name.content, "region", name.loc);
                        res::RegionKind::Unknown
                    }
                },
            },
            ast::Region::Static(loc) => res::Region {
                loc,
                kind: res::RegionKind::Static,
            },
        }
    }
    fn get_generic_kind(&self, name: &str) -> Option<GenericKind> {
        self.generic_kinds.get(name).copied().or_else(|| {
            self.prev_kinds
                .iter()
                .rev()
                .find_map(|kinds| kinds.get(name).copied())
        })
    }
    fn resolve_generic_args(&mut self, args: Option<GenericArgs>) -> Vec<res::Type> {
        if let Some(args) = args {
            args.args
                .into_iter()
                .map(|arg| self.resolve_type(arg.ty))
                .collect()
        } else {
            Vec::new()
        }
    }
    fn resolve_generics<T>(
        &mut self,
        generics: ast::Generics,
        f: impl FnOnce(&mut Self) -> T,
    ) -> (res::Generics, T) {
        let names = generics
            .names
            .into_iter()
            .inspect(|param| {
                self.declare_param(param.content.clone());
            })
            .collect::<Vec<_>>();
        let old_kinds = std::mem::take(&mut self.generic_kinds);
        self.prev_kinds.push(old_kinds);
        let value = f(self);
        let kinds = names
            .iter()
            .map(|name| &name.content)
            .map(|name| self.get_generic_kind(name).unwrap_or(GenericKind::Type))
            .collect();
        if let Some(old_kinds) = self.prev_kinds.pop() {
            self.generic_kinds = old_kinds;
        }
        (
            res::Generics {
                loc: generics.loc,
                names,
                kinds,
            },
            value,
        )
    }
    fn resolve_type(&mut self, ty: ast::Type) -> res::Type {
        let kind = match ty.kind {
            ast::TypeKind::Char => res::TypeKind::Char,
            ast::TypeKind::Bool => res::TypeKind::Bool,
            ast::TypeKind::Int => res::TypeKind::Int,
            ast::TypeKind::Unit => res::TypeKind::Unit,
            ast::TypeKind::String => res::TypeKind::String,
            ast::TypeKind::Box(ty) => res::TypeKind::Box(Box::new(self.resolve_type(*ty))),
            ast::TypeKind::Option(ty) => res::TypeKind::Option(Box::new(self.resolve_type(*ty))),
            ast::TypeKind::List(ty) => res::TypeKind::List(Box::new(self.resolve_type(*ty))),
            ast::TypeKind::Record(ast::RecordType { fields }) => {
                let fields = fields
                    .into_iter()
                    .map(|ast::RecordField { name, ty }| res::RecordFieldType {
                        name,
                        ty: self.resolve_type(ty),
                    })
                    .collect();
                res::TypeKind::Record(fields)
            }
            ast::TypeKind::Function(ast::FunctionType {
                resource,
                params,
                return_type,
            }) => res::TypeKind::Function(
                resource,
                params.into_iter().map(|ty| self.resolve_type(ty)).collect(),
                Box::new(self.resolve_type(*return_type)),
            ),
            ast::TypeKind::Imm(region, ty) => res::TypeKind::Imm(
                self.resolve_region(region),
                Box::new(self.resolve_type(*ty)),
            ),
            ast::TypeKind::Mut(region, ty) => res::TypeKind::Mut(
                self.resolve_region(region),
                Box::new(self.resolve_type(*ty)),
            ),
            ast::TypeKind::Named(name, args) => match self.resolve_name(&name.content) {
                None => {
                    self.resolve_generic_args(args);
                    self.not_in_scope_error(&name.content, name.loc);
                    res::TypeKind::Unknown
                }
                Some(Res::Param(index)) => {
                    if self
                        .generic_kinds
                        .insert(name.content.clone(), res::GenericKind::Type)
                        .is_some_and(|kind| kind != res::GenericKind::Type)
                    {
                        self.diag.add_diagnostic(
                            format!("Generic kind mismatch for '{}'", name.content),
                            name.loc.clone(),
                        );
                    }
                    if args.is_some() {
                        self.diag.add_diagnostic(
                            format!(
                                "Generic param '{}' cannot have generic arguments",
                                name.content
                            ),
                            name.loc,
                        );
                    }
                    self.resolve_generic_args(args);
                    res::TypeKind::Param(name.content, index)
                }
                Some(Res::TypeAlias(alias)) => match alias {
                    TypeAlias::Ptr => {
                        let args = self.resolve_generic_args(args);
                        let arg: Result<[_; 1], _> = args.try_into();
                        let ty = match arg {
                            Ok([arg]) => arg,
                            Err(args) => {
                                self.diag.add_diagnostic(
                                    format!(
                                        "Expected '{}' generic arg but got '{}'",
                                        1,
                                        args.len()
                                    ),
                                    name.loc.clone(),
                                );
                                res::Type {
                                    loc: name.loc.clone(),
                                    kind: res::TypeKind::Unknown,
                                }
                            }
                        };
                        res::TypeKind::Ptr(Box::new(ty))
                    }
                },
                Some(
                    Res::Builtin(_)
                    | Res::Function(_)
                    | Res::LocalRegion(_)
                    | Res::Var(_)
                    | Res::Module(_),
                ) => {
                    if args.is_some() {
                        self.diag.add_diagnostic(
                            format!("'{}' cannot have generic arguments", name.content),
                            name.loc.clone(),
                        );
                    }
                    self.resolve_generic_args(args);
                    self.cannot_use_as_error(&name.content, "type", name.loc);
                    res::TypeKind::Unknown
                }
            },
        };
        res::Type { loc: ty.loc, kind }
    }
    fn declare_region(&mut self, region: Rc<str>) -> LocalRegionId {
        let region_id = LocalRegionId::new(self.vars);
        self.regions += 1;
        self.env.insert(region, Res::LocalRegion(region_id));
        region_id
    }
    fn next_lambda_id(&mut self) -> LambdaId {
        let id = LambdaId::new(self.lambdas);
        self.lambdas += 1;
        id
    }
    fn declare_var(&mut self, var: Rc<str>) -> VarId {
        let var_id = VarId::new(self.vars);
        self.vars += 1;
        self.env.insert(var, Res::Var(var_id));
        var_id
    }
    fn declare_module(&mut self, id: ModuleId, name: Rc<str>) {
        self.modules.insert(
            id,
            ModuleInfo {
                env: Default::default(),
            },
        );
        self.env.insert(name, Res::Module(id));
    }
    fn declare_param(&mut self, name: Rc<str>) -> usize {
        let generic = self.generics;
        self.generics += 1;
        self.env.insert(name, Res::Param(generic));
        generic
    }
    fn in_scope<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.prev_envs.push(std::mem::take(&mut self.env));
        let value = f(self);
        self.env = self
            .prev_envs
            .pop()
            .expect("There should be a pushed scope");
        value
    }
    fn in_module_scope<T>(&mut self, module: ModuleId, f: impl FnOnce(&mut Self) -> T) -> T {
        self.prev_envs.push({
            let old_env = std::mem::take(&mut self.env);
            self.env.clone_from(&self.modules[&module].env);
            old_env
        });
        let value = f(self);
        self.env = self
            .prev_envs
            .pop()
            .expect("There should be a pushed scope");
        value
    }
    fn take_new_scope(&mut self, f: impl FnOnce(&mut Self)) -> Scope {
        self.prev_envs.push(std::mem::take(&mut self.env));
        f(self);
        std::mem::replace(
            &mut self.env,
            self.prev_envs
                .pop()
                .expect("There should be a pushed scope"),
        )
    }
    fn path_error(&mut self, path: &ast::Path, loc: SrcLoc, error: NameResolutionError) {
        match error {
            NameResolutionError::NotInScope => {
                self.path_not_in_scope_error(path, loc);
            }
            NameResolutionError::InvalidPathStart => {
                self.invalid_path_start_error(path, loc);
            }
        }
    }
    fn resolve_pattern(&mut self, pattern: ast::Pattern) -> res::Pattern {
        let loc = pattern.loc;
        let kind = match pattern.kind {
            ast::PatternKind::Int(value) => res::PatternKind::Int(match value.try_into() {
                Ok(number) => number,
                Err(_) => {
                    self.diag
                        .add_diagnostic("Invalid integer".to_string(), loc.clone());
                    0
                }
            }),
            ast::PatternKind::Bool(value) => res::PatternKind::Bool(value),
            ast::PatternKind::None => res::PatternKind::None,
            ast::PatternKind::Some(pattern) => {
                res::PatternKind::Some(Box::new(self.resolve_pattern(*pattern)))
            }
            ast::PatternKind::Binding(borrow, mutable, name) => {
                let var = self.declare_var(name.content.clone());
                res::PatternKind::Binding(borrow, mutable, name, var)
            }
            ast::PatternKind::Record(fields) => {
                let fields = fields
                    .into_iter()
                    .map(|field| res::PatternField {
                        name: field.name,
                        pattern: self.resolve_pattern(field.pattern),
                    })
                    .collect();
                res::PatternKind::Record(fields)
            }
            ast::PatternKind::Ref(pattern) => {
                res::PatternKind::Ref(Box::new(self.resolve_pattern(*pattern)))
            }
        };
        res::Pattern { loc, kind }
    }
    fn resolve_let_binding(&mut self, let_binding: ast::LetBinding) -> res::LetBinding {
        let value = self.resolve_expr(let_binding.value);
        let ty = let_binding.ty.map(|ty| self.resolve_type(ty));
        let pattern = self.resolve_pattern(let_binding.pattern);
        res::LetBinding { pattern, ty, value }
    }
    fn resolve_place(&mut self, place: ast::Expr) -> Option<res::Place> {
        let loc = place.loc;
        match place.kind {
            ast::ExprKind::Deref(expr) => Some(res::Place {
                loc,
                kind: res::PlaceKind::Deref(Box::new(self.resolve_expr(*expr))),
            }),
            ast::ExprKind::Path(path) => {
                let kind = match self.resolve_path(&path) {
                    Err(err) => {
                        self.path_error(&path, loc, err);
                        return None;
                    }
                    Ok(Res::Var(var)) => {
                        res::PlaceKind::Var(res::Var(path.expect_head().content, var))
                    }
                    Ok(
                        Res::Builtin(_)
                        | Res::Function(..)
                        | Res::Param(_)
                        | Res::LocalRegion(_)
                        | Res::Module(_)
                        | Res::TypeAlias(_),
                    ) => {
                        self.diag.add_diagnostic(
                            format!("Can't use '{}' as place", path.display()),
                            path.expect_head().loc.clone(),
                        );
                        return None;
                    }
                };
                Some(res::Place { loc, kind })
            }
            _ => {
                self.diag
                    .add_diagnostic("Invalid place".to_string(), loc.clone());
                None
            }
        }
    }
    fn resolve_stmt(&mut self, stmt: ast::Stmt) -> res::Stmt {
        let loc = stmt.loc;
        match stmt.kind {
            StmtKind::Let(let_binding) => {
                let let_binding = self.resolve_let_binding(let_binding);
                res::Stmt {
                    loc,
                    kind: res::StmtKind::Let(Box::new(let_binding)),
                }
            }
            StmtKind::Expr(expr) => {
                let expr = self.resolve_expr(expr);
                res::Stmt {
                    loc,
                    kind: res::StmtKind::Expr(expr),
                }
            }
        }
    }

    fn resolve_path(&mut self, path: &Path) -> Result<Res, NameResolutionError> {
        let Some(head) = self.resolve_name(&path.head().content) else {
            return Err(NameResolutionError::NotInScope);
        };
        let mut curr = head;
        for segment in path.segments_iter().into_iter().skip(1) {
            curr = match curr {
                Res::Module(module) => {
                    let env = &self.modules[&module].env;
                    if let Some(&res) = env.get(&segment.content) {
                        res
                    } else {
                        return Err(NameResolutionError::NotInScope);
                    }
                }
                Res::Builtin(_)
                | Res::Function(_)
                | Res::Param(_)
                | Res::Var(_)
                | Res::LocalRegion(_)
                | Res::TypeAlias(_) => return Err(NameResolutionError::InvalidPathStart),
            }
        }
        Ok(curr)
    }
    fn resolve_expr(&mut self, expr: ast::Expr) -> res::Expr {
        let loc = expr.loc;
        let kind = match expr.kind {
            ast::ExprKind::Block(block, region) => self.in_scope(|this| {
                let region = region.map(|region| this.declare_region(region.content));
                res::ExprKind::Block(
                    res::BlockBody {
                        stmts: block
                            .stmts
                            .into_iter()
                            .map(|stmt| this.resolve_stmt(stmt))
                            .collect(),
                        expr: Box::new(this.resolve_expr(*block.expr)),
                    },
                    region,
                )
            }),

            ast::ExprKind::Unit => res::ExprKind::Unit,
            ast::ExprKind::String(value) => res::ExprKind::String(value.into()),
            ast::ExprKind::Number(value) => res::ExprKind::Int(value as i64),
            ast::ExprKind::Bool(value) => res::ExprKind::Bool(value),
            ast::ExprKind::None(ty) => res::ExprKind::None(ty.map(|ty| self.resolve_type(ty))),
            ast::ExprKind::Some(arg) => res::ExprKind::Some(Box::new(self.resolve_expr(*arg))),
            ast::ExprKind::Print(arg) => {
                res::ExprKind::Print(arg.map(|arg| Box::new(self.resolve_expr(*arg))))
            }
            ast::ExprKind::Annotate(expr, ty) => res::ExprKind::Annotate(
                Box::new(self.resolve_expr(*expr)),
                Box::new(self.resolve_type(*ty)),
            ),
            ast::ExprKind::Panic(ty) => res::ExprKind::Panic(ty.map(|ty| self.resolve_type(ty))),
            ast::ExprKind::Call(callee, args) => res::ExprKind::Call(
                Box::new(self.resolve_expr(*callee)),
                args.into_iter().map(|arg| self.resolve_expr(arg)).collect(),
            ),
            ast::ExprKind::List(values) => res::ExprKind::List(
                values
                    .into_iter()
                    .map(|value| self.resolve_expr(value))
                    .collect(),
            ),
            ast::ExprKind::Deref(value) => {
                res::ExprKind::Deref(Box::new(self.resolve_expr(*value)))
            }
            ast::ExprKind::Binary(op, left, right) => res::ExprKind::Binary(
                op,
                Box::new(self.resolve_expr(*left)),
                Box::new(self.resolve_expr(*right)),
            ),
            ast::ExprKind::Record(ast::RecordExpr { fields }) => res::ExprKind::Record(
                fields
                    .into_iter()
                    .map(|field| res::FieldInit {
                        name: field.name,
                        value: self.resolve_expr(field.value),
                    })
                    .collect(),
            ),
            ast::ExprKind::Path(path) => match self.resolve_path(&path) {
                Err(error) => {
                    match error {
                        NameResolutionError::NotInScope => {
                            self.path_not_in_scope_error(&path, loc.clone());
                        }
                        NameResolutionError::InvalidPathStart => {
                            self.invalid_path_start_error(&path, loc.clone());
                        }
                    }
                    res::ExprKind::Err
                }
                Ok(res) => match res {
                    Res::Builtin(builtin) => res::ExprKind::Builtin(builtin),
                    Res::Var(id) => res::ExprKind::Var(path.into_last().content, id),
                    Res::Function(function) => {
                        res::ExprKind::Function(path.into_last().content, function)
                    }
                    Res::Param(_) | Res::LocalRegion(_) | Res::Module(_) | Res::TypeAlias(_) => {
                        self.diag.add_diagnostic(
                            format!("Can't use '{}' as a value", path.display()),
                            loc.clone(),
                        );
                        res::ExprKind::Err
                    }
                },
            },
            ast::ExprKind::Lambda(lambda) => self.in_scope(|this| {
                res::ExprKind::Lambda(Box::new(res::Lambda {
                    id: this.next_lambda_id(),
                    params: lambda
                        .params
                        .into_iter()
                        .map(|(name, ty)| {
                            let var = this.declare_var(name.content.clone());
                            let ty = ty.map(|ty| this.resolve_type(ty));
                            (name, var, ty)
                        })
                        .collect(),
                    resource: lambda.resource,
                    body: this.resolve_expr(*lambda.body),
                }))
            }),
            ast::ExprKind::Assign(place, value) => {
                let place = self.resolve_place(*place);
                let value = self.resolve_expr(*value);
                let Some(place) = place else {
                    return res::Expr {
                        loc: loc.clone(),
                        kind: res::ExprKind::Err,
                    };
                };
                res::ExprKind::Assign(place, Box::new(value))
            }
            ast::ExprKind::Borrow(borrow_expr) => {
                let BorrowExpr {
                    mutable,
                    expr,
                    region,
                } = *borrow_expr;
                let place = self.resolve_place(expr);
                let region = self.resolve_region(region);
                let Some(place) = place else {
                    return res::Expr {
                        loc: loc.clone(),
                        kind: res::ExprKind::Err,
                    };
                };
                res::ExprKind::Borrow(Box::new(res::BorrowExpr {
                    mutable,
                    place,
                    region,
                }))
            }
            ast::ExprKind::Case(matched, arms) => {
                let matched = self.resolve_expr(*matched);
                let arms = arms
                    .into_iter()
                    .map(|arm| {
                        let pattern = self.resolve_pattern(arm.pat);
                        let body = self.resolve_expr(arm.body);
                        res::CaseArm { pattern, body }
                    })
                    .collect();
                res::ExprKind::Case(Box::new(matched), arms)
            }
            ast::ExprKind::For(pattern, iterator, body) => {
                let iterator = self.resolve_expr(*iterator);
                self.in_scope(|this| {
                    let pattern = this.resolve_pattern(*pattern);
                    let body = this.resolve_expr(*body);
                    res::ExprKind::For(pattern, Box::new(iterator), Box::new(body))
                })
            }
        };
        res::Expr { loc, kind }
    }
    fn resolve_signature(
        &mut self,
        params: Vec<ast::Param>,
        return_type: ast::Type,
    ) -> (Vec<res::Param>, res::Type) {
        let params = params
            .into_iter()
            .map(|param| {
                let var = self.declare_var(param.name.content.clone());
                res::Param {
                    loc: param.name.loc,
                    var: res::Var(param.name.content, var),
                    ty: self.resolve_type(param.ty),
                }
            })
            .collect::<Vec<_>>();
        let return_type = self.resolve_type(return_type);
        (params, return_type)
    }
    fn resolve_function(&mut self, function: ast::Function) -> res::Function {
        let function = self.in_scope(|this| {
            let (generics, (params, return_type)) = if let Some(generics) = function.generics {
                let (generics, sig) = this.resolve_generics(generics, |this| {
                    this.resolve_signature(function.params, function.return_type)
                });
                (Some(generics), sig)
            } else {
                (
                    None,
                    this.resolve_signature(function.params, function.return_type),
                )
            };
            let body = function.body.map(|body| this.resolve_expr(body));
            res::Function {
                loc: function.loc,
                name: function.name,
                generics,
                params,
                return_type,
                body,
            }
        });
        self.generics = 0;
        self.vars = 0;
        function
    }
    fn declare_module_items(&mut self, module: &ast::Module) {
        self.declare_module(module.id, module.name.clone());
        let scope = self.take_new_scope(|this| {
            for function in &module.functions {
                this.declare_function(function.name.clone());
            }
            for module in &module.child_modules {
                this.declare_module_items(module);
            }
        });
        self.modules.get_mut(&module.id).unwrap().env.extend(scope);
    }
    fn declare(&mut self, modules: &[ast::Module]) {
        for module in modules.iter() {
            self.declare_module_items(module);
        }
    }
    fn resolve_module(
        &mut self,
        functions: &mut IndexVec<FunctionId, res::Function>,
        module: ast::Module,
    ) {
        self.in_module_scope(module.id, |this| {
            for function in module.functions {
                functions.push(this.resolve_function(function));
            }
            for child in module.child_modules {
                this.resolve_module(functions, child);
            }
        });
    }
    pub fn resolve(mut self, modules: Vec<ast::Module>) -> Result<res::Program, ResolveErrored> {
        //First pass : Declare everything
        self.declare(&modules);
        //Second pass : Resolve
        let mut functions = IndexVec::new();
        for module in modules.into_iter() {
            self.resolve_module(&mut functions, module);
        }
        let program = res::Program { functions };
        if !self.diag.report_all() {
            Ok(program)
        } else {
            Err(ResolveErrored)
        }
    }
}
