use std::collections::HashMap;

use crate::{
    diagnostics::DiagnosticReporter,
    ident::Ident,
    resolved_ast::{self, DefId, Item, ItemId, ItemKind},
    scheme::Scheme,
    src_loc::SrcLoc,
    typecheck::infer::TypeInfer,
    types::{FunctionSig, GenericArg, GenericKind, Region, Type, lower::Lower},
};

pub struct Generics {
    kinds: Vec<GenericKind>,
}
impl Generics {
    const fn new() -> Self {
        Self { kinds: Vec::new() }
    }
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }
    pub fn kind(&self, index: usize) -> GenericKind {
        self.kinds[index]
    }
    pub fn count(&self) -> usize {
        self.kinds.len()
    }
    pub fn instantiate(&self, infer: &mut TypeInfer, loc: SrcLoc) -> Vec<GenericArg> {
        self.kinds
            .iter()
            .copied()
            .map(|kind| match kind {
                GenericKind::Region => {
                    GenericArg::Region(Region::Infer(infer.fresh_region(loc.clone())))
                }
                GenericKind::Type => GenericArg::Type(Type::Infer(infer.fresh_ty(loc.clone()))),
            })
            .collect()
    }
    pub fn instantiate_unknown(&self) -> Vec<GenericArg> {
        self.kinds
            .iter()
            .map(|kind| match kind {
                GenericKind::Region => GenericArg::Region(Region::Unknown),
                GenericKind::Type => GenericArg::Type(Type::Unknown),
            })
            .collect()
    }
}

pub struct GlobalContext {
    names: HashMap<DefId, Ident>,
    generics: HashMap<DefId, Generics>,
    diag: DiagnosticReporter,
    item_indexes: HashMap<DefId, usize>,
    items: Vec<Item>,
    parents: HashMap<DefId, DefId>,
}
impl GlobalContext {
    pub fn as_ref(&self) -> CtxtRef<'_> {
        CtxtRef(&self)
    }
}
#[derive(Copy, Clone)]
pub struct CtxtRef<'a>(&'a GlobalContext);

impl CtxtRef<'_> {
    pub fn name(&self, id: DefId) -> &Ident {
        &self.0.names[&id]
    }
    pub fn generics(&self, id: DefId) -> &Generics {
        static GENERICS_DEFAULT: Generics = Generics::new();
        self.0.generics.get(&id).unwrap_or(&GENERICS_DEFAULT)
    }
    pub fn item(&self, id: DefId) -> &Item {
        &self.0.items[self.0.item_indexes[&id]]
    }
    pub fn parent_of(self, id: DefId) -> Option<DefId> {
        self.0.parents.get(&id).copied()
    }
    pub fn root_ids(&self) -> impl Iterator<Item = DefId> {
        self.0
            .items
            .iter()
            .map(|&Item { id: ItemId(id), .. }| id)
            .filter(|&id| self.parent_of(id).is_none())
    }
    pub fn main_id(self) -> Option<DefId> {
        self.root_ids()
            .find(|&id| self.name(id).content.as_ref() == "main")
    }
    pub fn all_ids(self) -> Vec<DefId> {
        self.0
            .items
            .iter()
            .map(|&Item { id: ItemId(id), .. }| id)
            .collect()
    }
    pub fn function_def(&self, id: DefId) -> Option<&resolved_ast::Function> {
        match self.item(id).kind {
            ItemKind::Function(ref function) => Some(function),
            _ => None,
        }
    }
    pub fn signature_of(self, id: DefId) -> Scheme<FunctionSig> {
        let Item {
            id: _,
            loc: _,
            kind: ItemKind::Function(function),
        } = self.item(id)
        else {
            unreachable!("expected a function item")
        };
        let lower = Lower::new(self, id);
        Scheme::new(FunctionSig::new(
            lower
                .lower_types(&mut function.params.iter().map(|param| &param.ty))
                .collect(),
            lower.lower_type(&function.return_type),
        ))
    }
    pub fn display(&self, id: DefId) -> impl std::fmt::Display {
        &self.name(id).content
    }
    pub fn diag(&self) -> &DiagnosticReporter {
        &self.0.diag
    }
}
fn lower_generics(generics: &resolved_ast::Generics) -> Generics {
    Generics {
        kinds: generics
            .kinds
            .iter()
            .map(|kind| match kind {
                resolved_ast::GenericKind::Region => GenericKind::Region,
                resolved_ast::GenericKind::Type => GenericKind::Type,
            })
            .collect(),
    }
}
pub fn build_global_context(program: resolved_ast::Program) -> GlobalContext {
    let mut names = HashMap::new();
    let mut generics = HashMap::new();
    let mut item_indexes = HashMap::new();
    let mut parents = HashMap::new();
    let items = program.items;
    for (i, item) in items.iter().enumerate() {
        let ItemId(id) = item.id;
        match &item.kind {
            ItemKind::Function(function) => {
                names.insert(id, function.name.clone());
                generics.insert(
                    id,
                    function
                        .generics
                        .as_ref()
                        .map(|generics| lower_generics(generics))
                        .unwrap_or(Generics::new()),
                );
            }
            ItemKind::TypeDef(type_def) => {
                names.insert(id, type_def.name.clone());
                generics.insert(
                    id,
                    type_def
                        .generics
                        .as_ref()
                        .map(|generics| lower_generics(generics))
                        .unwrap_or(Generics::new()),
                );
            }
            ItemKind::Module(module) => {
                names.insert(id, module.name.clone());
                for &item_id in module.items.iter() {
                    parents.insert(item_id, id);
                }
            }
        }
        item_indexes.insert(id, i);
    }
    let diag = DiagnosticReporter::new();
    GlobalContext {
        parents,
        names,
        generics,
        diag,
        items,
        item_indexes,
    }
}
