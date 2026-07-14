use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
};

use crate::{
    Symbol,
    builtins::Builtins,
    captures::{self, captures},
    config::Config,
    def_ids::DefId,
    diagnostics::DiagnosticReporter,
    ident::Ident,
    index_vec::IndexVec,
    lang_items::LangItems,
    resolved_ast::{self, AnnotationKind, Item, ItemKind, Node, TypeDef},
    scheme::Scheme,
    src_loc::SrcLoc,
    typecheck::infer::TypeInfer,
    typed_ast::FieldId,
    types::{
        CaseId, FunctionSig, GenericArg, GenericArgsRef, GenericKind, GenericParam, Region, Type,
        lower::Lower,
    },
};

pub struct Cache<Key, R> {
    value: RefCell<HashMap<Key, R>>,
}
impl<Key, R> Default for Cache<Key, R> {
    fn default() -> Self {
        Self {
            value: RefCell::default(),
        }
    }
}
impl<Key: Copy + Eq + Hash, R: Clone> Cache<Key, R> {
    pub fn compute(&self, key: Key, f: impl FnOnce(Key) -> R) -> R {
        if let Some(value) = self.value.borrow().get(&key) {
            return value.clone();
        };
        let value = f(key);
        self.value.borrow_mut().insert(key, value.clone());
        value
    }
}
#[derive(Clone, Copy)]
pub struct Field {
    pub id: DefId,
    pub name: Symbol,
}
impl Field {
    pub fn type_of(self, args: GenericArgsRef, ctxt: CtxtRef<'_>) -> Type {
        ctxt.type_of(self.id).bind(args)
    }
}
#[derive(Clone, Copy)]
pub struct Case {
    pub id: DefId,
    pub name: Symbol,
    pub field: Option<Field>,
}
impl Case {
    #[track_caller]
    pub fn expect_field(self) -> Field {
        self.field.expect("should have a field")
    }
    pub fn payload_type(self, args: GenericArgsRef<'_>, ctxt: CtxtRef<'_>) -> Type {
        Type::tuple(
            self.field
                .into_iter()
                .map(|field| field.type_of(args, ctxt)),
        )
    }
}
pub enum TypeDefKind {
    Record(IndexVec<FieldId, Field>),
    Variant(IndexVec<CaseId, Case>),
}
pub struct TypeDefInfo {
    pub name: Symbol,
    pub kind: TypeDefKind,
}
impl TypeDefInfo {
    pub fn case_value(&self, case: CaseId) -> u32 {
        _ = self;
        case.into_usize() as u32
    }
    #[track_caller]
    pub fn case_with_id(&self, id: DefId) -> (CaseId, &Case) {
        self.expect_cases()
            .iter_enumerated()
            .find_map(|(case_id, case)| (case.id == id).then_some((case_id, case)))
            .expect("unknown case")
    }
    #[track_caller]
    pub fn case(&self, index: CaseId) -> &Case {
        &self.expect_cases()[index]
    }
    #[track_caller]
    pub fn expect_cases(&self) -> &IndexVec<CaseId, Case> {
        let TypeDefKind::Variant(cases) = &self.kind else {
            panic!("Expected a variant type")
        };
        cases
    }
    pub fn cases(&self) -> Option<&IndexVec<CaseId, Case>> {
        let TypeDefKind::Variant(cases) = &self.kind else {
            return None;
        };
        Some(cases)
    }
    #[track_caller]
    pub fn fields(&self) -> &IndexVec<FieldId, Field> {
        let TypeDefKind::Record(fields) = &self.kind else {
            panic!("Expected a record type")
        };
        fields
    }
    pub fn all_fields(&self) -> impl Iterator<Item = Field> {
        let (rec_iter, case_iter) = match &self.kind {
            TypeDefKind::Record(fields) => (Some(fields.iter().copied()), None),
            TypeDefKind::Variant(cases) => (None, Some(cases.iter().flat_map(|case| case.field))),
        };
        rec_iter
            .into_iter()
            .flatten()
            .chain(case_iter.into_iter().flatten())
    }
}

