use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fmt::Debug,
};

use crate::{
    Symbol,
    diagnostics::DiagnosticReporter,
    ident::Ident,
    resolved_ast::{
        self, Builtins, DefId, Item, ItemId, ItemKind, TypeDef, TypeDefKind, VariantDef,
    },
    scheme::Scheme,
    src_loc::SrcLoc,
    typecheck::infer::TypeInfer,
    typed_ast::FieldId,
    types::{FunctionSig, GenericArg, GenericKind, GenericParam, Region, Type, lower::Lower},
};
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
    pub const fn kind(&self, index: usize) -> GenericKind {
        self.params.as_slice()[index].kind
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
                GenericKind::Region => GenericArg::Region(Region::Param(param.name.symbol, i)),
                GenericKind::Type => GenericArg::Type(Type::Param(param.name.symbol, i)),
            })
            .collect()
    }
}
#[derive(Clone, Copy, Debug)]
enum NodePath {
    Item(ItemId),
    Case {
        parent: ItemId,
        index: usize,
    },
    CaseField {
        parent: ItemId,
        case: DefId,
        index: usize,
    },
    Field {
        parent: ItemId,
        index: FieldId,
    },
}
pub struct GlobalContext {
    diag: DiagnosticReporter,
    idents: RefCell<HashMap<DefId, Ident>>,
    generics: RefCell<HashMap<DefId, Generics>>,
    parents: HashMap<DefId, DefId>,
    item_indexes: HashMap<ItemId, usize>,
    items: Vec<Item>,
    nodes: HashMap<DefId, NodePath>,
    builtins: Builtins,
    std_lib: std::cell::Cell<Option<DefId>>,
}
impl GlobalContext {
    pub fn as_ref(&self) -> CtxtRef<'_> {
        CtxtRef(self)
    }
}
#[derive(Copy, Clone)]
pub struct CtxtRef<'a>(&'a GlobalContext);

