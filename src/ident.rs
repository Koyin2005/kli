use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    sync::{LazyLock, Mutex},
};

use crate::{index_vec::IndexVec, src_loc::SrcLoc};

#[derive(Debug, Clone, Copy)]
pub struct Ident {
    pub symbol: Symbol,
    pub loc: SrcLoc,
}
pub type SymbolContent = String;
#[derive(Clone, Copy)]
enum NamedSymbol {
    Empty,
    Main,
    Std,
    Builtins,
    NumberZero,
    Copy,
    Unsafe,
    LangItem,
    Box,
    Opaque,
}
impl NamedSymbol {
    pub const fn content(self) -> &'static str {
        match self {
            Self::Empty => "",
            Self::Main => "main",
            Self::Std => "std",
            Self::Builtins => "builtins",
            Self::NumberZero => "0",
            Self::Copy => "copy",
            Self::Unsafe => "unsafe",
            Self::LangItem => "lang_item",
            Self::Box => "box",
            Self::Opaque => "opaque",
        }
    }
}
const fn byte_eq(b1: &[u8], b2: &[u8]) -> bool {
    if b1.len() != b2.len() {
        return false;
    }
    let mut i = 0;
    while i < b1.len() {
        if b1[i] != b2[i] {
            return false;
        }
        i += 1;
    }
    true
}
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(SymbolId);
impl Symbol {
    const NAMED_SYMBOLS: [NamedSymbol; 10] = [
        NamedSymbol::Empty,
        NamedSymbol::Main,
        NamedSymbol::Std,
        NamedSymbol::Builtins,
        NamedSymbol::NumberZero,
        NamedSymbol::Copy,
        NamedSymbol::Unsafe,
        NamedSymbol::LangItem,
        NamedSymbol::Box,
        NamedSymbol::Opaque,
    ];
    const fn expect_symbol(name: NamedSymbol) -> Symbol {
        let content = name.content();
        let mut i = 0;
        while i < Self::NAMED_SYMBOLS.len() {
            if byte_eq(
                Self::NAMED_SYMBOLS[i].content().as_bytes(),
                content.as_bytes(),
            ) {
                return if i > u32::MAX as usize {
                    panic!("too many symbols")
                } else {
                    Symbol(hidden::make_symbol(i as u32))
                };
            }
            i += 1;
        }
        panic!("not found")
    }
    pub const EMPTY_STRING: Self = Self::expect_symbol(NamedSymbol::Empty);
    pub const MAIN: Self = Self::expect_symbol(NamedSymbol::Main);
    pub const STD: Self = Self::expect_symbol(NamedSymbol::Std);
    pub const BUILTINS: Self = Self::expect_symbol(NamedSymbol::Builtins);
    pub const ZERO: Self = Self::expect_symbol(NamedSymbol::NumberZero);
    pub const COPY: Self = Self::expect_symbol(NamedSymbol::Copy);
    pub const UNSAFE: Self = Self::expect_symbol(NamedSymbol::Unsafe);
    pub const LANG_ITEM: Self = Self::expect_symbol(NamedSymbol::LangItem);
    pub const BOX: Self = Self::expect_symbol(NamedSymbol::Box);
    pub const OPAQUE: Self = Self::expect_symbol(NamedSymbol::Opaque);
    pub fn intern(txt: &str) -> Self {
        INTERNER.lock().unwrap().intern(txt)
    }
}
type SymbolId = hidden::SymbolId;

mod hidden {
    use crate::define_id;

    define_id!(SymbolId);
    pub const fn make_symbol(index: u32) -> SymbolId {
        SymbolId(index)
    }
}

#[derive(Default)]
struct SymbolInternerInner {
    names: IndexVec<SymbolId, SymbolContent>,
    seen_names: HashMap<SymbolContent, SymbolId>,
}
impl SymbolInternerInner {
    fn intern(&mut self, name: &str) -> Symbol {
        if let Some(&name) = self.seen_names.get(name) {
            return Symbol(name);
        }

        let name: SymbolContent = name.into();
        let id = self.names.push(name.clone());
        self.seen_names.insert(name, id);
        Symbol(id)
    }
    fn resolve(&self, id: Symbol) -> &str {
        &self.names[id.0]
    }
}
impl Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let interner = INTERNER.lock().unwrap();
        f.pad(interner.resolve(*self))
    }
}
impl Debug for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let interner = INTERNER.lock().unwrap();
        f.pad(interner.resolve(*self))
    }
}
static INTERNER: LazyLock<Mutex<SymbolInterner>> =
    LazyLock::new(|| Mutex::new(SymbolInterner::new()));
#[derive(Default)]
struct SymbolInterner(SymbolInternerInner);

impl SymbolInterner {
    pub fn new() -> Self {
        let mut intern = Self::default();
        for symbol in Symbol::NAMED_SYMBOLS {
            intern.intern(symbol.content());
        }
        intern
    }
    pub fn intern(&mut self, name: &str) -> Symbol {
        self.0.intern(name)
    }

    pub fn resolve(&self, id: Symbol) -> &str {
        self.0.resolve(id)
    }
}
