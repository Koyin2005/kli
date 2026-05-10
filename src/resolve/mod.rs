use std::collections::HashMap;

use crate::ast::Ident;
use crate::diagnostics::DiagnosticReporter;
use crate::resolved_ast::{Builtin, FunctionId, GenericKind, LocalRegionId, VarId};
use crate::{ast, names, resolved_ast as res};

#[derive(Clone, Copy, Debug)]
pub(super) enum Res {
    LocalRegion(LocalRegionId),
    Param(usize),
    Builtin(Builtin),
    Function(FunctionId),
    Var(VarId),
}
pub struct Resolve {
    env: HashMap<String, Res>,
    prev_envs: Vec<HashMap<String, Res>>,
    functions: Vec<Option<res::Function>>,
    binder: usize,
    binder_start: usize,
    vars: usize,
    regions: usize,
    generics: usize,
    prev_kinds: Vec<HashMap<String, GenericKind>>,
    generic_kinds: HashMap<String, GenericKind>,
    diag: DiagnosticReporter,
}
impl Default for Resolve {
    fn default() -> Self {
        Self::new()
    }
}
impl Resolve {
    pub fn new() -> Self {
        let env = HashMap::from([
            (
                names::ALLOC_BOX.to_string(),
                Res::Builtin(Builtin::AllocBox),
            ),
            (
                names::DEALLOC_BOX.to_string(),
                Res::Builtin(Builtin::DeallocBox),
            ),
            (
                names::DEREF_BOX.to_string(),
                Res::Builtin(Builtin::DerefBox),
            ),
            (
                names::DEREF_BOX_MUT.to_string(),
                Res::Builtin(Builtin::DerefBoxMut),
            ),
            (
                names::DESTROY_LIST.to_string(),
                Res::Builtin(Builtin::DestroyList),
            ),
            (names::FREEZE.to_string(), Res::Builtin(Builtin::Freeze)),
            (
                names::DESTROY_STRING.to_string(),
                Res::Builtin(Builtin::DestroyString),
            ),
            (names::REPLACE.to_string(), Res::Builtin(Builtin::Replace)),
            (names::SWAP.to_string(), Res::Builtin(Builtin::Swap)),
        ]);
        Self {
            prev_envs: Vec::new(),
            env,
            vars: 0,
            regions: 0,
            functions: Vec::new(),
            generics: 0,
            binder: 0,
            binder_start: 0,
            diag: DiagnosticReporter::new(),
            generic_kinds: HashMap::new(),
            prev_kinds: Vec::new(),
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
    fn not_in_scope_error(&mut self, name: &str, line: usize) {
        self.diag.report(format!("'{}' not in scope", name), line);
    }
    fn cannot_use_as_error(&mut self, name: &str, expected: &str, line: usize) {
        self.diag
            .report(format!("Cannot use '{}' as {}", name, expected), line);
    }
    fn declare_function(&mut self, name: Ident) -> FunctionId {
        let function = FunctionId::new(self.functions.len());
        self.functions.push(None);
        match self.env.entry(name.content) {
            std::collections::hash_map::Entry::Occupied(mut occupied) => {
                if let Res::Function(_) = occupied.get() {
                    self.diag.report(
                        format!("Cannot redeclare function '{}'", occupied.key()),
                        name.line,
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
                line: name.line,
                kind: match self.resolve_name(&name.content) {
                    None => {
                        self.not_in_scope_error(&name.content, name.line);
                        res::RegionKind::Unknown
                    }
                    Some(Res::LocalRegion(region)) => res::RegionKind::Local(name.content, region),
                    Some(Res::Param(index)) => {
                        if self
                            .generic_kinds
                            .insert(name.content.clone(), res::GenericKind::Region)
                            .is_some_and(|kind| kind != res::GenericKind::Region)
                        {
                            self.diag.report(
                                format!("Generic kind mismatch for '{}'", name.content),
                                name.line,
                            );
                        }
                        if let Some(new_index) = index.checked_sub(self.binder_start)
                            && self.binder > 0
                        {
                            res::RegionKind::BoundParam(name.content, new_index, self.binder)
                        } else {
                            res::RegionKind::Param(name.content, index)
                        }
                    }
                    Some(Res::Builtin(_) | Res::Function(_) | Res::Var(_)) => {
                        self.cannot_use_as_error(&name.content, "region", name.line);
                        res::RegionKind::Unknown
                    }
                },
            },
            ast::Region::Static(line) => res::Region {
                line,
                kind: res::RegionKind::Static,
            },
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
        let old_kinds = std::mem::replace(&mut self.generic_kinds, HashMap::new());
        self.prev_kinds.push(old_kinds);
        let value = f(self);
        fn get_generic_kind(this: &Resolve, name: &str) -> Option<GenericKind> {
            this.generic_kinds.get(name).copied().or_else(|| {
                this.prev_kinds
                    .iter()
                    .rev()
                    .find_map(|kinds| kinds.get(name).copied())
            })
        }
        let kinds = names
            .iter()
            .map(|name| &name.content)
            .map(|name| get_generic_kind(self, name).unwrap_or(GenericKind::Type))
            .collect();
        if let Some(old_kinds) = self.prev_kinds.pop() {
            self.generic_kinds = old_kinds;
        }
        (
            res::Generics {
                line: generics.line,
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
            ast::TypeKind::Function(
                generics,
                ast::FunctionType {
                    resource,
                    params,
                    return_type,
                },
            ) => {
                if let Some(generics) = generics
                    && !generics.names.is_empty()
                {
                    let new_binder = self.binder + 1;
                    let old_binder = std::mem::replace(&mut self.binder, new_binder);
                    let old_binder_start = std::mem::replace(&mut self.binder_start, self.generics);
                    let (generics, (params, return_type)) =
                        self.resolve_generics(generics, |this| {
                            (
                                params
                                    .into_iter()
                                    .map(|param| this.resolve_type(param))
                                    .collect(),
                                this.resolve_type(*return_type),
                            )
                        });
                    if let Some(index) = generics.kinds.iter().position(|kind| *kind != GenericKind::Region){
                        let name = &generics.names[index];
                        let line = name.line;
                        let msg = format!("Cannot use type '{}' with forall",name.content);
                        self.diag.report(msg, line);
                    }
                    self.binder_start = old_binder_start;
                    self.binder = old_binder;
                    res::TypeKind::Function(
                        Some((new_binder, generics)),
                        resource,
                        params,
                        Box::new(return_type),
                    )
                } else {
                    res::TypeKind::Function(
                        None,
                        resource,
                        params.into_iter().map(|ty| self.resolve_type(ty)).collect(),
                        Box::new(self.resolve_type(*return_type)),
                    )
                }
            }
            ast::TypeKind::Imm(region, ty) => res::TypeKind::Imm(
                self.resolve_region(region),
                Box::new(self.resolve_type(*ty)),
            ),
            ast::TypeKind::Mut(region, ty) => res::TypeKind::Mut(
                self.resolve_region(region),
                Box::new(self.resolve_type(*ty)),
            ),
            ast::TypeKind::Named(name) => match self.resolve_name(&name.content) {
                None => {
                    self.not_in_scope_error(&name.content, name.line);
                    res::TypeKind::Unknown
                }
                Some(Res::Param(index)) => {
                    if self
                        .generic_kinds
                        .insert(name.content.clone(), res::GenericKind::Type)
                        .is_some_and(|kind| kind != res::GenericKind::Type)
                    {
                        self.diag.report(
                            format!("Generic kind mismatch for '{}'", name.content),
                            name.line,
                        );
                    }
                    if self.binder > 0 && index.checked_sub(self.binder_start).is_some(){
                        res::TypeKind::Unknown
                    }
                    else{
                        res::TypeKind::Param(name.content, index)
                    }
                }
                Some(Res::Builtin(_) | Res::Function(_) | Res::LocalRegion(_) | Res::Var(_)) => {
                    self.cannot_use_as_error(&name.content, "type", name.line);
                    res::TypeKind::Unknown
                }
            },
        };
        res::Type {
            line: ty.line,
            kind,
        }
    }
    fn declare_region(&mut self, region: String) -> LocalRegionId {
        let region_id = LocalRegionId::new(self.vars);
        self.regions += 1;
        self.env.insert(region, Res::LocalRegion(region_id));
        region_id
    }
    fn declare_var(&mut self, var: String) -> VarId {
        let var_id = VarId::new(self.vars);
        self.vars += 1;
        self.env.insert(var, Res::Var(var_id));
        var_id
    }
    fn declare_param(&mut self, name: String) -> usize {
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
    fn resolve_pattern(&mut self, pattern: ast::Pattern) -> res::Pattern {
        let line = pattern.line;
        let kind = match pattern.kind {
            ast::PatternKind::Bool(value) => res::PatternKind::Bool(value),
            ast::PatternKind::None => res::PatternKind::None,
            ast::PatternKind::Deref(pattern) => {
                res::PatternKind::Deref(Box::new(self.resolve_pattern(*pattern)))
            }
            ast::PatternKind::Some(pattern) => {
                res::PatternKind::Some(Box::new(self.resolve_pattern(*pattern)))
            }
            ast::PatternKind::Binding(mutable, name, region) => {
                let var = self.declare_var(name.content.clone());
                res::PatternKind::Binding(
                    mutable,
                    name,
                    var,
                    region.map(|region| self.resolve_region(region)),
                )
            }
        };
        res::Pattern { line, kind }
    }
    fn resolve_let_expr(&mut self, let_expr: ast::LetExpr) -> res::LetExpr {
        let binder = self.resolve_expr(let_expr.binder);
        let ty = let_expr.ty.map(|ty| self.resolve_type(ty));
        self.in_scope(|this| {
            let pattern = this.resolve_pattern(let_expr.pattern);
            let body = this.resolve_expr(let_expr.body);
            res::LetExpr {
                pattern,
                ty,
                body,
                binder,
            }
        })
    }
    fn resolve_place(&mut self, place: ast::Place) -> Option<res::Place> {
        match place {
            ast::Place::Deref(expr, line) => Some(res::Place {
                line,
                kind: res::PlaceKind::Deref(Box::new(self.resolve_expr(*expr))),
            }),
            ast::Place::Ident(name) => Some(res::Place {
                line: name.line,
                kind: match self.resolve_name(&name.content) {
                    None => {
                        self.not_in_scope_error(&name.content, name.line);
                        return None;
                    }
                    Some(Res::Var(var)) => res::PlaceKind::Var(res::Var(name.content, var)),
                    Some(
                        Res::Builtin(_) | Res::Function(..) | Res::Param(_) | Res::LocalRegion(_),
                    ) => {
                        self.diag
                            .report(format!("Can't use '{}' as place", name.content), name.line);
                        return None;
                    }
                },
            }),
        }
    }
    fn resolve_expr(&mut self, expr: ast::Expr) -> res::Expr {
        let line = expr.line;
        let kind = match expr.kind {
            ast::ExprKind::Instantiate(expr) => {
                res::ExprKind::Instantiate(Box::new(self.resolve_expr(*expr)))
            }
            ast::ExprKind::Unit => res::ExprKind::Unit,
            ast::ExprKind::String(value) => res::ExprKind::String(value),
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
            ast::ExprKind::Sequence(first, second) => res::ExprKind::Sequence(
                Box::new(self.resolve_expr(*first)),
                Box::new(self.resolve_expr(*second)),
            ),
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
            ast::ExprKind::Ident(name) => match self.resolve_name(&name.content) {
                None => {
                    self.not_in_scope_error(&name.content, name.line);
                    res::ExprKind::Err
                }
                Some(res) => match res {
                    Res::Builtin(builtin) => res::ExprKind::Builtin(builtin),
                    Res::Var(id) => res::ExprKind::Var(name.content, id),
                    Res::Function(function) => res::ExprKind::Function(name.content, function),
                    Res::Param(_) | Res::LocalRegion(_) => {
                        self.diag
                            .report(format!("Can't use '{}' as a value", name.content), line);
                        res::ExprKind::Err
                    }
                },
            },
            ast::ExprKind::Let(let_expr) => {
                res::ExprKind::Let(Box::new(self.resolve_let_expr(*let_expr)))
            }
            ast::ExprKind::Lambda(lambda) => self.in_scope(|this| {
                res::ExprKind::Lambda(Box::new(res::Lambda {
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
                let place = self.resolve_place(place);
                let value = self.resolve_expr(*value);
                let Some(place) = place else {
                    return res::Expr {
                        line,
                        kind: res::ExprKind::Err,
                    };
                };
                res::ExprKind::Assign(place, Box::new(value))
            }
            ast::ExprKind::Borrow(mutable, var_name, region_name, body) => {
                let (body, new_var, var, region) = self.in_scope(|this| {
                    let region = this.declare_region(region_name.content.clone());
                    let var = match this.resolve_name(&var_name.content) {
                        None => {
                            this.not_in_scope_error(&var_name.content, var_name.line);
                            None
                        }
                        Some(Res::Var(var)) => Some(var),
                        Some(_) => {
                            this.cannot_use_as_error(&var_name.content, "variable", var_name.line);
                            None
                        }
                    };
                    let new_var = this.declare_var(var_name.content.clone());
                    let body = this.resolve_expr(*body);
                    (body, new_var, var, region)
                });
                match var {
                    None => res::ExprKind::Err,
                    Some(var) => res::ExprKind::Borrow(Box::new(res::BorrowExpr {
                        mutable,
                        var_name,
                        old_var: var,
                        new_var,
                        region_name,
                        region,
                        body,
                    })),
                }
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
                    let pattern = this.resolve_pattern(pattern);
                    let body = this.resolve_expr(*body);
                    res::ExprKind::For(pattern, Box::new(iterator), Box::new(body))
                })
            }
        };
        res::Expr { line, kind }
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
                    line: param.name.line,
                    var: res::Var(param.name.content, var),
                    ty: self.resolve_type(param.ty),
                }
            })
            .collect::<Vec<_>>();
        let return_type = self.resolve_type(return_type);
        (params, return_type)
    }
    pub fn resolve(mut self, program: ast::Program) -> res::Program {
        for function in &program.functions {
            self.declare_function(function.name.clone());
        }
        let program = res::Program {
            functions: (0..program.functions.len())
                .map(FunctionId::new)
                .zip(program.functions)
                .map(|(_, function)| {
                    let function = self.in_scope(|this| {
                        let (generics, (params, return_type)) =
                            if let Some(generics) = function.generics {
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
                        let body = this.resolve_expr(function.body);
                        res::Function {
                            line: function.line,
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
                })
                .collect(),
        };
        self.diag.finish();
        program
    }
}
