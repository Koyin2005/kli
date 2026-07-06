use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{BorrowExpr, GenericArgs, ModuleId, NodeId, Path, StmtKind};
use crate::builtins::{Builtin, Builtins};
use crate::collect::{GlobalContext, build_global_context};
use crate::def_ids::DefId;
use crate::diagnostics::DiagnosticReporter;
use crate::ident::Ident;
use crate::index_vec::IndexVec;
use crate::lang_items::LangItem;
use crate::resolved_ast::{GenericKind, LocalRegionId, VarId, VariantDef};
use crate::src_loc::SrcLoc;
use crate::{Symbol, ast, resolved_ast as res};

pub struct ResolveErrored;

enum NameResolutionError {
    NotInScope,
    InvalidPathStart,
    VariableField(SrcLoc, res::Var, Vec<Ident>),
}
struct ModuleInfo {
    env: Scope,
    id: DefId,
}

#[derive(Clone, Copy, Debug)]
enum TypeAlias {
    Ptr,
    Byte,
    Box,
    ArrayList,
}
type Scope = HashMap<Symbol, Res>;

#[derive(Clone, Copy, Debug)]
enum Res {
    LocalRegion(LocalRegionId),
    Param(usize),
    Builtin(Builtin),
    Function(ModuleNodeId),
    Var(VarId),
    Module(ModuleId),
    TypeAlias(TypeAlias),
    TypeDef(ModuleNodeId),
    VariantCase(ModuleNodeId),
}
#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
struct ModuleNodeId(ModuleId, NodeId);
struct CaseInfo {
    pub name: Ident,
    pub id: NodeId,
}
struct FieldInfo {
    _name: Ident,
    _id: NodeId,
}
enum TypeDefInfo {
    Variant {
        cases: Vec<CaseInfo>,
        case_map: HashMap<Symbol, usize>,
    },
    Record {
        _fields: Vec<FieldInfo>,
    },
}

