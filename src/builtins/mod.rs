use crate::Symbol;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum IntegerBuiltin {
    IntMaxValue,
    ShiftLeft,
    ShiftRight,
    WrappingAdd,
    OverflowingAdd,
    WrappingSub,
    OverflowingSub,
    WrappingMul,
    OverflowingMul,
    Truncate,
    Widen,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Builtin {
    // Reinterpretation
    Transmute,
    Bitcast,

    // Allocation
    ArrayNew,
    GcAlloc,

    //Pointers
    PtrWrite,
    Offset,
    PtrRead,
    PtrCopy,

    // Arrays
    ArraySetUnchecked,
    ArrayGetUnchecked,
    Len,
    ArrayAddr,

    // IO
    PrintString,
    EprintString,
    ReadLine,

    // Integers
    IntegerBuiltin(IntegerBuiltin),

    // Memory
    UninitZeroed,
    UninitAssumeInit,
    UninitNew,
}
impl Builtin {
    pub const fn name(self) -> &'static str {
        match self {
            Builtin::Transmute => "transmute",
            Builtin::Bitcast => "bitcast",
            Builtin::IntegerBuiltin(IntegerBuiltin::WrappingAdd) => "wrapping_add",
            Builtin::IntegerBuiltin(IntegerBuiltin::OverflowingAdd) => "overflowing_add",
            Builtin::IntegerBuiltin(IntegerBuiltin::Widen) => "widen",
            Builtin::IntegerBuiltin(IntegerBuiltin::Truncate) => "trunc",
            Builtin::IntegerBuiltin(IntegerBuiltin::OverflowingSub) => "overflowing_sub",
            Builtin::IntegerBuiltin(IntegerBuiltin::WrappingSub) => "wrapping_sub",
            Builtin::IntegerBuiltin(IntegerBuiltin::OverflowingMul) => "overflowing_mul",
            Builtin::IntegerBuiltin(IntegerBuiltin::WrappingMul) => "wrapping_mul",
            Builtin::IntegerBuiltin(IntegerBuiltin::IntMaxValue) => "int_max_value",
            Builtin::IntegerBuiltin(IntegerBuiltin::ShiftLeft) => "shift_left",
            Builtin::IntegerBuiltin(IntegerBuiltin::ShiftRight) => "shift_right",
            Builtin::ArrayAddr => "array_addr",
            Builtin::Len => "array_len",
            Builtin::ArrayNew => "array_new",
            Builtin::ArrayGetUnchecked => "array_get_unchecked",
            Builtin::ArraySetUnchecked => "array_set_unchecked",
            Builtin::PrintString => "print_string",
            Builtin::UninitAssumeInit => "uninit_assume_init",
            Builtin::UninitZeroed => "uninit_zeroed",
            Builtin::UninitNew => "uninit_new",
            Builtin::ReadLine => "read_line",
            Builtin::Offset => "offset",
            Builtin::PtrRead => "ptr_read",
            Builtin::PtrWrite => "ptr_write",
            Builtin::GcAlloc => "gc_alloc",
            Builtin::PtrCopy => "ptr_copy",
            Builtin::EprintString => "eprint_string",
        }
    }
    pub fn find(name: Symbol) -> Option<Builtin> {
        match name {
            Symbol::TRANSMUTE => Some(Builtin::Transmute),
            Symbol::ARRAY_LEN => Some(Builtin::Len),
            Symbol::ARRAY_ADDR => Some(Builtin::ArrayAddr),
            Symbol::WRAPPING_ADD => Some(Builtin::IntegerBuiltin(IntegerBuiltin::WrappingAdd)),
            Symbol::OVERFLOWING_ADD => {
                Some(Builtin::IntegerBuiltin(IntegerBuiltin::OverflowingAdd))
            }
            Symbol::SHIFT_LEFT => Some(Builtin::IntegerBuiltin(IntegerBuiltin::ShiftLeft)),
            Symbol::SHIFT_RIGHT => Some(Builtin::IntegerBuiltin(IntegerBuiltin::ShiftRight)),
            Symbol::WRAPPING_SUB => Some(Builtin::IntegerBuiltin(IntegerBuiltin::WrappingSub)),
            Symbol::OVERFLOWING_SUB => {
                Some(Builtin::IntegerBuiltin(IntegerBuiltin::OverflowingSub))
            }
            Symbol::WRAPPING_MUL => Some(Builtin::IntegerBuiltin(IntegerBuiltin::WrappingMul)),
            Symbol::OVERFLOWING_MUL => {
                Some(Builtin::IntegerBuiltin(IntegerBuiltin::OverflowingMul))
            }
            Symbol::WIDEN => Some(Builtin::IntegerBuiltin(IntegerBuiltin::Widen)),
            Symbol::TRUNCATE => Some(Builtin::IntegerBuiltin(IntegerBuiltin::Truncate)),
            Symbol::BITCAST => Some(Builtin::Bitcast),
            Symbol::INT_MAX_VALUE => Some(Builtin::IntegerBuiltin(IntegerBuiltin::IntMaxValue)),
            Symbol::ARRAY_NEW => Some(Builtin::ArrayNew),
            Symbol::ARRAY_SET_UNCHECKED => Some(Builtin::ArraySetUnchecked),
            Symbol::ARRAY_GET_UNCHECKED => Some(Builtin::ArrayGetUnchecked),
            Symbol::PRINT_STRING => Some(Builtin::PrintString),
            Symbol::UNINIT_ZEROED => Some(Builtin::UninitZeroed),
            Symbol::UNINIT_ASSUME_INIT => Some(Builtin::UninitAssumeInit),
            Symbol::UNINIT_NEW => Some(Builtin::UninitNew),
            Symbol::READ_LINE => Some(Builtin::ReadLine),
            Symbol::PTR_READ => Some(Builtin::PtrRead),
            Symbol::PTR_WRITE => Some(Builtin::PtrWrite),
            Symbol::OFFSET => Some(Builtin::Offset),
            Symbol::GC_ALLOC => Some(Builtin::GcAlloc),
            Symbol::PTR_COPY => Some(Builtin::PtrCopy),
            Symbol::EPRINT_STRING => Some(Builtin::EprintString),
            _ => None,
        }
    }
}
