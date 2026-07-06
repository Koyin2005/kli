use crate::{
    CtxtRef,
    resolved_ast::{AnnotationKind, DefId},
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LangItem {
    Box,
}
impl LangItem {
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Box => "box",
        }
    }
    pub fn with_name(name: &str) -> Option<Self> {
        Self::ALL_LANG_ITEMS
            .into_iter()
            .find(|&item| item.name() == name)
    }
    pub const COUNT: usize = 1;
    pub const ALL_LANG_ITEMS: [LangItem; Self::COUNT] = [LangItem::Box];
}
#[derive(Clone, Copy)]
pub struct LangItems([Option<DefId>; LangItem::COUNT]);

impl LangItems {
    pub fn declare(&mut self, item: LangItem, id: DefId) {
        self.0[item as usize] = Some(id);
    }
    pub fn get(&self, item: LangItem) -> Option<DefId> {
        self.0[item as usize]
    }
    pub fn expect(&self, item: LangItem) -> DefId {
        let Some(id) = self.get(item) else {
            panic!("Expected lang item '{}' ", item.name())
        };
        id
    }
    pub fn collect(ctxt: CtxtRef<'_>) -> LangItems {
        let mut lang_items = LangItems([None; LangItem::COUNT]);
        for item in ctxt.all_items() {
            let id = item.id;
            for annotation in item.annotations.iter() {
                if let AnnotationKind::LangItem(item) = annotation.kind {
                    lang_items.declare(item, id);
                }
            }
        }
        lang_items
    }
}
