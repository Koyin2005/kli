use std::collections::HashMap;

use crate::resolved_ast::DefId;
#[derive(Clone, Copy,PartialEq, Eq)]
pub enum LangItem {
    Box
}
impl LangItem{
    pub const fn name(&self) -> &'static str {
        match self{
            Self::Box => "box"
        }
    }
    pub const COUNT : usize = 1;
    pub const ALL_LANG_ITEMS : [LangItem;Self::COUNT] = [LangItem::Box];
}
pub struct LangItems([Option<DefId>;LangItem::COUNT],HashMap<DefId,LangItem>);

impl LangItems{
    pub fn declare(&mut self, item : LangItem, id : DefId){
        self.0[item as usize] = Some(id);
    }
    pub fn get(&self, item : LangItem) -> Option<DefId>{
        self.0[item as usize]
    }
    pub fn expect(&self, item : LangItem) -> DefId{
        let Some(id) = self.get(item) else {
            panic!("Expected lang item '{}' ",item.name())  
        };
        id
    }
}