#[derive(Debug, Clone, Default)]
pub struct Generics {
    params: Vec<GenericParam>,
}
impl Generics {
    const fn new() -> Self {
        Self { params: Vec::new() }
    }
    pub const fn is_empty(&self) -> bool {
        self.params.is_empty()
    }
    #[track_caller]
    pub fn kind(&self, index: usize) -> GenericKind {
        let Some(kind) = self.get_kind(index) else {
            panic!("generic param for {:?} not found", index)
        };
        kind
    }
    #[track_caller]
    pub fn get_kind(&self, index: usize) -> Option<GenericKind> {
        self.params.as_slice().get(index).map(|param| param.kind)
    }
    pub const fn count(&self) -> usize {
        self.params.len()
    }
    pub fn kinds(&self) -> impl Iterator<Item = GenericKind> {
        self.params.iter().map(|param| param.kind)
    }
    pub fn instantiate(&self, infer: &mut TypeInfer, loc: SrcLoc) -> Vec<GenericArg> {
        self.kinds()
            .map(|kind| match kind {
                GenericKind::Region => GenericArg::Region(Region::Infer(infer.fresh_region(loc))),
                GenericKind::Type => GenericArg::Type(Type::Infer(infer.fresh_ty(loc))),
            })
            .collect()
    }
    pub fn instantiate_unknown(&self) -> Vec<GenericArg> {
        self.kinds()
            .map(|kind| match kind {
                GenericKind::Region => GenericArg::Region(Region::Unknown),
                GenericKind::Type => GenericArg::Type(Type::Unknown),
            })
            .collect()
    }
    pub fn instantiate_identity(&self) -> Vec<GenericArg> {
        self.params
            .iter()
            .enumerate()
            .map(|(i, param)| match param.kind {
                GenericKind::Region => GenericArg::Region(Region::Param(param.name, i)),
                GenericKind::Type => GenericArg::Type(Type::Param(param.name, i)),
            })
            .collect()
    }
}
pub struct GlobalContext {
    diag: DiagnosticReporter,
    idents: Cache<DefId, Option<Ident>>,
    generics: Cache<DefId, Generics>,
    captures: Cache<DefId, Option<captures::CaptureSet>>,
    lang_items: Cache<(), LangItems>,
    parents: HashMap<DefId, DefId>,
    nodes: IndexVec<DefId, Node>,
    builtins: Builtins,
    std_lib: Cache<(), Option<DefId>>,
    ty_cache: Cache<DefId, Scheme<Type>>,
    config: Config,
}
impl GlobalContext {
    pub fn as_ref(&self) -> CtxtRef<'_> {
        CtxtRef(self)
    }
}
#[derive(Copy, Clone)]
pub struct CtxtRef<'a>(&'a GlobalContext);

