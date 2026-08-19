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

    // Allocation
    BoxAlloc,
    RawArrayAlloc,
    GcAlloc,

    //Pointers
    Offset,
    PtrCopy,

    // Arrays
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
}
impl Builtin {
    pub const fn name(self) -> &'static str {
        match self {
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
            Builtin::BoxAlloc => "box_alloc",
            Builtin::RawArrayAlloc => "raw_array_alloc",
            Builtin::PrintString => "print_string",
            Builtin::UninitZeroed => "uninit_zeroed",
            Builtin::ReadLine => "read_line",
            Builtin::Offset => "offset",
            Builtin::GcAlloc => "gc_alloc",
            Builtin::PtrCopy => "ptr_copy",
            Builtin::EprintString => "eprint_string",
        }
    }
    pub fn find(name: Symbol) -> Option<Builtin> {
        match name {
            Symbol::BOX_ALLOC => Some(Builtin::BoxAlloc),
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
            Symbol::INT_MAX_VALUE => Some(Builtin::IntegerBuiltin(IntegerBuiltin::IntMaxValue)),
            Symbol::RAW_ARRAY_ALLOC => Some(Builtin::RawArrayAlloc),
            Symbol::PRINT_STRING => Some(Builtin::PrintString),
            Symbol::UNINIT_ZEROED => Some(Builtin::UninitZeroed),
            Symbol::READ_LINE => Some(Builtin::ReadLine),
            Symbol::OFFSET => Some(Builtin::Offset),
            Symbol::GC_ALLOC => Some(Builtin::GcAlloc),
            Symbol::PTR_COPY => Some(Builtin::PtrCopy),
            Symbol::EPRINT_STRING => Some(Builtin::EprintString),
            _ => None,
        }
    }
}
