use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    sync::{LazyLock, Mutex},
};

use crate::{index_vec::IndexVec, src_loc::SrcLoc};
///A source code identifier
#[derive(Debug, Clone, Copy)]
pub struct Ident {
    pub symbol: Symbol,
    pub loc: SrcLoc,
}
impl Ident {
    /// Constructs an ident with `symbol` and `loc`
    pub fn new(symbol: Symbol, loc: SrcLoc) -> Self {
        Self { symbol, loc }
    }
}
pub type SymbolContent = String;
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol(SymbolId);
impl Symbol {
    const NAMED_SYMBOLS: &[&str] = &[
        "",
        "main",
        "std",
        "builtins",
        "0",
        "copy",
        "unsafe",
        "lang_item",
        "box",
        "opaque",
        "builtin",
        "array",

// builtins
        "box_alloc",
        "transmute",
        "array_len",
        "array_addr",
        "wrapping_add",
        "overflowing_add",
        "wrapping_sub",
        "overflowing_sub",
        "zero_extend",
        "array_repeat",
        "raw_array_alloc",
        "array_set_unchecked",
        "array_get_unchecked",
        "print_string",
        "int_max_value"
        
    ];
    const _NO_REPEATS : () = {
        let mut i = 0;
        while i < Self::NAMED_SYMBOLS.len(){
            let mut j = 0;
            while j < Self::NAMED_SYMBOLS.len(){
                if i != j{
                    assert!(!Self::NAMED_SYMBOLS[i].eq_ignore_ascii_case(Self::NAMED_SYMBOLS[j]),"repeat");
                }
                j += 1;
            }
            i += 1;
        }

    };
    
    pub fn index(self) -> usize{
        self.0.into_usize()
    }
    const fn expect_symbol(name: &str) -> Symbol {
        let mut i = 0;
        while i < Self::NAMED_SYMBOLS.len() {
               if Self::NAMED_SYMBOLS[i].eq_ignore_ascii_case(name)
             {
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
    pub const EMPTY_STRING: Self = Self::expect_symbol("");
    pub const MAIN: Self = Self::expect_symbol("main");
    pub const STD: Self = Self::expect_symbol("std");
    pub const BUILTINS: Self = Self::expect_symbol("builtins");
    pub const ZERO: Self = Self::expect_symbol("0");
    pub const COPY: Self = Self::expect_symbol("copy");
    pub const UNSAFE: Self = Self::expect_symbol("unsafe");
    pub const LANG_ITEM: Self = Self::expect_symbol("lang_item");
    pub const BOX: Self = Self::expect_symbol("box");
    pub const OPAQUE: Self = Self::expect_symbol("opaque");
    pub const BUILTIN: Self = Self::expect_symbol("builtin");
    pub const ARRAY: Self = Self::expect_symbol("array");
    pub const TRANSMUTE : Self = Self::expect_symbol("transmute");
    pub const ARRAY_LEN : Self = Self::expect_symbol("array_len");
    pub const BOX_ALLOC : Self = Self::expect_symbol("box_alloc");
    pub const ARRAY_ADDR : Self = Self::expect_symbol("array_addr");
    pub const WRAPPING_ADD : Self = Self::expect_symbol("wrapping_add");
    pub const OVERFLOWING_ADD : Self = Self::expect_symbol("overflowing_add");
    pub const WRAPPING_SUB : Self = Self::expect_symbol("wrapping_sub");
    pub const OVERFLOWING_SUB : Self = Self::expect_symbol("overflowing_sub");
    pub const ZERO_EXTEND : Self = Self::expect_symbol("zero_extend");
    pub const ARRAY_REPEAT : Self = Self::expect_symbol("array_repeat");
    pub const RAW_ARRAY_ALLOC : Self = Self::expect_symbol("raw_array_alloc");
    pub const ARRAY_SET_UNCHECKED : Self = Self::expect_symbol("array_set_unchecked");
    pub const ARRAY_GET_UNCHECKED : Self = Self::expect_symbol("array_get_unchecked");
    pub const PRINT_STRING : Self = Self::expect_symbol("print_string");
    pub const INT_MAX_VALUE : Self = Self::expect_symbol("int_max_value");
    pub fn intern(txt: &str) -> Self {
        INTERNER.lock().unwrap().intern(txt)
    }
    pub fn with_str<T>(&self, f : impl FnOnce(&str) -> T) -> T {
        let lock = INTERNER.lock().unwrap();
        let value = f(lock.resolve(*self));
        drop(lock);
        value
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
        for &symbol in Symbol::NAMED_SYMBOLS {
            intern.intern(symbol);
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
