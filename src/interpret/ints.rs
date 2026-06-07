use crate::interpret::Endianess;
use std::{
    fmt::Display,
    ops::{Add, AddAssign, BitOr, BitOrAssign, Div, Mul, Neg, Rem, Shl, Shr, Sub},
};

#[derive(Debug, PartialEq, Eq, Clone, Hash)]
pub struct Int(i64);
impl Int {
    pub const ZERO: Self = Self(0);
    pub fn new(value: i64) -> Self {
        Self(value)
    }
    pub const fn is_negative(&self) -> bool {
        self.0.is_negative()
    }
    pub fn overflowing_add(self, other: Self) -> (Self,bool){
        let (result,overflow) = self.0.overflowing_add(other.0);
        (Self(result),overflow)
    }
    pub fn overflowing_sub(self, other: Self) -> (Self,bool){
        let (result,overflow) = self.0.overflowing_sub(other.0);
        (Self(result),overflow)
    }
    pub fn overflowing_mul(self, other: Self) -> (Self,bool){
        let (result,overflow) = self.0.overflowing_mul(other.0);
        (Self(result),overflow)
    }
    pub fn overflowing_div(self, other: Self) -> (Self,bool){
        let (result,overflow) = self.0.overflowing_div(other.0);
        (Self(result),overflow)
    }
    pub fn as_u8(&self) -> Option<u8> {
        self.0.try_into().ok()
    }
    pub fn into_size(self) -> usize {
        self.0 as usize
    }
    pub fn from_size(size: usize) -> Self {
        Self(size as i64)
    }
    pub const fn abs(self) -> Self {
        Self(self.0.unsigned_abs() as i64)
    }
}
impl<T: Into<i64>> From<T> for Int {
    fn from(value: T) -> Self {
        Self(value.into())
    }
}
impl Display for Int {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl Neg for Int {
    type Output = Int;
    fn neg(self) -> Self::Output {
        Self(-self.0)
    }
}
impl Add for Int {
    type Output = Int;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}
impl Sub for Int {
    type Output = Int;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0.wrapping_sub(rhs.0))
    }
}
impl Mul for Int {
    type Output = Int;
    fn mul(self, rhs: Self) -> Self::Output {
        Self(self.0.wrapping_mul(rhs.0))
    }
}
impl Div for Int {
    type Output = Int;
    fn div(self, rhs: Self) -> Self::Output {
        Self(self.0.wrapping_div(rhs.0))
    }
}
impl AddAssign for Int {
    fn add_assign(&mut self, rhs: Self) {
        *self = Self(self.0 + rhs.0);
    }
}
impl Shl for Int {
    type Output = Int;
    fn shl(self, rhs: Self) -> Self::Output {
        Self(self.0 << rhs.0)
    }
}
impl Shr for Int {
    type Output = Int;
    fn shr(self, rhs: Self) -> Self::Output {
        Self(self.0 >> rhs.0)
    }
}
impl Rem for Int {
    type Output = Int;
    fn rem(self, rhs: Self) -> Self::Output {
        Self(self.0 % rhs.0)
    }
}
impl BitOr for Int {
    type Output = Int;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}
impl BitOrAssign for Int {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

pub fn encode_int(e: Endianess, value: Int, size: usize) -> Vec<u8> {
    let is_neg = value.is_negative();
    let value = value.abs();
    let mut bytes = Vec::<u8>::new();
    let mut j = (size - 1) as isize;
    let mut byte = ((value.clone() >> Int::from_size((j * 8).try_into().unwrap())) % Int::new(256))
        .as_u8()
        .unwrap();
    if is_neg {
        byte |= 0b1000_0000;
    }
    bytes.push(byte);
    j -= 1;
    while j >= 0 {
        let byte = ((value.clone() >> Int::from_size((j * 8).try_into().unwrap())) % Int::new(256))
            .as_u8()
            .unwrap();
        bytes.push(byte);
        j -= 1;
    }
    if matches!(e, Endianess::Little) {
        bytes.reverse();
    }
    bytes
}
pub fn decode_int(e: Endianess, mut bytes: Vec<u8>) -> Int {
    if matches!(e, Endianess::Little) {
        bytes.reverse();
    }
    let byte = bytes.first().copied().unwrap();
    let mut value = Int::from(byte as i8);
    for &b in bytes.iter().skip(1) {
        value = (value << Int::new(8)) | Int::new(b as i64);
    }
    value
}
