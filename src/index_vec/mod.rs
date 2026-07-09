use std::{
    fmt::Debug,
    hash::Hash,
    marker::PhantomData,
    ops::{Index, IndexMut},
};
#[derive(PartialEq, Eq, Clone, Hash)]
pub struct IndexVec<I, V>(Vec<V>, PhantomData<I>);

impl<I: Debug + Id, V: Debug> Debug for IndexVec<I, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter_enumerated()).finish()
    }
}
impl<I, V> Default for IndexVec<I, V> {
    fn default() -> Self {
        Self(Vec::default(), PhantomData)
    }
}
#[macro_export]
macro_rules! define_id {
    ($(#[$meta:meta])*$name:ident) => {
        $(#[$meta])*
        #[derive(Debug, PartialEq, Eq, Clone, Hash, Copy,PartialOrd,Ord)]
        pub struct $name(u32);
        impl $name {
            pub const fn new(id: usize) -> Self {
                if id < u32::MAX as usize {
                    $name(id as u32)
                } else {
                    panic!("too many ids")
                }
            }
            pub const fn into_usize(self) -> usize {
                self.0 as usize
            }
            pub const fn next(self) -> Self {
                Self::new((self.0 + 1) as usize)
            }
        }
        impl $crate::index_vec::Id for $name {
            fn new(id: usize) -> Self {
                Self::new(id)
            }
            fn into_usize(self) -> usize {
                Self::into_usize(self)
            }
            fn next(self) -> Self {
                Self::next(self)
            }
        }
    };
}

pub trait Id: Sized + Hash + Copy + Clone + Debug + PartialEq + Eq {
    fn new(id: usize) -> Self;
    fn into_usize(self) -> usize;
    fn next(self) -> Self;
}

impl<I: Id, V> IndexVec<I, V> {
    pub const fn new() -> Self {
        Self(Vec::new(), PhantomData)
    }
    pub fn new_from(count: usize, value: V) -> Self
    where
        V: Clone,
    {
        if count == 0 {
            return Self::new();
        } else if count == 1 {
            let mut v = Vec::with_capacity(1);
            v.push(value);
            return Self(v, PhantomData);
        }
        Self(
            Vec::from_iter(std::iter::repeat_n(value, count)),
            PhantomData,
        )
    }
    pub fn push(&mut self, value: V) -> I {
        let index = I::new(self.0.len());
        self.0.push(value);
        index
    }
    pub fn get(&self, i: I) -> Option<&V> {
        self.0.get(i.into_usize())
    }
    pub fn get_mut(&mut self, i: I) -> Option<&mut V> {
        self.0.get_mut(i.into_usize())
    }
    #[track_caller]
    pub fn expect_get(&self, i: I) -> &V {
        &self.0[i.into_usize()]
    }
    #[track_caller]
    pub fn expect_get_mut(&mut self, i: I) -> &mut V {
        &mut self.0[i.into_usize()]
    }
    pub fn iter(&self) -> impl Iterator<Item = &V> {
        self.0.iter()
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.0.iter_mut()
    }
    pub const fn len(&self) -> usize {
        self.0.len()
    }
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn into_iter_enumerated(self) -> impl Iterator<Item = (I, V)> {
        self.0.into_iter().enumerate().map(|(i, v)| (I::new(i), v))
    }
    pub fn iter_enumerated(&self) -> impl Iterator<Item = (I, &V)> {
        self.0.iter().enumerate().map(|(i, v)| (I::new(i), v))
    }
    pub fn indices(&self) -> impl Iterator<Item = I> + use<I, V> {
        (0..self.len()).map(Id::new)
    }
    pub fn iter_mut_enumerated(&mut self) -> impl Iterator<Item = (I, &mut V)> {
        self.0.iter_mut().enumerate().map(|(i, v)| (I::new(i), v))
    }
    pub fn into_vec(self) -> Vec<V> {
        self.0
    }
    pub const fn as_slice(&self) -> &[V] {
        self.0.as_slice()
    }
    pub fn extend(&mut self, iter: impl IntoIterator<Item = V>) {
        self.0.extend(iter);
    }
    pub fn retain(&mut self, mut f: impl FnMut(I, &V) -> bool) {
        let mut i = I::new(0);
        self.0.retain(|v| {
            let keep = f(i, v);
            i = i.next();
            keep
        });
    }
}

impl<I: Id, V> Index<I> for IndexVec<I, V> {
    type Output = V;
    fn index(&self, index: I) -> &Self::Output {
        self.expect_get(index)
    }
}
impl<I: Id, V> IndexMut<I> for IndexVec<I, V> {
    fn index_mut(&mut self, index: I) -> &mut Self::Output {
        self.expect_get_mut(index)
    }
}
impl<I: Id, V> IntoIterator for IndexVec<I, V> {
    type IntoIter = std::vec::IntoIter<V>;
    type Item = V;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl<'a, I: Id, V> IntoIterator for &'a IndexVec<I, V> {
    type IntoIter = std::slice::Iter<'a, V>;
    type Item = &'a V;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
impl<'a, I: Id, V> IntoIterator for &'a mut IndexVec<I, V> {
    type IntoIter = std::slice::IterMut<'a, V>;
    type Item = &'a mut V;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter_mut()
    }
}
impl<I: Id, V> FromIterator<V> for IndexVec<I, V> {
    fn from_iter<T: IntoIterator<Item = V>>(iter: T) -> Self {
        Self(Vec::from_iter(iter), PhantomData)
    }
}

impl<const N: usize, I: Id, T> From<[T; N]> for IndexVec<I, T> {
    fn from(value: [T; N]) -> Self {
        Self::from_iter(value)
    }
}