impl CtxtRef<'_> {
    fn node_path_for(self, id: DefId) -> NodePath {
        self.0.nodes[&id]
    }
    #[track_caller]
    fn expect_item_id(self, id: DefId) -> ItemId {
        let NodePath::Item(item) = self.node_path_for(id) else {
            unreachable!("not an item")
        };
        item
    }
    #[track_caller]
    pub fn expect_case(&self, case_id: DefId) -> (&VariantDef, usize) {
        let NodePath::Case { parent, index } = self.node_path_for(case_id) else {
            unreachable!("expected a variant def")
        };
        let variant_def = self.expect_variant_def(parent);
        (variant_def, index)
    }
    #[track_caller]
    pub fn expect_item(&self, id: DefId) -> &Item {
        let item = self.expect_item_id(id);
        self.item(item)
    }
    #[track_caller]
    pub fn expect_type(&self, id: DefId) -> &TypeDef {
        let item = self.expect_item_id(id);
        self.expect_type_def(item)
    }
    #[track_caller]
    fn expect_type_def(&self, id: ItemId) -> &TypeDef {
        let ItemKind::TypeDef(type_def) = &self.item(id).kind else {
            unreachable!("expected a type def")
        };
        type_def
    }
    #[track_caller]
    fn expect_variant_def(&self, id: ItemId) -> &VariantDef {
        self.expect_type_def(id).expect_variant()
    }
    pub fn is_type_recursive(self, id: DefId) -> bool {
        fn is_ty_recursive(
            ctxt: CtxtRef<'_>,
            ty: &Type,
            mut seen_ids: &mut HashSet<DefId>,
        ) -> bool {
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
                | Type::Box(_)
                | Type::List(_)
                | Type::RawPointer(_)
                | Type::Int
                | Type::Imm(..)
                | Type::Mut(..)
                | Type::Function(..) => false,
                Type::Record(fields) => fields
                    .iter()
                    .any(|field| is_ty_recursive(ctxt, &field.ty, seen_ids)),
                Type::Named(id, _, args) => {
                    if !seen_ids.insert(*id) {
                        return true;
                    }

                    match &ctxt.expect_type(*id).kind {
                        resolved_ast::TypeDefKind::Record(record) => {
                            for field in record.fields.iter() {
                                if is_ty_recursive(
                                    ctxt,
                                    &ctxt.type_of(field.id).bind(args),
                                    &mut seen_ids,
                                ) {
                                    return true;
                                }
                            }
                        }
                        resolved_ast::TypeDefKind::Variant(variant) => {
                            for case in variant.cases.iter() {
                                let Some(id) = case.ty.as_ref().map(|ty| ty.id) else {
                                    continue;
                                };
                                if is_ty_recursive(
                                    ctxt,
                                    &ctxt.type_of(id).bind(args),
                                    &mut seen_ids,
                                ) {
                                    return true;
                                }
                            }
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
                self.name(id).symbol,
                self.generics(id).instantiate_identity(),
            ),
            &mut HashSet::new(),
        )
    }

    pub fn span(self, id: DefId) -> SrcLoc {
        self.name(id).loc
    }
    #[track_caller]
    pub fn name(self, id: DefId) -> Ident {
        if let Some(&ident) = self.0.idents.borrow().get(&id) {
            return ident;
        }
        let ident = match self.node_path_for(id) {
            NodePath::Item(item) => match &self.item(item).kind {
                ItemKind::Function(function) => function.name,
                ItemKind::Module(module) => module.name,
                ItemKind::TypeDef(type_def) => type_def.name,
            },
            NodePath::Case { parent, index } => {
                let case = &self.expect_variant_def(parent).cases[index];
                case.name
            }
            NodePath::CaseField { index, case, .. } => Ident {
                symbol: Symbol::ZERO,
                loc: self.expect_case(case).0.cases[index].name.loc,
            },
            NodePath::Field { parent, index } => {
                self.expect_type_def(parent).expect_record().fields[index].name
            }
        };
        self.0.idents.borrow_mut().insert(id, ident);
        ident
    }
    pub fn type_of(self, id: DefId) -> Scheme<Type> {
        let ty = match self.node_path_for(id) {
            NodePath::Case { parent, index } => {
                let parent_id = parent.into_def_id();
                let type_def = self.expect_type_def(parent);
                let variant_def = type_def.expect_variant();
                let case = &variant_def.cases[index];
                let name = type_def.name;
                let variant_ty = Type::Named(
                    parent_id,
                    name.symbol,
                    self.generics(parent_id).instantiate_identity(),
                );
                if let Some(inner_ty) = case
                    .ty
                    .as_ref()
                    .map(|case| Lower::new(self, parent_id, None).lower_type(&case.ty))
                {
                    Type::new_function(vec![inner_ty], variant_ty)
                } else {
                    variant_ty
                }
            }
            NodePath::Item(item) => match &self.item(item).kind {
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
                ItemKind::Module(module) => {
                    unreachable!("cannot get type of module {}", module.name.symbol)
                }
            },
            NodePath::CaseField {
                parent,
                case,
                index,
            } => {
                let case = &self.expect_case(case).0.cases[index];
                Lower::new(self, parent.into_def_id(), None)
                    .lower_type(&case.ty.as_ref().expect("should have a type").ty)
            }
            NodePath::Field { parent, index } => Lower::new(self, parent.into_def_id(), None)
                .lower_type(&self.expect_type_def(parent).expect_record().fields[index].ty),
        };
        Scheme::new(ty)
    }
    pub fn generics(self, id: DefId) -> Generics {
        if let Some(generics) = self.0.generics.borrow().get(&id) {
            return generics.clone();
        }
        let generics = match self.node_path_for(id) {
            NodePath::Item(item) => match &self.item(item).kind {
                ItemKind::Function(function_def) => function_def
                    .generics
                    .as_ref()
                    .map_or_else(Generics::new, lower_generics),
                ItemKind::Module(_) => Generics::new(),
                ItemKind::TypeDef(type_def) => type_def
                    .generics
                    .as_ref()
                    .map_or_else(Generics::new, lower_generics),
            },
            NodePath::Case { parent, .. }
            | NodePath::CaseField { parent, .. }
            | NodePath::Field { parent, .. } => self.generics(parent.into_def_id()),
        };
        self.0.generics.borrow_mut().insert(id, generics.clone());
        generics
    }
    fn item(&self, id: ItemId) -> &Item {
        &self.0.items[self.0.item_indexes[&id]]
    }
    fn get_item(&self, id: DefId) -> Option<&Item> {
        let NodePath::Item(item) = self.node_path_for(id) else {
            return None;
        };
        Some(self.item(item))
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
    pub fn parent_of(self, id: DefId) -> Option<DefId> {
        self.0.parents.get(&id).copied()
    }
    pub fn all_items(&self) -> impl Iterator<Item = &Item> {
        self.0.items.iter()
    }
    pub fn top_level_items(&self) -> impl Iterator<Item = &Item> {
        self.all_items()
            .filter(|item| self.parent_of(item.id.0).is_none())
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
                let function = self.function_def(item_id)?;
                if function.name.symbol != Symbol::MAIN {
                    return None;
                }
                Some((item_id, function))
            })
    }
    pub fn function_def(&self, id: DefId) -> Option<&resolved_ast::Function> {
        match self.get_item(id)?.kind {
            ItemKind::Function(ref function) => Some(function),
            _ => None,
        }
    }
    #[track_caller]
    pub fn signature_of(self, id: DefId) -> Scheme<FunctionSig> {
        let function = self.function_def(id).expect("should be a function def");
        let lower = Lower::new(self, id, None);
        Scheme::new(FunctionSig::new(
            lower
                .lower_types(&mut function.params.iter().map(|param| &param.ty))
                .collect(),
            lower.lower_type(&function.return_type),
        ))
    }
    pub fn display(&self, id: DefId) -> impl std::fmt::Display {
        self.name(id).symbol
    }
    pub fn diag(&self) -> &DiagnosticReporter {
        &self.0.diag
    }
    pub fn builtins(&self) -> &Builtins {
        &self.0.builtins
    }
    pub fn std_lib_module(self) -> Option<DefId> {
        if let Some(std_lib) = self.0.std_lib.get() {
            return Some(std_lib);
        }
        let std_lib = self.top_level_items().find_map(|item| {
            let ItemKind::Module(ref module) = item.kind else {
                return None;
            };
            if module.name.symbol != Symbol::STD {
                return None;
            }
            Some(item.id.into_def_id())
        });
        self.0.std_lib.set(std_lib);
        std_lib
    }
}
fn lower_generics(generics: &resolved_ast::Generics) -> Generics {
    Generics {
        params: generics
            .kinds
            .iter()
            .zip(generics.names.iter().copied())
            .map(|(kind, name)| GenericParam {
                name,
                kind: match kind {
                    resolved_ast::GenericKind::Region => GenericKind::Region,
                    resolved_ast::GenericKind::Type => GenericKind::Type,
                },
            })
            .collect(),
    }
}
pub fn build_global_context(
    items: Vec<Item>,
    builtins: Builtins,
    parents: HashMap<DefId, DefId>,
) -> GlobalContext {
    let mut item_indexes = HashMap::new();
    let mut nodes = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        let id = item.id.into_def_id();
        nodes.insert(id, NodePath::Item(item.id));
        match &item.kind {
            ItemKind::Function(_) => {}
            ItemKind::TypeDef(type_def) => match &type_def.kind {
                TypeDefKind::Variant(variant_def) => {
                    nodes.extend(variant_def.cases.iter().enumerate().flat_map(|(i, case)| {
                        std::iter::once({
                            (
                                case.id,
                                NodePath::Case {
                                    parent: item.id,
                                    index: i,
                                },
                            )
                        })
                        .chain(case.ty.as_ref().map(|ty| {
                            (
                                ty.id,
                                NodePath::CaseField {
                                    case: case.id,
                                    parent: item.id,
                                    index: i,
                                },
                            )
                        }))
                    }));
                }
                TypeDefKind::Record(record) => {
                    nodes.extend(record.fields.iter_enumerated().map(|(i, field)| {
                        (
                            field.id,
                            NodePath::Field {
                                parent: item.id,
                                index: i,
                            },
                        )
                    }));
                }
            },
            ItemKind::Module(_) => {}
        }
        item_indexes.insert(item.id, i);
    }
    let diag = DiagnosticReporter::new();
    GlobalContext {
        parents,
        generics: Default::default(),
        idents: Default::default(),
        nodes,
        diag,
        items,
        item_indexes,
        builtins,
        std_lib: Default::default(),
    }
}
