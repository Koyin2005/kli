use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{BorrowExpr, GenericArgs, ModuleId, NodeId, Path, StmtKind};
use crate::builtins::{Builtin, Builtins};
use crate::collect::{GlobalContext, build_global_context};
use crate::config::Config;
use crate::def_ids::DefId;
use crate::diagnostics::DiagnosticReporter;
use crate::ident::Ident;
use crate::index_vec::IndexVec;
use crate::lang_items::LangItem;
use crate::resolve::decl::DeclareResults;
use crate::resolved_ast::{GenericKind, LocalRegionId, VarId, VariantDef};
use crate::src_loc::SrcLoc;
use crate::{Symbol, ast, resolved_ast as res};

mod decl;
pub struct ResolveErrored;

enum NameResolutionError {
    NotInScope,
    InvalidPathStart,
    VariableField(SrcLoc, res::Var, Vec<Ident>),
    TypeRelative(res::TypeName, Vec<Ident>),
}
pub(super) type ModuleItems = HashMap<Symbol, Def>;
#[derive(Debug)]
pub(super) struct ModuleInfo {
    pub id: DefId,
    pub items: ModuleItems,
}
#[derive(Clone, Copy, Debug)]
enum TypeAlias {
    Ptr,
    Byte,
    Box,
    ArrayList,
    Never,
    Pair,
}
impl TypeAlias {
    fn into_type_name(self) -> res::TypeName {
        match self {
            TypeAlias::Ptr => res::TypeName::Ptr,
            TypeAlias::Box => res::TypeName::Box,
            TypeAlias::Byte => res::TypeName::Byte,
            TypeAlias::ArrayList => res::TypeName::ArrayList,
            TypeAlias::Never => res::TypeName::Never,
            TypeAlias::Pair => res::TypeName::Pair,
        }
    }
}
type Scope = HashMap<Symbol, Res>;

#[derive(Clone, Copy, Debug)]
pub(super) enum Def {
    Function(ModuleNodeId),
    Type(ModuleNodeId),
    Module(ModuleId),
}
impl From<Def> for Res {
    fn from(value: Def) -> Self {
        Res::Def(value)
    }
}

