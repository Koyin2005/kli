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
    GcAlloc,

    //Pointers
    PtrWrite,
    Offset,
    PtrRead,
    PtrCopy,
    WriteZeroes,

    // Arrays
    ArrayNew,
    Len,
    ArrayPtr,

    // IO
    PrintString,
    EprintString,
    ReadLine,

    // Integers
    IntegerBuiltin(IntegerBuiltin),
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
            Builtin::ArrayPtr => "array_ptr",
            Builtin::Len => "array_len",
            Builtin::ArrayNew => "array_new",
            Builtin::PrintString => "print_string",
            Builtin::ReadLine => "read_line",
            Builtin::Offset => "offset",
            Builtin::PtrRead => "ptr_read",
            Builtin::PtrWrite => "ptr_write",
            Builtin::GcAlloc => "gc_alloc",
            Builtin::PtrCopy => "ptr_copy",
            Builtin::EprintString => "eprint_string",
            Builtin::WriteZeroes => "write_zeroes",
        }
    }
    pub fn find(name: Symbol) -> Option<Builtin> {
        match name {
            Symbol::TRANSMUTE => Some(Builtin::Transmute),
            Symbol::ARRAY_LEN => Some(Builtin::Len),
            Symbol::ARRAY_PTR => Some(Builtin::ArrayPtr),
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
            Symbol::PRINT_STRING => Some(Builtin::PrintString),
            Symbol::READ_LINE => Some(Builtin::ReadLine),
            Symbol::PTR_READ => Some(Builtin::PtrRead),
            Symbol::PTR_WRITE => Some(Builtin::PtrWrite),
            Symbol::OFFSET => Some(Builtin::Offset),
            Symbol::GC_ALLOC => Some(Builtin::GcAlloc),
            Symbol::PTR_COPY => Some(Builtin::PtrCopy),
            Symbol::EPRINT_STRING => Some(Builtin::EprintString),
            Symbol::WRITE_ZEROES => Some(Builtin::WriteZeroes),
            _ => None,
        }
    }
}