impl CtxtRef<'_> {
    pub fn config(&self) -> &Config {
        &self.0.config
    }
    pub fn node(&self, id: DefId) -> &Node {
        &self.0.nodes[id]
    }
    #[track_caller]
    pub fn annotations(&self, id: DefId) -> &[resolved_ast::Annotation] {
        if let Some(item) = self.node(id).item() {
            &item.annotations
        } else {
            &[]
        }
    }
    #[track_caller]
    fn expect_item(&self, id: DefId) -> &Item {
        let Node::Item(item) = self.node(id) else {
            unreachable!("not an item")
        };
        item
    }
    #[track_caller]
    pub fn type_def(self, id: DefId) -> TypeDefInfo {
        let type_def = self.expect_type(id);
        lower_type_def(self, type_def)
    }
    #[track_caller]
    fn expect_type(&self, id: DefId) -> &TypeDef {
        self.expect_item(id).expect_type_def()
    }
    pub fn is_type_recursive(self, id: DefId) -> bool {
        fn is_ty_recursive(ctxt: CtxtRef<'_>, ty: &Type, seen_ids: &mut HashSet<DefId>) -> bool {
            match ty {
                Type::Array(ty, _) => is_ty_recursive(ctxt, ty, seen_ids),
                Type::Bool
                | Type::Byte
                | Type::Char
                | Type::String
                | Type::Unit
                | Type::Unknown
                | Type::Infer(_)
                | Type::Param(..)
                | Type::RawPointer(_)
                | Type::Int
                | Type::Imm(..)
                | Type::Mut(..)
                | Type::Function(..)
                | Type::Never => false,
                Type::Record(fields) => fields
                    .iter()
                    .any(|field| is_ty_recursive(ctxt, &field.ty, seen_ids)),
                Type::Tuple(fields) => fields
                    .iter()
                    .any(|field| is_ty_recursive(ctxt, field, seen_ids)),
                Type::Named(id, _, args) => {
                    if !seen_ids.insert(*id) {
                        return true;
                    }
                    for field in ctxt.type_def(*id).all_fields() {
                        if is_ty_recursive(ctxt, &field.type_of(args, ctxt), seen_ids) {
                            return true;
                        }
                    }
                    false
                }
            }
        }
        is_ty_recursive(
            self,
            &Type::Named(
                id,
                self.expect_type(id).name.symbol,
                self.generics(id).instantiate_identity(),
            ),
            &mut HashSet::new(),
        )
    }

    pub fn span(self, id: DefId) -> SrcLoc {
        match self.node(id) {
            Node::Item(item) => item.ident().loc,
            Node::Lambda(lambda) => lambda.loc,
            Node::Field(field_def) => field_def.name.loc,
            Node::Case(case_def) => case_def.name.loc,
            Node::CaseField(case_field) => case_field.ty.loc,
        }
    }
    #[track_caller]
    pub fn expect_ident(self, id: DefId) -> Ident {
        self.ident(id).expect("expected an ident")
    }
    pub fn ident(self, id: DefId) -> Option<Ident> {
        self.0.idents.compute(id, |id| {
            Some(match self.node(id) {
                Node::Item(item) => item.ident(),
                Node::Case(case_def) => case_def.name,
                Node::CaseField(field) => Ident {
                    symbol: Symbol::ZERO,
                    loc: field.ty.loc,
                },
                Node::Field(field_def) => field_def.name,
                Node::Lambda(_) => return None,
            })
        })
    }
    pub fn type_of(self, id: DefId) -> Scheme<Type> {
        self.0.ty_cache.compute(id, |id| {
            Scheme::new(match self.node(id) {
                Node::Case(case_def) => {
                    let parent_id = self.expect_parent(id);
                    let type_def = self.expect_type(parent_id);
                    let name = type_def.name;
                    let variant_ty = Type::Named(
                        parent_id,
                        name.symbol,
                        self.generics(parent_id).instantiate_identity(),
                    );
                    if let Some(ty) = case_def.field.map(|inner| {
                        self.type_of(inner)
                            .bind(&self.generics(parent_id).instantiate_identity())
                    }) {
                        Type::new_function(vec![ty], variant_ty)
                    } else {
                        variant_ty
                    }
                }
                Node::Item(item) => match &item.kind {
                    ItemKind::TypeDef(type_def) => Type::Named(
                        id,
                        type_def.name.symbol,
                        self.generics(id).instantiate_identity(),
                    ),
                    ItemKind::Function(_) => {
                        return self.signature_of(id).map(|signature| {
                            Type::new_function(signature.params, signature.return_type)
                        });
                    }
                    ItemKind::Module(_) | ItemKind::Import(_) => {
                        unreachable!(
                            "cannot get type of {} {}",
                            item.kind_str(),
                            item.ident().symbol
                        )
                    }
                },
                Node::CaseField(field) => Lower::new(self, id, None).lower_type(&field.ty),
                Node::Field(field) => Lower::new(self, id, None).lower_type(&field.ty),
                Node::Lambda(_) => unreachable!("Can't get the type of lambda"),
            })
        })
    }
    pub fn def_id_for_path(self, mut path: impl Iterator<Item = Symbol>) -> Option<DefId> {
        let mut item = {
            let top_level_tem_name = path.next()?;
            self.top_level_items()
                .find(|item| item.ident().symbol == top_level_tem_name)?
        };
        let mut current_item = item.id;
        loop {
            let Some(current) = path.next() else {
                break Some(current_item);
            };
            match &item.kind {
                ItemKind::Function(_) | ItemKind::Import(_) | ItemKind::TypeDef(_) => {
                    if item.ident().symbol == current {
                        current_item = item.id;
                    } else {
                        break None;
                    }
                }
                ItemKind::Module(module) => {
                    let &next_item = module.items.iter().find(|&&item_id| {
                        self.ident(item_id)
                            .is_some_and(|ident| ident.symbol == current)
                    })?;
                    item = self.expect_item(next_item);
                    current_item = item.id;
                }
            }
        }
    }
    pub fn generics(self, id: DefId) -> Generics {
        self.0.generics.compute(id, |id| match self.node(id) {
            Node::Item(item) => match &item.kind {
                ItemKind::Function(function_def) => function_def
                    .generics
                    .as_deref()
                    .map_or_else(Generics::new, lower_generics),
                ItemKind::Module(_) | ItemKind::Import(_) => Generics::new(),
                ItemKind::TypeDef(type_def) => type_def
                    .generics
                    .as_deref()
                    .map_or_else(Generics::new, lower_generics),
            },
            Node::Case(_) | Node::CaseField(_) | Node::Field(_) => {
                self.generics(self.expect_parent(id))
            }
            Node::Lambda(_) => self.generics(self.expect_parent(id)),
        })
    }
    pub fn self_with_anecstors(self, id: DefId) -> impl Iterator<Item = DefId> {
        std::iter::once(id).chain(self.ancestors(id))
    }
    pub fn ancestors(self, id: DefId) -> impl Iterator<Item = DefId> {
        struct Ancestors<'ctxt> {
            ctxt: CtxtRef<'ctxt>,
            current: DefId,
        }
        impl Iterator for Ancestors<'_> {
            type Item = DefId;
            fn next(&mut self) -> Option<Self::Item> {
                let parent = self.ctxt.parent_of(self.current)?;
                self.current = parent;
                Some(parent)
            }
        }
        Ancestors {
            ctxt: self,
            current: id,
        }
    }
    pub fn root_of(self, id: DefId) -> DefId {
        self.ancestors(id).last().unwrap_or(id)
    }
    pub fn parent_of(self, id: DefId) -> Option<DefId> {
        self.0.parents.get(&id).copied()
    }
    #[track_caller]
    pub fn expect_parent(self, id: DefId) -> DefId {
        self.0
            .parents
            .get(&id)
            .copied()
            .expect("should have a parent")
    }
    pub fn all_items(&self) -> impl Iterator<Item = &Item> {
        self.0.nodes.iter().filter_map(Node::item)
    }
    pub fn top_level_items(&self) -> impl Iterator<Item = &Item> {
        self.all_items()
            .filter(|item| self.parent_of(item.id).is_none())
    }
    pub fn main_function(&self) -> Option<(DefId, &resolved_ast::Function)> {
        self.top_level_items()
            .filter_map(|item| {
                let ItemKind::Module(module) = &item.kind else {
                    return None;
                };
                if module.name.symbol != Symbol::MAIN {
                    return None;
                }
                Some(module.items.iter())
            })
            .flatten()
            .find_map(|&item_id| {
                let function = self.expect_item(item_id).function_def()?;
                if function.name.symbol != Symbol::MAIN {
                    return None;
                }
                Some((item_id, function))
            })
    }
    #[track_caller]
    pub fn signature_of(self, id: DefId) -> Scheme<FunctionSig> {
        let function = self.expect_item(id).expect_function_def();
        let lower = Lower::new(self, id, None);
        Scheme::new(FunctionSig::new(
            lower.lower_types(&mut function.param_tys.iter()).collect(),
            lower.lower_type(&function.return_type),
        ))
    }
    #[track_caller]
    pub fn captures(self, id: DefId) -> Option<captures::CaptureSet> {
        self.0.captures.compute(id, |id| captures(self, id))
    }
    pub fn display(&self, id: DefId) -> impl std::fmt::Display {
        struct DisplayNode<'n> {
            node: &'n Node,
        }
        fn fmt(node: &DisplayNode<'_>, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            match node.node {
                Node::Item(item) => write!(f, "{}", item.ident().symbol),
                Node::Case(case_def) => write!(f, "{}", case_def.name.symbol),
                Node::CaseField(_) => write!(f, "{}", Symbol::ZERO),
                Node::Lambda(lambda) => write!(f, "(lambda at {:?})", lambda.loc),
                Node::Field(field) => write!(f, "{}", field.name.symbol),
            }
        }
        impl std::fmt::Display for DisplayNode<'_> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                fmt(self, f)
            }
        }
        DisplayNode {
            node: self.node(id),
        }
    }
    pub fn display_path_for(&self, id: DefId) -> impl std::fmt::Display {
        std::fmt::from_fn(move |f| {
            let mut id = id;
            let mut output = self.display(id).to_string();
            while let Some(parent) = self.parent_of(id) {
                output = format!("{}.{}", self.display(parent), output);
                id = parent;
            }
            f.write_str(&output)
        })
    }
    pub fn diag(&self) -> &DiagnosticReporter {
        &self.0.diag
    }
    pub fn builtins(&self) -> &Builtins {
        &self.0.builtins
    }
    pub fn std_lib_module(self) -> Option<DefId> {
        self.0.std_lib.compute((), |()| {
            self.top_level_items().find_map(|item| {
                let ItemKind::Module(ref module) = item.kind else {
                    return None;
                };
                if module.name.symbol != Symbol::STD {
                    return None;
                }
                Some(item.id)
            })
        })
    }
    pub fn same_module(&self, src: DefId, from: DefId) -> bool {
        let src_module = self.module_of(src);
        let from_module = self.module_of(from);
        src_module == from_module
    }
    pub fn is_opaque(&self, id: DefId) -> bool {
        self.annotations(id)
            .iter()
            .any(|annotation| annotation.kind == AnnotationKind::Opaque)
    }
    pub fn module_of(&self, id: DefId) -> DefId {
        self.self_with_anecstors(id)
            .find(|&id| {
                let Some(item) = self.node(id).item() else {
                    return false;
                };
                let ItemKind::Module(_) = item.kind else {
                    return false;
                };
                true
            })
            .unwrap_or(id)
    }
    pub fn lang_items(self) -> LangItems {
        self.0.lang_items.compute((), |()| LangItems::collect(self))
    }
}
fn lower_generics(generics: &resolved_ast::Generics) -> Generics {
    Generics {
        params: generics
            .kinds
            .iter()
            .zip(generics.names.iter().copied())
            .map(|(kind, name)| GenericParam {
                name: name.symbol,
                kind: match kind {
                    resolved_ast::GenericKind::Region => GenericKind::Region,
                    resolved_ast::GenericKind::Type => GenericKind::Type,
                },
            })
            .collect(),
    }
}
fn lower_type_def(ctxt: CtxtRef<'_>, type_def: &TypeDef) -> TypeDefInfo {
    TypeDefInfo {
        name: type_def.name.symbol,
        kind: match &type_def.kind {
            resolved_ast::TypeDefKind::Record(record) => TypeDefKind::Record(
                record
                    .fields
                    .iter()
                    .map(|&field_id| {
                        let field = ctxt.node(field_id).expect_field();
                        Field {
                            id: field_id,
                            name: field.name.symbol,
                        }
                    })
                    .collect(),
            ),
            resolved_ast::TypeDefKind::Variant(variant) => TypeDefKind::Variant(
                variant
                    .cases
                    .iter()
                    .map(|case_def| Case {
                        id: case_def.id,
                        name: case_def.name.symbol,
                        field: case_def
                            .field
                            .map(|field| ctxt.node(field).expect_case_field())
                            .map(|field| Field {
                                id: field.id,
                                name: Symbol::ZERO,
                            }),
                    })
                    .collect(),
            ),
        },
    }
}
pub fn build_global_context(
    config: Config,
    nodes: IndexVec<DefId, Node>,
    builtins: Builtins,
    parents: HashMap<DefId, DefId>,
) -> GlobalContext {
    let diag = DiagnosticReporter::new();
    GlobalContext {
        config,
        parents,
        lang_items: Default::default(),
        generics: Default::default(),
        idents: Default::default(),
        captures: Default::default(),
        nodes,
        diag,
        builtins,
        std_lib: Default::default(),
        ty_cache: Default::default(),
    }
}