#[derive(Clone, Copy, Debug)]
enum Res {
    LocalRegion(LocalRegionId),
    Param(usize),
    Var(VarId),
    Def(Def),
    TypeAlias(TypeAlias),
    VariantCase(ModuleNodeId),
    Unknown,
}
#[derive(PartialEq, Eq, Hash, Clone, Copy, Debug)]
pub(super) struct ModuleNodeId(ModuleId, NodeId);
#[derive(Debug)]
struct CaseInfo {
    pub name: Ident,
    pub id: NodeId,
}
#[derive(Debug)]
struct FieldInfo {
    _name: Ident,
    _id: NodeId,
}
#[derive(Debug)]
pub(super) struct TypeImplInfo {
    pub methods: HashMap<Symbol, ModuleNodeId>,
}
#[derive(Debug)]
pub struct TypeInfo {
    kind: TypeDefInfoKind,
    impl_: Option<ModuleNodeId>,
}
#[derive(Debug)]
enum TypeDefInfoKind {
    Variant {
        cases: Vec<CaseInfo>,
        case_map: HashMap<Symbol, usize>,
    },
    Record {
        _fields: Vec<FieldInfo>,
    },
}
fn make_block_expr(
    loc: SrcLoc,
    region: Option<LocalRegionId>,
    stmts: impl IntoIterator<Item = res::Stmt>,
    expr: res::Expr,
) -> res::Expr {
    res::Expr {
        loc,
        kind: res::ExprKind::Block(
            Box::new(res::BlockBody {
                stmts: stmts.into_iter().collect(),
                expr: Box::new(expr),
            }),
            region,
        ),
    }
}
pub struct Resolve<'info> {
    config: Config,
    env: Scope,
    prev_envs: Vec<Scope>,
    vars: usize,
    regions: usize,
    generics: usize,
    generic_kinds: HashMap<Symbol, GenericKind>,
    current_module: Option<ModuleId>,
    diag: DiagnosticReporter,
    nodes: IndexVec<DefId, Option<res::Node>>,
    decl_info: &'info DeclareResults,
}
impl<'info> Resolve<'info> {
    fn new(config: Config, results: &'info DeclareResults) -> Self {
        let env = Scope::from_iter([
            (Symbol::intern("ptr"), Res::TypeAlias(TypeAlias::Ptr)),
            (Symbol::intern("byte"), Res::TypeAlias(TypeAlias::Byte)),
            (Symbol::intern("Box"), Res::TypeAlias(TypeAlias::Box)),
            (Symbol::intern("never"), Res::TypeAlias(TypeAlias::Never)),
            (Symbol::intern("Pair"), Res::TypeAlias(TypeAlias::Pair)),
            (
                Symbol::intern("ArrayList"),
                Res::TypeAlias(TypeAlias::ArrayList),
            ),
        ]);
        Self {
            config,
            prev_envs: Vec::new(),
            env,
            vars: 0,
            regions: 0,
            generics: 0,
            diag: DiagnosticReporter::new(),
            generic_kinds: HashMap::new(),
            nodes: IndexVec::from_function(results.def_ids, |_| None),
            current_module: None,
            decl_info: results,
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
        let &module = self.decl_info.top_level_modules.get(&name)?;
        Some(Res::Def(Def::Module(module)))
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
    fn is_region_param(&self, name: Ident) -> bool {
        self.generic_kinds
            .get(&name.symbol)
            .is_some_and(|kind| *kind == res::GenericKind::Region)
    }
    fn resolve_region_param(&mut self, name: Ident, index: usize) -> res::RegionKind {
        if self.generic_kinds[&name.symbol] != res::GenericKind::Region {
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
                    Some(Res::Unknown) => res::RegionKind::Unknown,
                    Some(Res::TypeAlias(_) | Res::Def(_) | Res::Var(_) | Res::VariantCase(..)) => {
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
        generics: Option<ast::Generics>,
        f: impl FnOnce(&mut Self) -> T,
    ) -> (Option<res::Generics>, T) {
        if let Some(generics) = generics {
            let names = generics
                .params
                .iter()
                .map(|param| {
                    self.declare_param(param.name.symbol);
                    param.name
                })
                .collect::<Vec<_>>();
            self.generic_kinds
                .extend(generics.params.iter().map(|param| {
                    (
                        param.name.symbol,
                        match param.kind {
                            ast::GenericParamKind::Region => res::GenericKind::Region,
                            ast::GenericParamKind::Type => res::GenericKind::Type,
                        },
                    )
                }));
            let value = f(self);
            let kinds = generics
                .params
                .into_iter()
                .map(|param| match param.kind {
                    ast::GenericParamKind::Region => res::GenericKind::Region,
                    ast::GenericParamKind::Type => res::GenericKind::Type,
                })
                .collect();
            (
                Some(res::Generics {
                    loc: generics.loc,
                    names,
                    kinds,
                }),
                value,
            )
        } else {
            (None, f(self))
        }
    }
    fn resolve_type_path(&mut self, path: &Path, loc: SrcLoc) -> Option<res::TypeName> {
        let name = *path.segments().last().unwrap();
        match self.resolve_path(path) {
            Err(err) => {
                self.path_res_error(path, loc, err);
                None
            }
            Ok(Res::Param(index)) => {
                if self.generic_kinds[&name.symbol] != res::GenericKind::Type {
                    self.diag.add_diagnostic(
                        format!("Generic kind mismatch for '{}'", name.symbol),
                        name.loc,
                    );
                }
                Some(res::TypeName::Param(name.symbol, index))
            }
            Ok(Res::Unknown) => None,
            Ok(Res::TypeAlias(alias)) => Some(alias.into_type_name()),
            Ok(Res::Def(Def::Type(id))) => Some(res::TypeName::UserDefined(self.def_id_for(id))),
            Ok(Res::VariantCase(..)) => {
                self.cannot_use_as_error(name.symbol, "type", name.loc);
                None
            }
            Ok(Res::Def(Def::Function(_) | Def::Module(_)) | Res::LocalRegion(_) | Res::Var(_)) => {
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
            ast::TypeKind::Uint => {
                res::TypeKind::Named(res::TypeName::Uint, Box::new(res::GenericArgs::NONE))
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
            ast::TypeKind::Tuple(fields) => res::TypeKind::Tuple(
                fields
                    .into_iter()
                    .map(|field| self.resolve_type(field))
                    .collect(),
            ),
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
    fn fresh_region(&mut self) -> LocalRegionId {
        let region_id = LocalRegionId::new(self.vars);
        self.regions += 1;
        region_id
    }
    fn declare_region(&mut self, region: Symbol) -> LocalRegionId {
        let region_id = self.fresh_region();
        self.env.insert(region, Res::LocalRegion(region_id));
        region_id
    }
    fn fresh_var(&mut self) -> VarId {
        let var_id = VarId::new(self.vars);
        self.vars += 1;
        var_id
    }
    fn declare_var(&mut self, var: Symbol) -> VarId {
        let var_id = self.fresh_var();
        self.env.insert(var, Res::Var(var_id));
        var_id
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
        let old_module = self.current_module.replace(module);
        let value = self.in_scope(|this| {
            this.env.extend(
                this.decl_info.modules[&module]
                    .items
                    .iter()
                    .map(|(&name, &def)| (name, def.into())),
            );
            f(this)
        });
        self.current_module = old_module;
        value
    }
    fn resolve_int_lit(
        &mut self,
        _: SrcLoc,
        ast::IntLit { value, kind }: ast::IntLit,
    ) -> res::IntegerLiteral {
        res::IntegerLiteral {
            value,
            kind: match kind {
                Some(ast::NumberKind::Signed) => res::IntegerLiteralKind::Signed,
                Some(ast::NumberKind::Unsigned) => res::IntegerLiteralKind::Unsigned,
                None => res::IntegerLiteralKind::Implicit,
            },
        }
    }
    fn resolve_pattern(&mut self, pattern: ast::Pattern) -> res::Pattern {
        let loc = pattern.loc;
        let kind = match pattern.kind {
            ast::PatternKind::Unit => res::PatternKind::Unit,
            ast::PatternKind::Tuple(fields) => res::PatternKind::Tuple(
                fields
                    .into_iter()
                    .map(|field| self.resolve_pattern(field))
                    .collect(),
            ),
            ast::PatternKind::Int(lit) => res::PatternKind::Int(self.resolve_int_lit(loc, lit)),
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
    fn resolve_sub_path(
        &mut self,
        res: Res,
        segment: Ident,
        head: Ident,
    ) -> Result<Res, NameResolutionError> {
        Ok(match res {
            Res::Unknown => Res::Unknown,
            Res::Def(Def::Module(module)) => {
                if let Some(&def) = self.decl_info.modules[&module].items.get(&segment.symbol) {
                    def.into()
                } else {
                    return Err(NameResolutionError::NotInScope);
                }
            }
            Res::Def(Def::Type(id)) => {
                let type_def = &self.decl_info.type_defs[&id];
                let res = 'a: {
                    let (cases, case_map) = match &type_def.kind {
                        TypeDefInfoKind::Record { .. } => {
                            break 'a None;
                        }
                        TypeDefInfoKind::Variant { cases, case_map } => (cases, case_map),
                    };
                    if let Some(&case) = case_map.get(&segment.symbol) {
                        Some(Res::VariantCase(ModuleNodeId(id.0, cases[case].id)))
                    } else {
                        None
                    }
                };
                if let Some(res) = res {
                    res
                } else {
                    return Err(NameResolutionError::TypeRelative(
                        res::TypeName::UserDefined(self.def_id_for(id)),
                        vec![segment],
                    ));
                }
            }
            Res::Var(var) => {
                return Err(NameResolutionError::VariableField(
                    head.loc,
                    res::Var(head.symbol, var),
                    Vec::new(),
                ));
            }
            Res::TypeAlias(alias) => {
                return Err(NameResolutionError::TypeRelative(
                    alias.into_type_name(),
                    vec![segment],
                ));
            }
            Res::Def(Def::Function(_))
            | Res::Param(_)
            | Res::LocalRegion(_)
            | Res::VariantCase(..) => return Err(NameResolutionError::InvalidPathStart),
        })
    }
    fn resolve_path(&mut self, path: &Path) -> Result<Res, NameResolutionError> {
        let head_seg = *path.head();
        let Some(head) = self.resolve_name(head_seg.symbol) else {
            return Err(NameResolutionError::NotInScope);
        };
        let mut curr = head;
        for (index, &segment) in path.segments_iter().into_iter().enumerate().skip(1) {
            curr = self
                .resolve_sub_path(curr, segment, head_seg)
                .map_err(|mut err| {
                    match &mut err {
                        NameResolutionError::TypeRelative(.., fields)
                        | NameResolutionError::VariableField(.., fields) => {
                            *fields = path.segments()[index..].to_vec()
                        }
                        _ => (),
                    }

                    err
                })?;
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
            NameResolutionError::TypeRelative(..) => {
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
                let args = self.resolve_generic_args(args);
                match error {
                    NameResolutionError::NotInScope | NameResolutionError::InvalidPathStart => {
                        self.path_res_error(&path, loc, error);
                    }
                    NameResolutionError::TypeRelative(.., ref methods) if methods.len() > 1 => {
                        self.path_res_error(&path, loc, error);
                    }
                    NameResolutionError::TypeRelative(ty, methods) => {
                        let mut methods = methods.into_iter();
                        let method = methods.next().unwrap();
                        return res::ExprKind::TypeRelativePath(ty, method, Box::new(args));
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
                Res::Unknown => res::ExprKind::Err,
                Res::Var(id) => {
                    let name = path.into_last().symbol;
                    self.error_on_generic_args("var", name, loc, args);
                    res::ExprKind::Var(res::Var(name, id))
                }
                Res::Def(Def::Function(function)) => res::ExprKind::Function(
                    res::FunctionDefId(self.def_id_for(function)),
                    Box::new(self.resolve_generic_args(args)),
                ),
                Res::Param(_)
                | Res::LocalRegion(_)
                | Res::Def(Def::Module(_) | Def::Type(_))
                | Res::TypeAlias(_) => {
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
            ast::ExprKind::Unsafe(expr) => {
                let expr = self.resolve_expr(*expr);
                res::ExprKind::Unsafe(Box::new(expr))
            }
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
            ast::ExprKind::Tuple(fields) => res::ExprKind::Tuple(
                fields
                    .into_iter()
                    .map(|field| self.resolve_expr(field))
                    .collect(),
            ),
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
            ast::ExprKind::Number(lit) => res::ExprKind::Int(self.resolve_int_lit(loc, lit)),
            ast::ExprKind::Bool(value) => res::ExprKind::Bool(value),
            ast::ExprKind::Return(value) => {
                res::ExprKind::Return(Box::new(self.resolve_expr(*value)))
            }
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
            ast::ExprKind::Panic => res::ExprKind::Panic,
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
            ast::ExprKind::MethodCall(rcvr, method, args) => res::ExprKind::MethodCall(
                Box::new(self.resolve_expr(*rcvr)),
                method,
                args.into_iter().map(|arg| self.resolve_expr(arg)).collect(),
            ),
            ast::ExprKind::Array(fields) => {
                /*
                   [a_1,a_2,...,a_n]
                   do
                       let mut l = std.arrays.new();
                       do in r std.arrays.push(mut[r] l, a_1) end;
                       do in r std.arrays.push(mut[r] l, a_2) end;
                       ..
                       do in r std.arrays.push(mut[r] l, a_n) end;
                       l
                   end
                */
                let array_list_path = Path::new(vec![
                    Ident::new(Symbol::STD, loc),
                    Ident::new(Symbol::intern("arrays"), loc),
                    Ident::new(Symbol::intern("ArrayList"), loc),
                ]);
                let with_capacity_path = array_list_path
                    .clone()
                    .with_extra_segment(Ident::new(Symbol::intern("with_capacity"), loc));
                let with_cap_expr = self.resolve_path_as_expr(loc, with_capacity_path, None);
                let push_expr = |this: &mut Resolve<'_>| {
                    let push_path = array_list_path
                        .clone()
                        .with_extra_segment(Ident::new(Symbol::intern("push"), loc));
                    this.resolve_path_as_expr(loc, push_path, None)
                };

                let local_name = Ident::new(Symbol::intern("list"), loc);
                let local_var = self.fresh_var();
                let var_expr = move || res::Expr {
                    loc,
                    kind: res::ExprKind::Var(res::Var(local_name.symbol, local_var)),
                };
                fn make_call(
                    loc: SrcLoc,
                    callee: res::Expr,
                    args: impl IntoIterator<Item = res::Expr>,
                ) -> res::Expr {
                    res::Expr {
                        loc,
                        kind: res::ExprKind::Call(Box::new(callee), args.into_iter().collect()),
                    }
                }
                let let_stmt = res::Stmt {
                    loc,
                    kind: res::StmtKind::Let(Box::new(res::LetBinding {
                        pattern: res::Pattern {
                            loc,
                            kind: res::PatternKind::Binding(
                                None,
                                ast::Mutable::Mutable,
                                local_name,
                                local_var,
                            ),
                        },
                        ty: None,
                        value: make_call(
                            loc,
                            res::Expr {
                                loc,
                                kind: with_cap_expr,
                            },
                            [res::Expr {
                                loc,
                                kind: res::ExprKind::Int(res::IntegerLiteral {
                                    value: fields.len() as u64,
                                    kind: res::IntegerLiteralKind::Implicit,
                                }),
                            }],
                        ),
                    })),
                };
                let stmts = std::iter::once(let_stmt).chain(fields.into_iter().enumerate().map(
                    |(i, field)| {
                        let region_name = Symbol::intern(&format!("r{}", i + 1));
                        let region = self.fresh_region();
                        let borrowed_var = res::Expr {
                            loc,
                            kind: res::ExprKind::Borrow(Box::new(res::BorrowExpr {
                                mutable: ast::Mutable::Mutable,
                                place: var_expr(),
                                region: res::Region {
                                    loc,
                                    kind: res::RegionKind::Local(region_name, region),
                                },
                            })),
                        };
                        let push_call = make_call(
                            loc,
                            res::Expr {
                                loc,
                                kind: push_expr(self),
                            },
                            [borrowed_var, self.resolve_expr(field)],
                        );
                        let block_expr = make_block_expr(loc, Some(region), [], push_call);
                        res::Stmt {
                            loc,
                            kind: res::StmtKind::Expr(block_expr),
                        }
                    },
                ));

                return make_block_expr(loc, None, stmts, var_expr());
            }
        };
        res::Expr { loc, kind }
    }
    fn resolve_signature(
        &mut self,
        params: Vec<ast::Param>,
        return_type: ast::Type,
    ) -> (Box<[res::Param]>, res::Signature) {
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
            res::Signature {
                params: param_tys.into_boxed_slice(),
                return_type,
            },
        )
    }
    fn resolve_function(&mut self, function: ast::Function) -> res::Function {
        self.resolve_item(|this| {
            let (generics, rest) = this.resolve_generics(function.generics, |this| {
                let (params, sig) = this.resolve_signature(function.params, function.return_type);
                (
                    params,
                    sig,
                    function.body.map(|body| this.resolve_expr(body)),
                )
            });
            let generics = generics.map(Box::new);
            let (params, sig, body) = rest;
            let res::Signature {
                params: param_tys,
                return_type,
            } = sig;
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
        impl_: Option<ast::TypeImpl>,
    ) -> (Option<DefId>, res::TypeDefKind) {
        let kind = match body {
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
        };
        let impl_ = if let Some(impl_) = impl_ {
            let def_id = self.def_id_for(ModuleNodeId(type_id.0, impl_.id));
            let methods = impl_
                .methods
                .into_iter()
                .map(|method| {
                    let id = self.def_id_for(ModuleNodeId(type_id.0, method.id));
                    let annotations = self.resolve_annotations(method.annotations);
                    let function = self.resolve_function(method.function);
                    self.add_node(
                        id,
                        res::Node::Method(Box::new(res::Method {
                            annotations,
                            function,
                        })),
                    )
                })
                .collect();
            self.add_node(
                def_id,
                res::Node::Impl(Box::new(res::TypeImpl {
                    ty: self.def_id_for(type_id),
                    methods,
                })),
            );
            Some(def_id)
        } else {
            None
        };
        (impl_, kind)
    }
    fn resolve_type_def(&mut self, id: ModuleNodeId, type_def: ast::TypeDef) -> res::TypeDef {
        self.resolve_item(|this| {
            let (generics, (impl_, kind)) = {
                this.resolve_generics(type_def.generics, |this| {
                    this.resolve_type_def_body(id, type_def.kind, type_def.imp)
                })
            };
            res::TypeDef {
                name: type_def.name,
                generics: generics.map(Box::new),
                kind,
                impl_,
            }
        })
    }
    fn resolve_item<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let old_generics = self.generic_kinds.clone();
        let old_generic_count = self.generics;
        let value = self.in_scope(|this| f(this));
        self.generic_kinds = old_generics;
        self.generics = old_generic_count;
        self.vars = 0;
        value
    }
    #[track_caller]
    fn def_id_for(&self, id: ModuleNodeId) -> DefId {
        *self
            .decl_info
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
    fn def_id_for_module(&self, module: ModuleId) -> DefId {
        self.decl_info.modules[&module].id
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
    fn resolve_import_tree(
        &mut self,
        head: Ident,
        path: Path,
        tree: ast::ImportTree,
    ) -> res::ImportTree {
        let (name, children): (_, Box<[_]>) = match tree.tail {
            ast::ImportTreeTail::Alias(name) => {
                let res = self.resolve_path(&path).unwrap_or_else(|err| {
                    self.path_res_error(&path, head.loc, err);
                    Res::Unknown
                });
                self.declare_item(name, "import alias", res);
                (
                    name,
                    Box::new([res::ImportTree {
                        name,
                        children: Box::new([]),
                    }]),
                )
            }
            ast::ImportTreeTail::Children(sub_trees) => (
                tree.current,
                if let Some(sub_trees) = sub_trees {
                    if sub_trees.is_empty()
                        && let Err(err) = self.resolve_path(&path)
                    {
                        self.path_res_error(&path, head.loc, err)
                    }
                    sub_trees
                        .into_iter()
                        .map(|tree| {
                            let path = path.clone().with_extra_segment(tree.current);
                            self.resolve_import_tree(head, path, tree)
                        })
                        .collect()
                } else {
                    match self.resolve_path(&path) {
                        Ok(path) => {
                            self.declare_item(tree.current, "import alias", path);
                        }
                        Err(err) => self.path_res_error(&path, head.loc, err),
                    }
                    Box::new([])
                },
            ),
        };
        res::ImportTree { name, children }
    }
    fn resolve_import(&mut self, import: ast::Import) -> res::ImportTree {
        self.resolve_import_tree(
            import.tree.current,
            Path::new(vec![import.tree.current]),
            import.tree,
        )
    }
    fn resolve_module(&mut self, module: ast::Module) {
        self.resolve_in_module_scope(module.id, |this| {
            let mut mod_items = Vec::with_capacity(module.items.len() + module.child_modules.len());
            for item in module.items.into_iter() {
                let node_id = ModuleNodeId(module.id, item.id);
                let id = this.decl_info.item_id_to_def_id[&node_id];
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
                        ast::ItemKind::Import(import) => {
                            let import = this.resolve_import(import);
                            res::ItemKind::Import(Box::new(import))
                        }
                    },
                };
                this.add_item(id, item);
                mod_items.push(id);
            }
            for child in module.child_modules {
                let id = this.decl_info.modules[&child.id].id;
                this.resolve_module(child);
                mod_items.push(id);
            }
            let item = res::Item {
                id: this.def_id_for_module(module.id),
                loc: SrcLoc::dummy().with_file(module.name),
                annotations: Vec::new().into_boxed_slice(),
                kind: res::ItemKind::Module(Box::new(res::Module {
                    name: Ident::new(module.name, SrcLoc::dummy().with_file(module.name)),
                    items: mod_items.into_boxed_slice(),
                })),
            };
            this.add_item(item.id, item);
        })
    }
    pub fn resolve(
        config: Config,
        modules: Vec<ast::Module>,
    ) -> Result<GlobalContext, ResolveErrored> {
        //First pass : Declare everything
        let diag = DiagnosticReporter::new();
        let decl_info = decl::Declare::new(&diag).declare(&modules);
        let mut this = Resolve::new(config, &decl_info);
        //Second pass : Resolve
        let nodes = {
            for module in modules.into_iter() {
                this.resolve_module(module);
            }
            std::mem::take(&mut this.nodes)
                .into_iter_enumerated()
                .map(|(id, node)| node.unwrap_or_else(|| panic!("missing node for '{:?}'", id)))
                .collect()
        };

        let mut builtins = Builtins::default();
        if let Some(builtin_module) = decl_info.builtin_module {
            for (name, &def) in &decl_info.modules[&builtin_module].items {
                let Some(builtin) = Builtin::find(*name) else {
                    continue;
                };
                let Def::Function(id) = def else {
                    continue;
                };
                let id = this.def_id_for(id);
                builtins.insert(builtin, id);
            }
        }

        let resolved_diag = this.diag;
        let context = build_global_context(this.config, nodes, builtins, decl_info.parents);
        if !diag.report_all() & !resolved_diag.report_all() {
            Ok(context)
        } else {
            Err(ResolveErrored)
        }
    }
}
