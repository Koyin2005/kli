use std::collections::HashMap;

use crate::{Symbol, def_ids::DefId};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Builtin {
    Transmute,
    Len,
    ArrayAddr,
    WrappingAdd,
    OverflowingAdd,
}
impl Builtin {
    const _NO_REPEATS: () = {
        let mut i = 0;
        while i < Self::ALL_BUILTINS.len() {
            let mut j = 0;
            while j < Self::ALL_BUILTINS.len() {
                if i == j {
                    continue;
                }
                if Self::ALL_BUILTINS[i]
                    .name()
                    .eq_ignore_ascii_case(Self::ALL_BUILTINS[j].name())
                {
                    panic!("repeated const")
                }
                j += 1;
            }
            i += 1;
        }
    };
    pub const COUNT: usize = 5;
    pub const ALL_BUILTINS: [Self; Self::COUNT] = [
        Builtin::Transmute,
        Builtin::Len,
        Builtin::ArrayAddr,
        Builtin::WrappingAdd,
        Builtin::OverflowingAdd,
    ];
    pub const fn name(self) -> &'static str {
        match self {
            Builtin::Transmute => "transmute",
            Builtin::WrappingAdd => "wrapping_add",
            Builtin::OverflowingAdd => "overflowing_add",
            Builtin::ArrayAddr => "array_addr",
            Builtin::Len => "array_len",
        }
    }
    pub fn find(name: Symbol) -> Option<Builtin> {
        Self::ALL_BUILTINS
            .into_iter()
            .find(|builtin| Symbol::intern(builtin.name()) == name)
    }
    const fn index_of(self) -> usize {
        let mut i = 0;
        let builtins = Self::ALL_BUILTINS;
        let name = self.name();
        while i < builtins.len() {
            if name.eq_ignore_ascii_case(builtins[i].name()) {
                return i;
            }
            i += 1;
        }
        panic!("missing builtin")
    }
}
#[derive(Default, Clone)]
pub struct Builtins([Option<DefId>; Builtin::COUNT], HashMap<DefId, Builtin>);
impl Builtins {
    pub fn insert(&mut self, builtin: Builtin, id: DefId) {
        let _ = self.0[builtin.index_of()].insert(id);
        self.1.insert(id, builtin);
    }
    pub fn expect_id(&self, builtin: Builtin) -> DefId {
        self.0[builtin.index_of()]
            .unwrap_or_else(|| panic!("expected builtin '{}' to be defined", builtin.name()))
    }
    pub fn builtin_for(&self, id: DefId) -> Option<Builtin> {
        self.1.get(&id).copied()
    }
}
