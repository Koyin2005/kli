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
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Builtin {
    // Arrays
    Len,

    //Strings
    StringLen,
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
            Builtin::IntegerBuiltin(IntegerBuiltin::WrappingAdd) => "wrapping_add",
            Builtin::IntegerBuiltin(IntegerBuiltin::OverflowingAdd) => "overflowing_add",
            Builtin::IntegerBuiltin(IntegerBuiltin::OverflowingSub) => "overflowing_sub",
            Builtin::IntegerBuiltin(IntegerBuiltin::WrappingSub) => "wrapping_sub",
            Builtin::IntegerBuiltin(IntegerBuiltin::OverflowingMul) => "overflowing_mul",
            Builtin::IntegerBuiltin(IntegerBuiltin::WrappingMul) => "wrapping_mul",
            Builtin::IntegerBuiltin(IntegerBuiltin::IntMaxValue) => "int_max_value",
            Builtin::IntegerBuiltin(IntegerBuiltin::ShiftLeft) => "shift_left",
            Builtin::IntegerBuiltin(IntegerBuiltin::ShiftRight) => "shift_right",
            Builtin::Len => "array_len",
            Builtin::PrintString => "print_string",
            Builtin::ReadLine => "read_line",
            Builtin::EprintString => "eprint_string",
            Builtin::StringLen => "string_len",
        }
    }
    pub fn find(name: Symbol) -> Option<Builtin> {
        match name {
            Symbol::ARRAY_LEN => Some(Builtin::Len),
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
            Symbol::INT_MAX_VALUE => Some(Builtin::IntegerBuiltin(IntegerBuiltin::IntMaxValue)),
            Symbol::PRINT_STRING => Some(Builtin::PrintString),
            Symbol::READ_LINE => Some(Builtin::ReadLine),
            Symbol::EPRINT_STRING => Some(Builtin::EprintString),
            Symbol::STRING_LEN => Some(Builtin::StringLen),
            _ => None,
        }
    }
}
