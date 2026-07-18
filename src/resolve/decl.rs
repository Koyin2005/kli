use std::collections::HashMap;

use crate::{
    Symbol,
    ast::{self, ModuleId, NodeId, TypeImpl},
    def_ids::DefId,
    diagnostics::DiagnosticReporter,
    ident::Ident,
    resolve::{
        CaseInfo, Def, FieldInfo, ModuleInfo, ModuleItems, ModuleNodeId, TypeDefInfoKind,
        TypeImplInfo, TypeInfo,
    },
    src_loc::SrcLoc,
};

struct DeclareInBody<'d, 'root> {
    declare: &'root mut Declare<'d>,
    module: ModuleId,
}
impl DeclareInBody<'_, '_> {
    fn declare_in_exprs(&mut self, expr: &ast::Expr) {
        match &expr.kind {
            ast::ExprKind::Unit
            | ast::ExprKind::String(_)
            | ast::ExprKind::Bool(_)
            | ast::ExprKind::Number(..)
            | ast::ExprKind::Panic
            | ast::ExprKind::Path(..) => (),
            ast::ExprKind::Annotate(expr, _)
            | ast::ExprKind::Unsafe(expr)
            | ast::ExprKind::Deref(expr)
            | ast::ExprKind::AddressOf(expr)
            | ast::ExprKind::Field(expr, _) => self.declare_in_exprs(expr),
            ast::ExprKind::Print(expr) => {
                if let Some(expr) = expr {
                    self.declare_in_exprs(expr);
                }
            }
            ast::ExprKind::Call(callee, args) | ast::ExprKind::MethodCall(callee, _, args) => {
                self.declare_in_exprs(callee);
                for arg in args {
                    self.declare_in_exprs(arg);
                }
            }
            ast::ExprKind::Tuple(fields) => {
                for field in fields {
                    self.declare_in_exprs(field)
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
                self.declare.declare_def_id_for(self.module, lambda.id);
                self.declare_in_exprs(&lambda.body);
            }
            ast::ExprKind::Block(block_body, _) => {
                for stmt in block_body.stmts.iter() {
                    match &stmt.kind {
                        ast::StmtKind::Expr(expr) => self.declare_in_exprs(expr),
                        ast::StmtKind::Let(let_binding) => {
                            self.declare_in_exprs(&let_binding.value)
                        }
                    }
                }
                self.declare_in_exprs(&block_body.expr);
            }
            ast::ExprKind::Return(expr) => self.declare_in_exprs(expr),
            ast::ExprKind::Record(ast::RecordExpr { fields })
            | ast::ExprKind::NamedRecord(_, fields) => {
                for field in fields.iter() {
                    self.declare_in_exprs(&field.value);
                }
            }
        }
    }
}

#[derive(Default, Debug)]
pub(super) struct DeclareResults {
    pub def_ids: usize,
    pub parents: HashMap<DefId, DefId>,
    pub item_id_to_def_id: HashMap<ModuleNodeId, DefId>,
    pub modules: HashMap<ModuleId, ModuleInfo>,
    pub type_defs: HashMap<ModuleNodeId, TypeInfo>,
    pub impls_: HashMap<ModuleNodeId, TypeImplInfo>,
    pub builtin_module: Option<ModuleId>,
    pub top_level_modules: HashMap<Symbol, ModuleId>,
}

pub(super) struct Declare<'d> {
    current_item: Option<DefId>,
    results: DeclareResults,
    current_items: ModuleItems,
    prev_items: Vec<ModuleItems>,
    diag: &'d DiagnosticReporter,
}

impl<'d> Declare<'d> {
    pub fn new(diag: &'d DiagnosticReporter) -> Self {
        Self {
            current_item: None,
            results: DeclareResults::default(),
            current_items: ModuleItems::default(),
            prev_items: Vec::new(),
            diag,
        }
    }

    fn in_new_module(&mut self, module: &ast::Module, f: impl FnOnce(&mut Self)) {
        self.declare_item(
            Ident {
                symbol: module.name,
                loc: SrcLoc::dummy(),
            },
            "module",
            Def::Module(module.id),
        );
        self.prev_items
            .push(std::mem::take(&mut self.current_items));
        let def_id = self.next_def_id();
        self.with_parent_def_id(def_id, f);
        self.results.modules.insert(
            module.id,
            ModuleInfo {
                id: def_id,
                items: std::mem::take(&mut self.current_items),
            },
        );
        self.current_items = self.prev_items.pop().unwrap();
    }
    fn next_def_id(&mut self) -> DefId {
        let def_id = DefId::new(self.results.def_ids);
        self.results.def_ids += 1;
        if let Some(current_parent) = self.current_item {
            self.results.parents.insert(def_id, current_parent);
        }
        def_id
    }
    fn with_parent_def_id<T>(&mut self, id: DefId, f: impl FnOnce(&mut Self) -> T) -> T {
        let parent = self.current_item.replace(id);
        let value = f(self);
        self.current_item = parent;
        value
    }

