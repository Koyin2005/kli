use crate::Symbol;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Builtin {
    BoxAlloc,
    Transmute,
    Len,
    ArrayAddr,
    WrappingAdd,
    OverflowingAdd,
    WrappingSub,
    OverflowingSub,
    ZeroExtend,
    ArrayRepeat,
    RawArrayAlloc,
    ArraySetUnchecked,
    ArrayGetUnchecked,
    PrintString,
    IntMaxValue,
}
impl Builtin {
    pub const fn name(self) -> &'static str {
        match self {
            Builtin::Transmute => "transmute",
            Builtin::WrappingAdd => "wrapping_add",
            Builtin::OverflowingAdd => "overflowing_add",
            Builtin::ArrayAddr => "array_addr",
            Builtin::Len => "array_len",
            Builtin::BoxAlloc => "box_alloc",
            Builtin::ZeroExtend => "zero_extend",
            Builtin::ArrayRepeat => "array_repeat",
            Builtin::RawArrayAlloc => "raw_array_alloc",
            Builtin::ArrayGetUnchecked => "array_get_unchecked",
            Builtin::ArraySetUnchecked => "array_set_unchecked",
            Builtin::PrintString => "print_string",
            Builtin::OverflowingSub => "overflowing_sub",
            Builtin::WrappingSub => "wrapping_sub",
            Builtin::IntMaxValue => "int_max_value",
        }
    }
    pub fn find(name: Symbol) -> Option<Builtin> {
        match name {
            Symbol::BOX_ALLOC => Some(Builtin::BoxAlloc),
            Symbol::TRANSMUTE => Some(Builtin::Transmute),
            Symbol::ARRAY_LEN => Some(Builtin::Len),
            Symbol::ARRAY_ADDR => Some(Builtin::ArrayAddr),
            Symbol::WRAPPING_ADD => Some(Builtin::WrappingAdd),
            Symbol::OVERFLOWING_ADD => Some(Builtin::OverflowingAdd),
            Symbol::WRAPPING_SUB => Some(Builtin::WrappingSub),
            Symbol::OVERFLOWING_SUB => Some(Builtin::OverflowingSub),
            Symbol::ZERO_EXTEND => Some(Builtin::ZeroExtend),
            Symbol::ARRAY_REPEAT => Some(Builtin::ArrayRepeat),
            Symbol::RAW_ARRAY_ALLOC => Some(Builtin::RawArrayAlloc),
            Symbol::ARRAY_SET_UNCHECKED => Some(Builtin::ArraySetUnchecked),
            Symbol::ARRAY_GET_UNCHECKED => Some(Builtin::ArrayGetUnchecked),
            Symbol::PRINT_STRING => Some(Builtin::PrintString),
            Symbol::INT_MAX_VALUE => Some(Builtin::IntMaxValue),
            _ => None,
        }
    }
}
