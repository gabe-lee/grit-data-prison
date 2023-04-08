/*!
This crate provides the generic type [`Prison<T>`], a data structure that uses an underlying `Vec<T>`
to store values of the same type, but allows simultaneous interior mutability to each and every
value by providing .visit() methods that take closures that are passed mutable references to the values.

This documentation describes the usage of [`Prison<T>`], how its `Vec` analogous methods differ from
those found on a `Vec`, and how it achieves memory safety.

# DOCUMENTATION IN PROGRESS!


*/

// #![deny(rustdoc::broken_intra_doc_links)]
// #![deny(rustdoc::private_intra_doc_links)]
// #![deny(missing_docs)]

#[cfg(not(feature = "no_std"))]
use std::{cell::UnsafeCell, ops::RangeBounds, error::Error, fmt::{Display, Debug}};

#[cfg(feature = "no_std")]
use core::{cell::UnsafeCell, ops::RangeBounds, fmt::{Display, Debug}};

#[cfg(feature = "no_std")]
trait Error: Debug + Display {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        None
    }
}

pub mod single_threaded;

pub enum AccessError {
    IndexOutOfRange(usize),
    CellAlreadyBeingVisited(usize),
    PushAtMaxCapacityWhileVisiting,
    PopWhileLastElementIsVisited(usize),
    PopOnEmptyPrison
}

impl Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::IndexOutOfRange(idx) => write!(f, "Index [{}] is out of range", idx),
            Self::CellAlreadyBeingVisited(idx) => write!(f, "Cell at index [{}] is already being visited", idx),
            Self::PushAtMaxCapacityWhileVisiting => write!(f, "Prison is at max capacity, cannot push() new value while visiting"),
            Self::PopWhileLastElementIsVisited(idx) => write!(f, "Last index [{}] is being visited, cannot pop() it out", idx),
            Self::PopOnEmptyPrison => write!(f, "Prison is empty, nothing to pop() out"),
        }
    }
}

impl Debug for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::IndexOutOfRange(idx) => write!(f, "Index [{}] is out of range", idx),
            Self::CellAlreadyBeingVisited(idx) => write!(f, "Cell at index [{}] is already being visited\n---------\nVisiting the same cell twice would give two mutable references to the same memory. You could potentially alter some expected pre-condition the compiler expects of the value, such as changing an Enum's Variant or deleting all the items from a Vector expected to have a non-zero length.", idx),
            Self::PushAtMaxCapacityWhileVisiting => write!(f, "Prison is at max capacity\n---------\nPushing to a Vec at max capacity while a visit is in progress may cause re-allocation that will invalidate value references"),
            Self::PopWhileLastElementIsVisited(idx) => write!(f, "Last index [{}] is being visited, cannot pop() it out\n---------\nThe referenced data will become invalid, as there is no guarantee the data will not be overwritten as it no longer belongs to the Vec", idx),
            Self::PopOnEmptyPrison => write!(f, "Prison is empty, nothing to pop() out"),
        }
    }
}

impl Error for AccessError {}

#[doc(hidden)]
#[derive(Debug)]
pub(crate) struct LockValPair<T>(UnsafeCell<(bool, T)>);

#[doc(hidden)]
impl<T> LockValPair<T> {
    pub(crate) fn new(val: T) -> LockValPair<T> {
        return LockValPair(UnsafeCell::new((false, val)));
    }

    pub(crate) fn open(&self) -> (&mut bool, &mut T) {
        let mut_pair = unsafe { &mut *self.0.get() };
        return (&mut mut_pair.0, &mut mut_pair.1);
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub(crate) struct CountVecPair<T>(UnsafeCell<(usize, Vec<LockValPair<T>>)>);

impl<T> CountVecPair<T> {
    pub(crate) fn new() -> Self {
        return Self(UnsafeCell::new((0, Vec::new())));
    }

    pub(crate) fn with_capacity(size: usize) -> Self {
        return Self(UnsafeCell::new((0, Vec::with_capacity(size))));
    }

    pub(crate) fn open(&self) -> (&mut usize, &mut Vec<LockValPair<T>>) {
        let mut_pair = unsafe { &mut *self.0.get() };
        return (&mut mut_pair.0, &mut mut_pair.1);
    }
}

#[doc(hidden)]
fn extract_true_start_end<B>(range: B, max_len: usize) -> (usize, usize) 
    where
    B: RangeBounds<usize> {
    let start = match range.start_bound() {
        std::ops::Bound::Included(first) => *first,
        std::ops::Bound::Excluded(one_before_first) => *one_before_first + 1,
        std::ops::Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        std::ops::Bound::Included(last) => *last + 1,
        std::ops::Bound::Excluded(one_after_last) => *one_after_last,
        std::ops::Bound::Unbounded => max_len,
    };
    return (start, end);
}