    fn declare_item(&mut self, name: Ident, kind: &str, def: Def) {
        match self.current_items.entry(name.symbol) {
            std::collections::hash_map::Entry::Occupied(occupied) => {
                self.diag.add_diagnostic(
                    format!("Cannot redeclare '{}' '{}'", kind, occupied.key()),
                    name.loc,
                );
            }
            std::collections::hash_map::Entry::Vacant(vacant) => {
                vacant.insert(def);
            }
        }
    }

    fn declare_def_id_for(&mut self, module: ModuleId, id: NodeId) -> DefId {
        let def_id = self.next_def_id();
        self.results
            .item_id_to_def_id
            .insert(ModuleNodeId(module, id), def_id);
        def_id
    }

    fn declare_function_body(&mut self, id: DefId, function: &ast::Function, module: ModuleId) {
        self.with_parent_def_id(id, |this| {
            if let Some(body) = function.body.as_ref() {
                DeclareInBody {
                    declare: this,
                    module: module,
                }
                .declare_in_exprs(body);
            }
        })
    }
    fn declare_function(&mut self, id: ModuleNodeId, def_id: DefId, function: &ast::Function) {
        self.declare_item(function.name, "function", Def::Function(id));
        self.declare_function_body(def_id, function, id.0);
    }
    fn declare_module_items(&mut self, module: &ast::Module) {
        self.in_new_module(module, |this| {
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
                        this.declare_function(full_id, def_id, function);
                    }
                    ast::ItemKind::Import(..) => {}
                }
            }
            for module in &module.child_modules {
                this.declare_module_items(module);
            }
        })
    }
    fn declare_impl(&mut self, mod_id: ModuleId, imp_: &TypeImpl) {
        let id = self.declare_def_id_for(mod_id, imp_.id);
        let methods = self.with_parent_def_id(id, |this| {
            imp_.methods
                .iter()
                .map(|method| {
                    let id = this.declare_def_id_for(mod_id, method.id);
                    this.declare_function_body(id, &method.function, mod_id);
                    (method.function.name.symbol, ModuleNodeId(mod_id, method.id))
                })
                .collect()
        });
        self.results
            .impls_
            .insert(ModuleNodeId(mod_id, imp_.id), TypeImplInfo { methods });
    }
    fn declare_type_def(
        &mut self,
        mod_node_id: ModuleNodeId,
        def_id: DefId,
        type_def: &ast::TypeDef,
    ) {
        let name = type_def.name;
        let (kind, impl_) = self.with_parent_def_id(def_id, |this| {
            let info = match type_def.kind {
                ast::TypeDefKind::Record(ref record) => this.with_parent_def_id(def_id, |this| {
                    let mut fields = Vec::new();
                    for field in &record.fields {
                        this.declare_def_id_for(mod_node_id.0, field.id);
                        fields.push(FieldInfo {
                            _name: field.name,
                            _id: field.id,
                        });
                    }
                    TypeDefInfoKind::Record { _fields: fields }
                }),
                ast::TypeDefKind::Variant(ref cases) => {
                    let cases = cases
                        .iter()
                        .map(|case| {
                            let def_id = this.declare_def_id_for(mod_node_id.0, case.id);
                            if let Some(ty) = case.ty.as_ref() {
                                this.with_parent_def_id(def_id, |this| {
                                    this.declare_def_id_for(mod_node_id.0, ty.id);
                                })
                            }
                            CaseInfo {
                                name: case.name,
                                id: case.id,
                            }
                        })
                        .collect::<Vec<_>>();
                    TypeDefInfoKind::Variant {
                        case_map: cases
                            .iter()
                            .enumerate()
                            .map(|(i, case)| (case.name.symbol, i))
                            .collect(),
                        cases,
                    }
                }
            };
            let impl_ = if let Some(ref impl_) = type_def.imp {
                this.declare_impl(mod_node_id.0, impl_);
                Some(ModuleNodeId(mod_node_id.0, impl_.id))
            } else {
                None
            };
            (info, impl_)
        });
        self.results
            .type_defs
            .insert(mod_node_id, TypeInfo { kind, impl_ });
        self.declare_item(name, "type", Def::Type(mod_node_id));
    }
    pub fn declare(mut self, modules: &[ast::Module]) -> DeclareResults {
        for module in modules.iter() {
            if module.name == Symbol::BUILTINS {
                self.results.builtin_module = Some(module.id);
            }
            self.declare_module_items(module);
            self.results
                .top_level_modules
                .insert(module.name, module.id);
        }
        self.results
    }
}