pub struct Resolve {
    parents: HashMap<DefId, DefId>,
    modules: HashMap<ModuleId, ModuleInfo>,
    env: Scope,
    prev_envs: Vec<Scope>,
    type_defs: HashMap<ModuleNodeId, TypeDefInfo>,
    vars: usize,
    regions: usize,
    generics: usize,
    prev_kinds: Vec<HashMap<Symbol, GenericKind>>,
    generic_kinds: HashMap<Symbol, GenericKind>,
    item_id_to_def_id: HashMap<ModuleNodeId, DefId>,
    current_item: Option<DefId>,
    current_module: Option<ModuleId>,
    builtin_module: Option<ModuleId>,
    diag: DiagnosticReporter,
    nodes: IndexVec<DefId, Option<res::Node>>,
}
impl Default for Resolve {
    fn default() -> Self {
        Self::new()
    }
}
impl Resolve {
    pub fn new() -> Self {
        let builtins = Builtin::ALL_BUILTINS
            .map(|builtin| (Symbol::intern(builtin.name()), Res::Builtin(builtin)));
        let env = Scope::from_iter(builtins.into_iter().chain([
            (Symbol::intern("ptr"), Res::TypeAlias(TypeAlias::Ptr)),
            (Symbol::intern("byte"), Res::TypeAlias(TypeAlias::Byte)),
            //FIXME : Make box Box instead
            (Symbol::intern("box"), Res::TypeAlias(TypeAlias::Box)),
            (
                Symbol::intern("ArrayList"),
                Res::TypeAlias(TypeAlias::ArrayList),
            ),
        ]));
        Self {
            parents: HashMap::new(),
            item_id_to_def_id: HashMap::new(),
            modules: HashMap::new(),
            prev_envs: Vec::new(),
            env,
            vars: 0,
            regions: 0,
            generics: 0,
            diag: DiagnosticReporter::new(),
            generic_kinds: HashMap::new(),
            prev_kinds: Vec::new(),
            type_defs: HashMap::new(),
            current_item: None,
            builtin_module: None,
            nodes: IndexVec::new(),
            current_module: None,
        }
    }
    fn resolve_name(&self, name: Symbol) -> Option<Res> {
        if let Some(res) = self.env.get(&name).copied() {
            return Some(res);
        }

        for env in self.prev_envs.iter().rev() {
            if let Some(res) = env.get(&name).copied() {
                return Some(res);
            }
        }

        None
    }
    fn invalid_path_start_error(&mut self, path: &Path, loc: SrcLoc) {
        self.diag
            .add_diagnostic(format!("Invalid path '{}'", path), loc);
    }
    fn path_not_in_scope_error(&mut self, path: &Path, loc: SrcLoc) {
        self.diag
            .add_diagnostic(format!("'{}' not in scope", path), loc);
    }
    fn not_in_scope_error(&mut self, name: Symbol, loc: SrcLoc) {
        self.diag
            .add_diagnostic(format!("'{}' not in scope", name), loc);
    }
    fn builtin_not_found_error(&mut self, builtin: Builtin, loc: SrcLoc) {
        self.diag
            .add_diagnostic(format!("builtin '{}' not found", builtin.name()), loc);
    }
    fn cannot_use_as_error(&mut self, name: Symbol, expected: &str, loc: SrcLoc) {
        self.diag
            .add_diagnostic(format!("Cannot use '{}' as {}", name, expected), loc);
    }
    fn add_node(&mut self, id: DefId, node: res::Node) -> DefId {
        assert!(
            self.nodes[id].replace(node).is_none(),
            "can only have 1 node"
        );
        id
    }
    fn add_item(&mut self, id: DefId, item: res::Item) {
        self.add_node(id, res::Node::Item(Box::new(item)));
    }
    fn declare_in_exprs(&mut self, expr: &ast::Expr) {
        match &expr.kind {
            ast::ExprKind::Unit
            | ast::ExprKind::String(_)
            | ast::ExprKind::Bool(_)
            | ast::ExprKind::Number(_)
            | ast::ExprKind::Panic(_)
            | ast::ExprKind::Path(..) => (),
            ast::ExprKind::Annotate(expr, _)
            | ast::ExprKind::Deref(expr)
            | ast::ExprKind::AddressOf(expr)
            | ast::ExprKind::Field(expr, _) => self.declare_in_exprs(expr),
            ast::ExprKind::Print(expr) => {
                if let Some(expr) = expr {
                    self.declare_in_exprs(expr);
                }
            }
            ast::ExprKind::Call(callee, args) => {
                self.declare_in_exprs(callee);
                for arg in args {
                    self.declare_in_exprs(arg);
                }
            }
            ast::ExprKind::Borrow(borrow_expr) => self.declare_in_exprs(&borrow_expr.expr),
            ast::ExprKind::Case(expr, case_arms) => {
                self.declare_in_exprs(expr);
                for arm in case_arms.iter() {
                    self.declare_in_exprs(&arm.body);
                }
            }
            ast::ExprKind::Assign(expr1, expr2)
            | ast::ExprKind::While(expr1, expr2)
            | ast::ExprKind::Binary(_, expr1, expr2)
            | ast::ExprKind::For(_, expr1, expr2) => {
                self.declare_in_exprs(expr1);
                self.declare_in_exprs(expr2);
            }
            ast::ExprKind::Lambda(lambda) => {
                self.declare_def_id_for(self.current_module.unwrap(), lambda.id);
                self.declare_in_exprs(&lambda.body);
            }
            ast::ExprKind::Block(block_body, _) => {
                for stmt in block_body.stmts.iter() {
                    match &stmt.kind {
                        StmtKind::Expr(expr) => self.declare_in_exprs(expr),
                        StmtKind::Let(let_binding) => self.declare_in_exprs(&let_binding.value),
                    }
                }
                self.declare_in_exprs(&block_body.expr);
            }
            ast::ExprKind::Record(ast::RecordExpr { fields })
            | ast::ExprKind::NamedRecord(_, fields) => {
                for field in fields.iter() {
                    self.declare_in_exprs(&field.value);
                }
            }
        }
    }
    fn declare_function(&mut self, id: ModuleNodeId, function: &ast::Function) {
        self.declare_item(function.name, "function", Res::Function(id));
        if let Some(body) = function.body.as_ref() {
            let old_module = self.current_module.replace(id.0);
            self.declare_in_exprs(body);
            self.current_module = old_module;
        }
    }
    fn is_region_param(&self, name: Ident) -> bool {
        self.generic_kinds
            .get(&name.symbol)
            .is_some_and(|kind| *kind == res::GenericKind::Region)
    }
    fn resolve_region_param(&mut self, name: Ident, index: usize) -> res::RegionKind {
        if self
            .generic_kinds
            .insert(name.symbol, res::GenericKind::Region)
            .is_some_and(|kind| kind != res::GenericKind::Region)
        {
            self.diag.add_diagnostic(
                format!("Generic kind mismatch for '{}'", name.symbol),
                name.loc,
            );
        }
        res::RegionKind::Param(name.symbol, index)
    }
    fn resolve_region(&mut self, region: ast::Region) -> res::Region {
        match region {
            ast::Region::Named(name) => res::Region {
                loc: name.loc,
                kind: match self.resolve_name(name.symbol) {
                    None => {
                        self.not_in_scope_error(name.symbol, name.loc);
                        res::RegionKind::Unknown
                    }
                    Some(Res::LocalRegion(region)) => res::RegionKind::Local(name.symbol, region),
                    Some(Res::Param(index)) => self.resolve_region_param(name, index),
                    Some(
                        Res::TypeAlias(_)
                        | Res::Builtin(_)
                        | Res::Function(_)
                        | Res::Var(_)
                        | Res::Module(_)
                        | Res::TypeDef(_)
                        | Res::VariantCase(..),
                    ) => {
                        self.cannot_use_as_error(name.symbol, "region", name.loc);
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
    fn get_generic_kind(&self, name: Symbol) -> Option<GenericKind> {
        self.generic_kinds.get(&name).copied().or_else(|| {
            self.prev_kinds
                .iter()
                .rev()
                .find_map(|kinds| kinds.get(&name).copied())
        })
    }
    fn error_on_generic_args(
        &mut self,
        kind: &str,
        name: Symbol,
        loc: SrcLoc,
        args: Option<GenericArgs>,
    ) {
        if args.is_some() {
            self.diag.add_diagnostic(
                format!("{kind} '{}' cannot have generic arguments", name),
                loc,
            );
        }
        self.resolve_generic_args(args);
    }
    fn resolve_generic_args(&mut self, args: Option<GenericArgs>) -> res::GenericArgs {
        let Some(args) = args else {
            return res::GenericArgs::NONE;
        };
        res::GenericArgs {
            loc: Some(args.loc),
            args: args
                .args
                .into_iter()
                .map(|arg| match arg.ty.kind {
                    ast::TypeKind::Named(ast::InstancePath {
                        ref path,
                        generic_args: None,
                    }) if let Ok(Res::LocalRegion(local)) = self.resolve_path(path) => {
                        res::GenericArg::Region(res::Region {
                            loc: arg.ty.loc,
                            kind: res::RegionKind::Local(path.last().symbol, local),
                        })
                    }
                    ast::TypeKind::Named(ast::InstancePath {
                        ref path,
                        generic_args: None,
                    }) if let Ok(Res::Param(index)) = self.resolve_path(path)
                        && let name = path.last()
                        && self.is_region_param(name) =>
                    {
                        res::GenericArg::Region(res::Region {
                            loc: arg.ty.loc,
                            kind: res::RegionKind::Param(name.symbol, index),
                        })
                    }
                    _ => res::GenericArg::Type(self.resolve_type(arg.ty)),
                })
                .collect(),
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
                self.declare_param(param.symbol);
            })
            .collect::<Vec<_>>();
        let old_kinds = std::mem::take(&mut self.generic_kinds);
        self.prev_kinds.push(old_kinds);
        let value = f(self);
        let kinds = names
            .iter()
            .map(|name| name.symbol)
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
    fn resolve_type_path(&mut self, path: &Path, loc: SrcLoc) -> Option<res::TypeName> {
        let name = *path.segments().last().unwrap();
        match self.resolve_path(path) {
            Err(err) => {
                self.path_res_error(path, loc, err);
                None
            }
            Ok(Res::Param(index)) => {
                if self
                    .generic_kinds
                    .insert(name.symbol, res::GenericKind::Type)
                    .is_some_and(|kind| kind != res::GenericKind::Type)
                {
                    self.diag.add_diagnostic(
                        format!("Generic kind mismatch for '{}'", name.symbol),
                        name.loc,
                    );
                }
                Some(res::TypeName::Param(name.symbol, index))
            }
            Ok(Res::TypeAlias(alias)) => match alias {
                TypeAlias::Ptr => Some(res::TypeName::Ptr),
                TypeAlias::Box => Some(res::TypeName::Box),
                TypeAlias::Byte => Some(res::TypeName::Byte),
                TypeAlias::ArrayList => Some(res::TypeName::ArrayList),
            },
            Ok(Res::TypeDef(id)) => Some(res::TypeName::UserDefined(self.def_id_for(id))),
            Ok(Res::VariantCase(..)) => {
                self.cannot_use_as_error(name.symbol, "type", name.loc);
                None
            }
            Ok(
                Res::Builtin(_)
                | Res::Function(_)
                | Res::LocalRegion(_)
                | Res::Var(_)
                | Res::Module(_),
            ) => {
                self.cannot_use_as_error(name.symbol, "type", name.loc);
                None
            }
        }
    }
    fn resolve_type(&mut self, ty: ast::Type) -> res::Type {
        let kind = match ty.kind {
            ast::TypeKind::Char => {
                res::TypeKind::Named(res::TypeName::Char, Box::new(res::GenericArgs::NONE))
            }
            ast::TypeKind::Bool => {
                res::TypeKind::Named(res::TypeName::Bool, Box::new(res::GenericArgs::NONE))
            }
            ast::TypeKind::Int => {
                res::TypeKind::Named(res::TypeName::Int, Box::new(res::GenericArgs::NONE))
            }
            ast::TypeKind::Unit => {
                res::TypeKind::Named(res::TypeName::Unit, Box::new(res::GenericArgs::NONE))
            }
            ast::TypeKind::String => {
                res::TypeKind::Named(res::TypeName::String, Box::new(res::GenericArgs::NONE))
            }
            ast::TypeKind::Record(ast::RecordType { fields }) => {
                let fields = fields
                    .into_iter()
                    .map(
                        |ast::RecordField { id: _, name, ty }| res::RecordFieldType {
                            name,
                            ty: self.resolve_type(ty),
                        },
                    )
                    .collect();
                res::TypeKind::Record(fields)
            }
            ast::TypeKind::Function(ast::FunctionType {
                resource,
                params,
                return_type,
            }) => res::TypeKind::Function(Box::new(res::FunctionType {
                is_resource: resource,
                params: params.into_iter().map(|ty| self.resolve_type(ty)).collect(),
                return_type: Box::new(self.resolve_type(*return_type)),
            })),
            ast::TypeKind::Imm(region, ty) => res::TypeKind::Imm(
                Box::new(self.resolve_region(region)),
                Box::new(self.resolve_type(*ty)),
            ),
            ast::TypeKind::Mut(region, ty) => res::TypeKind::Mut(
                Box::new(self.resolve_region(region)),
                Box::new(self.resolve_type(*ty)),
            ),
            ast::TypeKind::Named(path) => match self.resolve_type_path(&path.path, ty.loc) {
                None => {
                    self.resolve_generic_args(path.generic_args);
                    res::TypeKind::Unknown
                }
                Some(name) => res::TypeKind::Named(
                    name,
                    Box::new(self.resolve_generic_args(path.generic_args)),
                ),
            },
        };
        res::Type { loc: ty.loc, kind }
    }
    fn declare_region(&mut self, region: Symbol) -> LocalRegionId {
        let region_id = LocalRegionId::new(self.vars);
        self.regions += 1;
        self.env.insert(region, Res::LocalRegion(region_id));
        region_id
    }
    fn declare_var(&mut self, var: Symbol) -> VarId {
        let var_id = VarId::new(self.vars);
        self.vars += 1;
        self.env.insert(var, Res::Var(var_id));
        var_id
    }
    fn declare_module(&mut self, id: ModuleId, name: Symbol) -> DefId {
        let def_id = self.next_def_id();
        self.modules.insert(
            id,
            ModuleInfo {
                id: def_id,
                env: Default::default(),
            },
        );
        self.env.insert(name, Res::Module(id));
        def_id
    }
    fn declare_param(&mut self, name: Symbol) -> usize {
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
    fn resolve_in_module_scope<T>(
        &mut self,
        module: ModuleId,
        f: impl FnOnce(&mut Self) -> T,
    ) -> T {
        self.prev_envs.push({
            let old_env = std::mem::take(&mut self.env);
            self.env.clone_from(&self.modules[&module].env);
            old_env
        });
        let old_module = self.current_module.replace(module);
        let value = f(self);
        self.env = self
            .prev_envs
            .pop()
            .expect("There should be a pushed scope");
        self.current_module = old_module;
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
    fn resolve_pattern(&mut self, pattern: ast::Pattern) -> res::Pattern {
        let loc = pattern.loc;
        let kind = match pattern.kind {
            ast::PatternKind::Unit => res::PatternKind::Unit,
            ast::PatternKind::Int(value) => res::PatternKind::Int(match value.try_into() {
                Ok(number) => number,
                Err(_) => {
                    self.diag.add_diagnostic("Invalid integer".to_string(), loc);
                    0
                }
            }),
            ast::PatternKind::Bool(value) => res::PatternKind::Bool(value),
            ast::PatternKind::Binding(borrow, mutable, name) => {
                let var = self.declare_var(name.symbol);
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
            ast::PatternKind::Case(name, inner) => res::PatternKind::Case(
                name,
                inner.map(|inner| Box::new(self.resolve_pattern(*inner))),
            ),
        };
        res::Pattern { loc, kind }
    }
    fn resolve_let_binding(&mut self, let_binding: ast::LetBinding) -> res::LetBinding {
        let value = self.resolve_expr(let_binding.value);
        let ty = let_binding.ty.map(|ty| self.resolve_type(ty));
        let pattern = self.resolve_pattern(let_binding.pattern);
        res::LetBinding { pattern, ty, value }
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
    fn resolve_builtin(&mut self, builtin: Builtin) -> Option<res::FunctionDefId> {
        let builtin_module = self.builtin_module?;
        let res = *self.modules[&builtin_module]
            .env
            .get(&Symbol::intern(builtin.name()))?;
        match res {
            Res::Function(id) => Some(res::FunctionDefId(self.def_id_for(id))),
            _ => None,
        }
    }
    fn resolve_path(&mut self, path: &Path) -> Result<Res, NameResolutionError> {
        let head_seg = *path.head();
        let Some(head) = self.resolve_name(head_seg.symbol) else {
            return Err(NameResolutionError::NotInScope);
        };
        let mut curr = head;
        for (index, &segment) in path.segments_iter().into_iter().enumerate().skip(1) {
            curr = match curr {
                Res::Module(module) => {
                    let env = &self.modules[&module].env;
                    if let Some(&res) = env.get(&segment.symbol) {
                        res
                    } else {
                        return Err(NameResolutionError::NotInScope);
                    }
                }
                Res::TypeDef(id) => {
                    let type_def = &self.type_defs[&id];
                    let (cases, case_map) = match type_def {
                        TypeDefInfo::Record { .. } => {
                            return Err(NameResolutionError::NotInScope);
                        }
                        TypeDefInfo::Variant { cases, case_map } => (cases, case_map),
                    };
                    let Some(&case) = case_map.get(&segment.symbol) else {
                        return Err(NameResolutionError::NotInScope);
                    };
                    Res::VariantCase(ModuleNodeId(id.0, cases[case].id))
                }
                Res::Var(var) => {
                    return Err(NameResolutionError::VariableField(
                        head_seg.loc,
                        res::Var(head_seg.symbol, var),
                        path.segments()[index..].to_vec(),
                    ));
                }
                Res::Builtin(_)
                | Res::Function(_)
                | Res::Param(_)
                | Res::LocalRegion(_)
                | Res::TypeAlias(_)
                | Res::VariantCase(..) => return Err(NameResolutionError::InvalidPathStart),
            }
        }
        Ok(curr)
    }
    fn path_res_error(&mut self, path: &Path, loc: SrcLoc, err: NameResolutionError) {
        match err {
            NameResolutionError::NotInScope => {
                self.path_not_in_scope_error(path, loc);
            }
            NameResolutionError::InvalidPathStart => {
                self.invalid_path_start_error(path, loc);
            }
            NameResolutionError::VariableField(..) => {
                self.invalid_path_start_error(path, loc);
            }
        }
    }
    fn resolve_path_as_expr(
        &mut self,
        loc: SrcLoc,
        path: Path,
        args: Option<GenericArgs>,
    ) -> res::ExprKind {
        match self.resolve_path(&path) {
            Err(error) => {
                self.resolve_generic_args(args);
                match error {
                    NameResolutionError::NotInScope | NameResolutionError::InvalidPathStart => {
                        self.path_res_error(&path, loc, error);
                    }
                    NameResolutionError::VariableField(loc, var, fields) => {
                        let expr = res::Expr {
                            loc,
                            kind: res::ExprKind::Var(var),
                        };
                        return fields
                            .into_iter()
                            .fold(expr, |expr, field| res::Expr {
                                loc: expr.loc,
                                kind: res::ExprKind::Field(Box::new(expr), field),
                            })
                            .kind;
                    }
                }
                res::ExprKind::Err
            }
            Ok(res) => match res {
                Res::Builtin(builtin) => match self.resolve_builtin(builtin) {
                    Some(id) => {
                        res::ExprKind::Function(id, Box::new(self.resolve_generic_args(args)))
                    }
                    None => {
                        self.builtin_not_found_error(builtin, loc);
                        res::ExprKind::Err
                    }
                },
                Res::Var(id) => {
                    let name = path.into_last().symbol;
                    self.error_on_generic_args("var", name, loc, args);
                    res::ExprKind::Var(res::Var(name, id))
                }
                Res::Function(function) => res::ExprKind::Function(
                    res::FunctionDefId(self.def_id_for(function)),
                    Box::new(self.resolve_generic_args(args)),
                ),
                Res::Param(_)
                | Res::LocalRegion(_)
                | Res::Module(_)
                | Res::TypeAlias(_)
                | Res::TypeDef(_) => {
                    self.resolve_generic_args(args);
                    self.diag
                        .add_diagnostic(format!("Can't use '{}' as a value", path), loc);
                    res::ExprKind::Err
                }
                Res::VariantCase(id) => res::ExprKind::VariantCase(
                    self.def_id_for(id),
                    Box::new(self.resolve_generic_args(args)),
                ),
            },
        }
    }
    fn resolve_expr(&mut self, expr: ast::Expr) -> res::Expr {
        let loc = expr.loc;
        let kind = match expr.kind {
            ast::ExprKind::Block(block, region) => self.in_scope(|this| {
                let region = region.map(|region| this.declare_region(region.symbol));
                res::ExprKind::Block(
                    Box::new(res::BlockBody {
                        stmts: block
                            .stmts
                            .into_iter()
                            .map(|stmt| this.resolve_stmt(stmt))
                            .collect(),
                        expr: Box::new(this.resolve_expr(*block.expr)),
                    }),
                    region,
                )
            }),
            ast::ExprKind::While(condition, body) => {
                let condition = self.resolve_expr(*condition);
                let body = self.resolve_expr(*body);
                res::ExprKind::While(Box::new(condition), Box::new(body))
            }
            ast::ExprKind::AddressOf(expr) => {
                res::ExprKind::AddressOf(Box::new(self.resolve_expr(*expr)))
            }
            ast::ExprKind::Unit => res::ExprKind::Unit,
            ast::ExprKind::String(value) => res::ExprKind::String(value.into()),
            ast::ExprKind::Number(value) => res::ExprKind::Int(value as i64),
            ast::ExprKind::Bool(value) => res::ExprKind::Bool(value),
            ast::ExprKind::Print(arg) => {
                res::ExprKind::Print(arg.map(|arg| Box::new(self.resolve_expr(*arg))))
            }
            ast::ExprKind::Annotate(expr, ty) => res::ExprKind::Annotate(
                Box::new(self.resolve_expr(*expr)),
                Box::new(self.resolve_type(*ty)),
            ),
            ast::ExprKind::Field(expr, field) => {
                res::ExprKind::Field(Box::new(self.resolve_expr(*expr)), field)
            }
            ast::ExprKind::Panic(ty) => {
                res::ExprKind::Panic(ty.map(|ty| Box::new(self.resolve_type(ty))))
            }
            ast::ExprKind::Call(callee, args) => res::ExprKind::Call(
                Box::new(self.resolve_expr(*callee)),
                args.into_iter().map(|arg| self.resolve_expr(arg)).collect(),
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
            ast::ExprKind::Path(path) => {
                self.resolve_path_as_expr(loc, path.path, path.generic_args)
            }
            ast::ExprKind::Lambda(lambda) => self.in_scope(|this| {
                let id = this.def_id_for(ModuleNodeId(this.current_module.unwrap(), lambda.id));
                let (params, param_tys): (Vec<_>, Vec<_>) = lambda
                    .params
                    .into_iter()
                    .map(|(name, ty)| {
                        let var = this.declare_var(name.symbol);
                        let ty = ty.map(|ty| this.resolve_type(ty));
                        (
                            res::Param {
                                loc: name.loc,
                                var: res::Var(name.symbol, var),
                            },
                            ty,
                        )
                    })
                    .unzip();
                let lambda = Rc::new(res::Lambda {
                    id,
                    loc,
                    params: params.into_boxed_slice(),
                    param_tys: param_tys.into_boxed_slice(),
                    resource: lambda.resource,
                    body: this.resolve_expr(*lambda.body),
                });
                this.add_node(id, res::Node::Lambda(Rc::clone(&lambda)));
                res::ExprKind::Lambda(lambda)
            }),
            ast::ExprKind::Assign(place, value) => {
                let place = self.resolve_expr(*place);
                let value = self.resolve_expr(*value);
                res::ExprKind::Assign(Box::new(place), Box::new(value))
            }
            ast::ExprKind::Borrow(borrow_expr) => {
                let BorrowExpr {
                    mutable,
                    expr,
                    region,
                } = *borrow_expr;
                let place = self.resolve_expr(expr);
                let region = self.resolve_region(region);
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
            ast::ExprKind::NamedRecord(path, fields) => {
                let ty_def = self.resolve_type_path(&path.path, loc);
                let generic_args = self.resolve_generic_args(path.generic_args);
                let fields = fields
                    .into_iter()
                    .map(|field| res::FieldInit {
                        name: field.name,
                        value: self.resolve_expr(field.value),
                    })
                    .collect::<Vec<_>>();
                let Some(type_name) = ty_def else {
                    return res::Expr {
                        loc,
                        kind: res::ExprKind::Err,
                    };
                };
                res::ExprKind::NamedRecord(
                    type_name,
                    Box::new(generic_args),
                    fields.into_boxed_slice(),
                )
            }
            ast::ExprKind::For(pattern, iterator, body) => {
                let iterator = self.resolve_expr(*iterator);
                self.in_scope(|this| {
                    let pattern = this.resolve_pattern(*pattern);
                    let body = this.resolve_expr(*body);
                    res::ExprKind::For(Box::new(res::ForExpr {
                        pattern,
                        iterator,
                        body,
                    }))
                })
            }
        };
        res::Expr { loc, kind }
    }
    fn resolve_signature(
        &mut self,
        params: Vec<ast::Param>,
        return_type: ast::Type,
    ) -> (Box<[res::Param]>, Box<[res::Type]>, res::Type) {
        let (names, tys): (Vec<_>, Vec<_>) = params
            .into_iter()
            .map(|param| (param.name, self.resolve_type(param.ty)))
            .unzip();
        let return_type = self.resolve_type(return_type);
        let (params, param_tys): (Vec<_>, Vec<_>) = names
            .into_iter()
            .zip(tys)
            .map(|(name, ty)| {
                (
                    res::Param {
                        loc: name.loc,
                        var: res::Var(name.symbol, self.declare_var(name.symbol)),
                    },
                    ty,
                )
            })
            .unzip();
        (
            params.into_boxed_slice(),
            param_tys.into_boxed_slice(),
            return_type,
        )
    }
    fn resolve_function(&mut self, function: ast::Function) -> res::Function {
        self.resolve_item(|this| {
            let (generics, (params, param_tys, return_type), body) =
                if let Some(generics) = function.generics {
                    let (generics, (sig, body)) = this.resolve_generics(generics, |this| {
                        (
                            this.resolve_signature(function.params, function.return_type),
                            function.body.map(|body| this.resolve_expr(body)),
                        )
                    });
                    (Some(Box::new(generics)), sig, body)
                } else {
                    (
                        None,
                        this.resolve_signature(function.params, function.return_type),
                        function.body.map(|body| this.resolve_expr(body)),
                    )
                };
            res::Function {
                name: function.name,
                param_tys,
                generics,
                params,
                return_type: Box::new(return_type),
                body: body.map(Box::new),
            }
        })
    }
    fn resolve_type_def_body(
        &mut self,
        type_id: ModuleNodeId,
        body: ast::TypeDefKind,
    ) -> res::TypeDefKind {
        match body {
            ast::TypeDefKind::Record(record) => res::TypeDefKind::Record(res::RecordDef {
                fields: record
                    .fields
                    .into_iter()
                    .map(|field| {
                        let node = res::Node::Field(Box::new(res::FieldDef {
                            name: field.name,
                            ty: self.resolve_type(field.ty),
                        }));
                        self.add_node(self.def_id_for(ModuleNodeId(type_id.0, field.id)), node)
                    })
                    .collect(),
            }),
            ast::TypeDefKind::Variant(cases) => res::TypeDefKind::Variant(VariantDef {
                cases: cases
                    .into_iter()
                    .map(|case| {
                        let id = self.def_id_for(ModuleNodeId(type_id.0, case.id));
                        let case = res::CaseDef {
                            id,
                            name: case.name,
                            field: case.ty.map(|ty| {
                                let id = self.def_id_for(ModuleNodeId(type_id.0, ty.id));
                                let ty = self.resolve_type(ty.ty);
                                self.add_node(
                                    id,
                                    res::Node::CaseField(Box::new(res::CaseField { id, ty })),
                                )
                            }),
                        };
                        let node = res::Node::Case(Box::new(case));
                        self.add_node(id, node);
                        case
                    })
                    .collect(),
            }),
        }
    }
    fn resolve_type_def(&mut self, id: ModuleNodeId, type_def: ast::TypeDef) -> res::TypeDef {
        self.resolve_item(|this| {
            let (generics, kind) = if let Some(generics) = type_def.generics {
                let (generics, kind) = this.resolve_generics(generics, |this| {
                    this.resolve_type_def_body(id, type_def.kind)
                });
                (Some(generics), kind)
            } else {
                (None, this.resolve_type_def_body(id, type_def.kind))
            };
            res::TypeDef {
                name: type_def.name,
                generics: generics.map(Box::new),
                kind,
            }
        })
    }
    fn resolve_item<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let value = self.in_scope(|this| f(this));
        self.generics = 0;
        self.vars = 0;
        value
    }
    fn next_def_id(&mut self) -> DefId {
        let def_id = self.nodes.push(None);
        if let Some(current_parent) = self.current_item {
            self.parents.insert(def_id, current_parent);
        }
        def_id
    }
    fn declare_def_id_for(&mut self, module: ModuleId, id: NodeId) -> DefId {
        let def_id = self.next_def_id();
        self.item_id_to_def_id
            .insert(ModuleNodeId(module, id), def_id);
        def_id
    }
    #[track_caller]
    fn def_id_for(&self, id: ModuleNodeId) -> DefId {
        *self
            .item_id_to_def_id
            .get(&id)
            .expect("should have a def id")
    }
    fn declare_item(&mut self, name: Ident, kind: &str, res: Res) {
        match self.env.entry(name.symbol) {
            std::collections::hash_map::Entry::Occupied(occupied) => {
                self.diag.add_diagnostic(
                    format!("Cannot redeclare '{}' '{}'", kind, occupied.key()),
                    name.loc,
                );
            }
            std::collections::hash_map::Entry::Vacant(vacant) => {
                vacant.insert(res);
            }
        }
    }
    fn with_parent_def_id<T>(&mut self, id: DefId, f: impl FnOnce(&mut Self) -> T) -> T {
        let parent = self.current_item.replace(id);
        let value = f(self);
        self.current_item = parent;
        value
    }
    fn declare_type_def(&mut self, id: ModuleNodeId, def_id: DefId, type_def: &ast::TypeDef) {
        let name = type_def.name;
        let info = self.with_parent_def_id(self.def_id_for(id), |this| match type_def.kind {
            ast::TypeDefKind::Record(ref record) => this.with_parent_def_id(def_id, |this| {
                let mut fields = Vec::new();
                for field in &record.fields {
                    this.declare_def_id_for(id.0, field.id);
                    fields.push(FieldInfo {
                        _name: field.name,
                        _id: field.id,
                    });
                }
                TypeDefInfo::Record { _fields: fields }
            }),
            ast::TypeDefKind::Variant(ref cases) => {
                let cases = cases
                    .iter()
                    .map(|case| {
                        let def_id = this.declare_def_id_for(id.0, case.id);
                        if let Some(ty) = case.ty.as_ref() {
                            this.with_parent_def_id(def_id, |this| {
                                this.declare_def_id_for(id.0, ty.id);
                            })
                        }
                        CaseInfo {
                            name: case.name,
                            id: case.id,
                        }
                    })
                    .collect::<Vec<_>>();
                TypeDefInfo::Variant {
                    case_map: cases
                        .iter()
                        .enumerate()
                        .map(|(i, case)| (case.name.symbol, i))
                        .collect(),
                    cases,
                }
            }
        });
        self.type_defs.insert(id, info);
        self.declare_item(name, "type", Res::TypeDef(id));
    }
    fn declare_module_items(&mut self, module: &ast::Module) {
        let id = self.declare_module(module.id, module.name);
        self.with_parent_def_id(id, |this| {
            let scope = this.take_new_scope(|this| {
                for item in module.items.iter() {
                    let &ast::Item {
                        id,
                        ref kind,
                        loc: _,
                        annotations: _,
                    } = item;
                    let full_id = ModuleNodeId(module.id, id);
                    let def_id = this.declare_def_id_for(module.id, item.id);
                    match kind {
                        ast::ItemKind::TypeDef(type_def) => {
                            this.declare_type_def(full_id, def_id, type_def);
                        }
                        ast::ItemKind::Function(function) => {
                            this.declare_function(full_id, function);
                        }
                    }
                }
                for module in &module.child_modules {
                    this.declare_module_items(module);
                }
            });
            this.modules.get_mut(&module.id).unwrap().env.extend(scope);
        })
    }
    fn declare(&mut self, modules: &[ast::Module]) {
        for module in modules.iter() {
            if module.name == Symbol::BUILTINS {
                self.builtin_module = Some(module.id);
            }
            self.declare_module_items(module);
        }
    }
    fn def_id_for_module(&self, module: ModuleId) -> DefId {
        self.modules[&module].id
    }
    fn resolve_annotations(&self, annotations: Vec<ast::Annotation>) -> Vec<res::Annotation> {
        annotations
            .into_iter()
            .filter_map(|annotation| {
                Some(res::Annotation {
                    loc: annotation.loc,
                    kind: match annotation.name.symbol {
                        Symbol::COPY => {
                            if !annotation.fields.is_empty() {
                                self.diag.add_diagnostic(
                                    format!("too many fields for '{}'", annotation.name.symbol),
                                    annotation.loc,
                                );
                            }
                            res::AnnotationKind::Copy
                        }
                        Symbol::UNSAFE => {
                            if !annotation.fields.is_empty() {
                                self.diag.add_diagnostic(
                                    format!("too many fields for '{}'", annotation.name.symbol),
                                    annotation.loc,
                                );
                            }
                            res::AnnotationKind::Unsafe
                        }
                        Symbol::LANG_ITEM => {
                            if let [field] = annotation.fields.as_slice()
                                && let ast::AnnotationField::String(_, name) = field
                                && let Some(lang_item) = LangItem::with_name(name)
                            {
                                res::AnnotationKind::LangItem(lang_item)
                            } else {
                                self.diag.add_diagnostic(
                                    format!("invalid fields for '{}'", annotation.name.symbol),
                                    annotation.loc,
                                );
                                return None;
                            }
                        }
                        Symbol::OPAQUE => {
                            if !annotation.fields.is_empty() {
                                self.diag.add_diagnostic(
                                    format!("too many fields for '{}'", annotation.name.symbol),
                                    annotation.loc,
                                );
                            }
                            res::AnnotationKind::Opaque
                        }
                        _ => {
                            self.diag.add_diagnostic(
                                format!("unknown annotation {}", annotation.name.symbol),
                                annotation.loc,
                            );
                            return None;
                        }
                    },
                })
            })
            .collect()
    }
    fn resolve_module(&mut self, module: ast::Module) {
        self.resolve_in_module_scope(module.id, |this| {
            let mut mod_items = Vec::with_capacity(module.items.len() + module.child_modules.len());
            for item in module.items.into_iter() {
                let node_id = ModuleNodeId(module.id, item.id);
                let id = this.item_id_to_def_id[&node_id];
                let item = res::Item {
                    id,
                    loc: item.loc,
                    annotations: this
                        .resolve_annotations(item.annotations)
                        .into_boxed_slice(),
                    kind: match item.kind {
                        ast::ItemKind::Function(function) => {
                            res::ItemKind::Function(Box::new(this.resolve_function(function)))
                        }
                        ast::ItemKind::TypeDef(type_def) => res::ItemKind::TypeDef(Box::new(
                            this.resolve_type_def(node_id, type_def),
                        )),
                    },
                };
                this.add_item(id, item);
                mod_items.push(id);
            }
            for child in module.child_modules {
                let id = this.modules[&child.id].id;
                this.resolve_module(child);
                mod_items.push(id);
            }
            let item = res::Item {
                id: this.def_id_for_module(module.id),
                loc: SrcLoc::dummy().with_file(module.name),
                annotations: Vec::new().into_boxed_slice(),
                kind: res::ItemKind::Module(Box::new(res::Module {
                    name: Ident {
                        symbol: module.name,
                        loc: SrcLoc::dummy().with_file(module.name),
                    },
                    items: mod_items.into_boxed_slice(),
                })),
            };
            this.add_item(item.id, item);
        })
    }
    pub fn resolve(mut self, modules: Vec<ast::Module>) -> Result<GlobalContext, ResolveErrored> {
        //First pass : Declare everything
        self.declare(&modules);
        //Second pass : Resolve
        let nodes = {
            for module in modules.into_iter() {
                self.resolve_module(module);
            }
            std::mem::take(&mut self.nodes)
                .into_iter_enumerated()
                .map(|(id, node)| node.unwrap_or_else(|| panic!("missing node for '{:?}'", id)))
                .collect()
        };
        let mut builtins = Builtins::default();
        if let Some(builtin_module) = self.builtin_module {
            for (name, &res) in &self.modules[&builtin_module].env {
                let Some(builtin) = Builtin::find(*name) else {
                    continue;
                };
                let Res::Function(id) = res else {
                    continue;
                };
                let id = self.def_id_for(id);
                builtins.insert(builtin, id);
            }
        }
        let context = build_global_context(nodes, builtins, self.parents);
        if !self.diag.report_all() {
            Ok(context)
        } else {
            Err(ResolveErrored)
        }
    }
}
