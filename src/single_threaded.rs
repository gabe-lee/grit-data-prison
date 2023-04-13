#[cfg(not(feature = "no_std"))]
use std::{ops::RangeBounds, cell::UnsafeCell, mem};

#[cfg(feature = "no_std")]
use core::{ops::RangeBounds, cell::UnsafeCell, mem};

use crate::{AccessError, CellKey, extract_true_start_end};

#[doc(hidden)]
#[derive(Debug)]
struct PrisonInternal<T> {
    visit_count: usize,
    gen: usize,
    next_free: usize,
    vec: Vec<CellOrFree<T>>
}

#[doc(hidden)]
#[derive(Debug)]
struct PrisonCellInternal<T> {
    locked: bool,
    gen: usize,
    val: T,
}


#[doc(hidden)]
#[derive(Debug)]
enum CellOrFree<T> {
    Cell(PrisonCellInternal<T>),
    Free(FreeCell)
}

#[doc(hidden)]
#[derive(Debug)]
struct FreeCell {
    next_free_idx: usize, 
}

/// The single-threaded implementation of [Prison<T>]
/// 
/// This struct uses an underlying [Vec<T>] to store data, but provides full interior mutability
/// for each of its elements. It does this by using [UnsafeCell], simple [bool] locks,
/// and a master [usize] counter that are used to determine what cells (indexes) are currently
/// being accessed to prevent violating Rust's memory management rules (to the best of it's ability).
/// 
/// See the crate-level documentation or individual methods for more info
#[derive(Debug)]
pub struct Prison<T> {
    internal: UnsafeCell<PrisonInternal<T>>,
}

impl<T> Prison<T> {

    /// Create a new [Prison<T>] with the default allocation strategy ([Vec::new()])
    /// 
    /// Because re-allocating the internal [Vec] comes with many restrictions,
    /// it is recommended to use [Prison::with_capacity()] with a suitable 
    /// best-guess starting value rather than [Prison::new()]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() {
    /// let my_prison: Prison<u32> = Prison::new();
    /// assert!(my_prison.cap() < 100)
    /// # }
    /// ```
    #[inline(always)]
    pub fn new() -> Self {
        return Self { 
            internal: UnsafeCell::new(PrisonInternal { visit_count: 0, gen: 0, next_free: NO_FREE, vec: Vec::new() })
        };
    }

    /// Create a new [Prison<T>] with a specific starting capacity ([Vec::with_capacity()])
    /// 
    /// Because re-allocating the internal [Vec] comes with many restrictions,
    /// it is recommended to use [Prison::with_capacity()] with a suitable 
    /// best-guess starting value rather than [Prison::new()]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() {
    /// let my_prison: Prison<u32> = Prison::with_capacity(1000);
    /// assert!(my_prison.cap() == 1000)
    /// # }
    /// ```
    #[inline(always)]
    pub fn with_capacity(size: usize) -> Self {
        return Self { 
            internal: UnsafeCell::new(PrisonInternal { visit_count: 0, gen: 0, next_free: NO_FREE, vec: Vec::with_capacity(size) })
        };
    }

    #[inline(always)]
    fn internal(&self) -> &mut PrisonInternal<T> {
        return unsafe { &mut *self.internal.get() };
    }

    /// Return the length of the underlying `Vec`
    /// 
    /// Length refers to the number of *filled* indexes in an Vec,
    /// not necessarily the number of reserved spaces in memory allocated to it.
    #[inline(always)]
    pub fn len(&self) -> usize {
        return self.internal().vec.len();
    }

    /// Return the capacity of the underlying `Vec`
    /// 
    /// Capacity refers to the number of total spaces in memory reserved for the Vec
    /// to *possibly* use, not the number it currently *has* used
    #[inline(always)]
    pub fn cap(&self) -> usize {
        return self.internal().vec.capacity();
    }

    /// Add a value into the [Prison] and recieve a CellKey that can be used to reference it in the future
    /// 
    /// As long as there is sufficient free cells or vector capacity to do so,
    /// you may `insert()` to the [Prison] while in the middle of any `visit()`
    /// ### Example
    /// ```rust
    /// # use use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(10);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// string_prison.visit(key_0, |first_string| {
    ///     let key_1 = string_prison.insert(String::from("World!"))?;
    ///     string_prison.visit(key_1, |second_string| {
    ///         let hello_world = format!("{}{}", first_string, second_string);
    ///         assert_eq!(hello_world, "Hello, World!");
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// 
    /// However, if the [Prison] is at maxumum capacity, attempting to `insert()`
    /// during a visit will cause the operation to fail and a [AccessError::InsertAtMaxCapacityWhileVisiting]
    /// to be returned
    /// ### Example
    /// ```rust
    /// # use use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(1);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// string_prison.visit(key_0, |first_string| {
    ///     assert!(string_prison.insert(String::from("World!")).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn insert(&self, value: T) -> Result<CellKey, AccessError> {
        self.insert_internal(0, false, false, value)
    }

    /// Remove and return the element indexed by the provided CellKey
    /// 
    /// As long as the element isn't being visited, you can `remove()` it,
    /// even from within an unrelated `.visit()`
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(15);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// let key_1 = string_prison.insert(String::from("World!"))?;
    /// let mut take_world = String::new();
    /// string_prison.visit(key_0, |hello| {
    ///     take_world = string_prison.remove(key_1)?;
    ///     Ok(())
    /// })?;
    /// assert_eq!(take_world, "World!");
    /// # Ok(())
    /// # }
    /// ```
    /// 
    /// However, if the element *is* being visited, `.remove()` will return an
    /// [AccessError::RemoveWhileCellBeingVisited(usize)] error with the index in question
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError>  {
    /// let string_prison: Prison<String> = Prison::with_capacity(15);
    /// let key_0 = string_prison.insert(String::from("Everything"))?;
    /// string_prison.visit(key_0, |everything| {
    ///     assert!(string_prison.remove(key_0).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn remove(&self, key: CellKey) -> Result<T, AccessError> {
        self.remove_internal(key.idx, key.gen, true)
    }

    /// Remove and return the element at the specified index
    /// 
    /// Like `remove()` but disregards the generation counter
    /// 
    /// As long as the element isn't being visited, you can `remove_idx()` it,
    /// even from within an unrelated `.visit()`
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(15);
    /// string_prison.insert(String::from("Hello, "))?;
    /// string_prison.insert(String::from("World!"))?;
    /// let mut take_world = String::new();
    /// string_prison.visit_idx(0, |hello| {
    ///     take_world = string_prison.remove_idx(1)?;
    ///     Ok(())
    /// })?;
    /// assert_eq!(take_world, "World!");
    /// # Ok(())
    /// # }
    /// ```
    /// 
    /// However, if the element *is* being visited, `.remove()` will return an
    /// [AccessError::RemoveWhileCellBeingVisited(usize)] error with the index in question
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError>  {
    /// let string_prison: Prison<String> = Prison::with_capacity(15);
    /// string_prison.insert(String::from("Everything"))?;
    /// string_prison.visit_idx(0, |everything| {
    ///     assert!(string_prison.remove_idx(0).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn remove_idx(&self, idx: usize) -> Result<T, AccessError> {
        self.remove_internal(idx, 0, true)
    }

    /// Visit a single value in the [Prison], obtaining a mutable reference to the 
    /// value that is passed to a closure you provide.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<T, AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// u32_prison.visit(key_0, |mut_ref_42| {
    ///     *mut_ref_42 = 69; // nice
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// You can only visit a cell once at any given time, and cannot move the mutable
    /// reference out of the closure, meaning there is only one mutable reference to it at
    /// any time (and zero immutable references).
    /// 
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::CellAlreadyBeingVisited(usize)], attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(usize)],
    /// and attempting to visit a value that was deleted returns an 
    /// [AccessError::ValueDeleted(usize, usize)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<T, AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(69)?;
    /// u32_prison.remove(key_1)?;
    /// u32_prison.visit(key_0, |mut_ref_42| {
    ///     assert!(u32_prison.visit(key_0, |mut_ref_42_again| {}).is_err());
    ///     assert!(u32_prison.visit(CellKey::from_raw_parts(5, 5), |doesnt_exist| {}).is_err());
    ///     assert!(u32_prison.visit(key_1, |deleted| {}).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// ### Example
    /// ```compile_fail
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<T, AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.push(42)?;
    /// let mut try_to_take_the_ref: &mut u32 = &mut 0;
    /// u32_prison.visit(key_0, |mut_ref_42| {
    ///     // will not compile: (error[E0521]: borrowed data escapes outside of closure)
    ///     try_to_take_the_ref = mut_ref_42;
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn visit<F>(&self, key: CellKey, mut operation: F) -> Result<(), AccessError>
    where F: FnMut(&mut T) -> Result<(), AccessError> {
        self.visit_one_internal(key.idx, key.gen, true, |_, val| operation(val))
    }

    /// Visit a single value in the [Prison], obtaining a mutable reference to the 
    /// value that is passed to a closure you provide.
    /// 
    /// Like `visit()`, but disregards the generation counter entirely
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<T, AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.visit_idx(0, |mut_ref_42| {
    ///     *mut_ref_42 = 69; // nice
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// You can only visit a cell once at any given time, and cannot move the mutable
    /// reference out of the closure, meaning there is only one mutable reference to it at
    /// any time (and zero immutable references).
    /// 
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::CellAlreadyBeingVisited(usize)], attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(usize)],
    /// and attempting to visit a value that was deleted returns an 
    /// [AccessError::ValueDeleted(usize, usize)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<T, AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(69)?;
    /// u32_prison.remove_idx(1)?;
    /// u32_prison.visit_idx(0, |mut_ref_42| {
    ///     assert!(u32_prison.visit_idx(0, |mut_ref_42_again| {}).is_err());
    ///     assert!(u32_prison.visit_idx(5, |doesnt_exist| {}).is_err());
    ///     assert!(u32_prison.visit_idx(1, |deleted| {}).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// ### Example
    /// ```compile_fail
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<T, AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42)?;
    /// let mut try_to_take_the_ref: &mut u32 = &mut 0;
    /// u32_prison.visit_idx(0, |mut_ref_42| {
    ///     // will not compile: (error[E0521]: borrowed data escapes outside of closure)
    ///     try_to_take_the_ref = mut_ref_42;
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn visit_idx<F>(&self, idx: usize, mut operation: F) -> Result<(), AccessError>
    where F: FnMut(&mut T) -> Result<(), AccessError> {
        self.visit_one_internal(idx, 0, false, |_, val| operation(val))
    }

    /// Visit many values in the [Prison] at the same time, obtaining a mutable reference
    /// to all of them in the same closure and in the same order they were requested.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<T, AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.push(42)?;
    /// let key_1 = u32_prison.push(43);
    /// let key_2 = u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_many(&[3, 2, 1, 0], |first_four_reversed| {
    ///     assert_eq!(*first_four_reversed[0], 45);
    ///     assert_eq!(*first_four_reversed[1], 44);
    ///     assert_eq!(*first_four_reversed[2], 43);
    ///     assert_eq!(*first_four_reversed[3], 42);
    /// });
    /// # }
    /// ```
    /// Just like `.visit()`, any particular cell can only be visited once,
    /// but as long as the cells requested don't overlap you may make nested
    /// `visit()`-family calls
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_many(&[0, 2], |evens| {
    ///     u32_prison.visit_many(&[1, 3], |odds| {
    ///         assert_eq!(*evens[1], 44);
    ///         assert_eq!(*odds[1], 45);
    ///     });
    /// });
    /// # }
    /// ```
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::CellAlreadyBeingVisited(usize)], and attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(usize)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// assert!(u32_prison.visit_many(&[0, 0], |double_idx_zero| {}).is_err());
    /// assert!(u32_prison.visit_many(&[0, 1, 2, 3, 4], |invalid_idx| {}).is_err());
    /// # }
    /// ```
    pub fn visit_many<F>(&self, keys: &[CellKey], mut operation: F) -> Result<(), AccessError> 
    where F: FnMut(&mut[&mut T]) -> Result<(), AccessError> {
        self.visit_many_internal(keys, true, |_, vals| operation(vals))
    }

    /// Visit many values in the [Prison] at the same time, obtaining a mutable reference
    /// to all of them in the same closure and in the same order they were requested.
    /// 
    /// Similar to `visit_many()` except the generation tag on the element is ignored
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<T, AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.push(42)?;
    /// let key_1 = u32_prison.push(43);
    /// let key_2 = u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_many(&[3, 2, 1, 0], |first_four_reversed| {
    ///     assert_eq!(*first_four_reversed[0], 45);
    ///     assert_eq!(*first_four_reversed[1], 44);
    ///     assert_eq!(*first_four_reversed[2], 43);
    ///     assert_eq!(*first_four_reversed[3], 42);
    /// });
    /// # }
    /// ```
    /// Just like `.visit_idx()`, any particular cell can only be visited once,
    /// but as long as the cells requested don't overlap you may make nested
    /// `visit()`-family calls
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_many(&[0, 2], |evens| {
    ///     u32_prison.visit_many(&[1, 3], |odds| {
    ///         assert_eq!(*evens[1], 44);
    ///         assert_eq!(*odds[1], 45);
    ///     });
    /// });
    /// # }
    /// ```
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::CellAlreadyBeingVisited(usize)], and attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(usize)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// assert!(u32_prison.visit_many(&[0, 0], |double_idx_zero| {}).is_err());
    /// assert!(u32_prison.visit_many(&[0, 1, 2, 3, 4], |invalid_idx| {}).is_err());
    /// # }
    /// ```
    pub fn visit_many_idx<F>(&self, indexes: &[usize], mut operation: F) -> Result<(), AccessError> 
    where F: FnMut(&mut[&mut T]) -> Result<(), AccessError> {
        let keys: Vec<CellKey> = indexes.iter().map(|idx| CellKey{ idx: *idx, gen: 0}).collect();
        self.visit_many_internal(&keys, false, |_, vals| operation(vals))
    }

    /// Visit a slice of values in the [Prison] at the same time, obtaining a mutable reference
    /// to all of them in the same closure.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.push(46);
    /// u32_prison.visit_slice(2..5, |last_three| {
    ///     assert_eq!(*last_three[0], 44);
    ///     assert_eq!(*last_three[1], 45);
    ///     assert_eq!(*last_three[2], 46);
    /// });
    /// # }
    /// ```
    /// Any standard Range<usize> notation is allowed as the first paramater
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.push(46);
    /// assert!(u32_prison.visit_slice(2..5,  |last_three| { }).is_ok());
    /// assert!(u32_prison.visit_slice(2..=4, |also_last_three| { }).is_ok());
    /// assert!(u32_prison.visit_slice(2..,   |again_last_three| { }).is_ok());
    /// assert!(u32_prison.visit_slice(..3,   |first_three| { }).is_ok());
    /// assert!(u32_prison.visit_slice(..=3,  |first_four| { }).is_ok());
    /// assert!(u32_prison.visit_slice(..,    |all| { }).is_ok());
    /// # }
    /// ```
    /// Just like `.visit()`, any particular cell can only be visited once,
    /// but as long as the cells requested don't overlap you may make nested
    /// `visit()`-family calls
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_slice(..2, |first_half| {
    ///     u32_prison.visit_slice(2.., |second_half| {
    ///         assert_eq!(*first_half[1], 43);
    ///         assert_eq!(*second_half[0], 44);
    ///     });
    /// });
    /// # }
    /// ```
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::CellAlreadyBeingVisited(usize)], and attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(usize)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_slice(.., |all| {
    ///     assert!(u32_prison.visit_slice(0..1, |second_visit_to_first_idx| {}).is_err());
    /// });
    /// assert!(u32_prison.visit_slice(0..10, |invalid_idx| {}).is_err());
    /// # }
    /// ```
    pub fn visit_slice<R, F>(&self, range: R, mut operation: F) -> Result<(), AccessError>
    where
    R: RangeBounds<usize>,
    F:  FnMut(&mut[&mut T]) -> Result<(), AccessError> {
        let (start, end) = extract_true_start_end(range, self.len());
        let keys: Vec<CellKey> = (start..end).map(|idx| CellKey {idx, gen: 0}).collect();
        self.visit_many_internal(&keys, false, |_, vals| operation(vals))
    }
}


/**************************
 * INTERNAL IMPLEMENTATIONS
 **************************/
macro_rules! internal {
    ($p:ident) => {
        unsafe {&mut *$p.internal.get()}
    };
}

const NO_FREE: usize = usize::MAX;
const MAX_CAP: usize = isize::MAX as usize;

impl<T> Prison<T> {
    #[doc(hidden)]
    fn insert_internal(&self, idx: usize, specific_idx: bool, overwrite: bool, val: T) -> Result<CellKey, AccessError> {
        let internal = internal!(self);
        let vec_len = internal.vec.len();
        let new_idx = if specific_idx {
            if idx >= vec_len {
                return Err(AccessError::IndexOutOfRange(idx));
            }
            idx
        } else if internal.next_free != NO_FREE {
            internal.next_free
        } else {
            if internal.vec.capacity() <= internal.vec.len() && internal.visit_count > 0 {
                return Err(AccessError::InsertAtMaxCapacityWhileVisiting);
            }
            if internal.vec.capacity() == MAX_CAP {
                return Err(AccessError::MaximumCapacityReached);
            }
            internal.vec.push(CellOrFree::Cell(PrisonCellInternal { locked: false, gen: internal.gen, val }));
            return Ok(CellKey { idx: internal.vec.len()-1, gen: internal.gen })
        };
        internal.vec[new_idx] = match &internal.vec[new_idx] {
            CellOrFree::Cell(cell) => {
                if !overwrite {
                    return Err(AccessError::IndexIsNotFree(new_idx))
                }
                if cell.locked {
                    return Err(AccessError::IndexAlreadyBeingVisited(new_idx));
                }
                if cell.gen >= internal.gen {
                    if cell.gen == usize::MAX {
                        return Err(AccessError::MaxValueForGenerationReached)
                    }
                    internal.gen = cell.gen + 1;
                }
                CellOrFree::Cell(PrisonCellInternal { locked: false, gen: internal.gen, val })
            },
            CellOrFree::Free(free) => {
                internal.next_free = free.next_free_idx;
                CellOrFree::Cell(PrisonCellInternal { locked: false, gen: internal.gen, val })
            },
        };
        return Ok(CellKey { idx: new_idx, gen: internal.gen });
    }

    #[doc(hidden)]
    fn remove_internal(&self, idx: usize, gen: usize, use_gen: bool) -> Result<T, AccessError> {
        let internal = internal!(self);
        if idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(idx))
        }
        let new_free = match &mut internal.vec[idx] {
            CellOrFree::Cell(cell) if (!use_gen || cell.gen == gen) => {
                if cell.locked {
                    return Err(AccessError::RemoveWhileIndexBeingVisited(idx));
                }
                if cell.gen >= internal.gen {
                    if cell.gen == usize::MAX {
                        return Err(AccessError::MaxValueForGenerationReached)
                    }
                    internal.gen = cell.gen + 1;
                }
                CellOrFree::Free(FreeCell { next_free_idx: internal.next_free })
            },
            _ => return Err(AccessError::ValueDeleted(idx, gen)),
        };
        internal.next_free = idx;
        let old_cell = mem::replace(&mut internal.vec[idx], new_free);
        return if let CellOrFree::Cell(cell) = old_cell {
            Ok(cell.val)
        } else {
            Err(AccessError::ValueDeleted(idx, gen))
        }
    }

    #[doc(hidden)]
    fn visit_one_internal<FF>(&self, idx: usize, gen: usize, use_gen: bool, mut ff: FF) -> Result<(), AccessError>
    where FF: FnMut(usize, &mut T) -> Result<(), AccessError> {
        let internal = internal!(self);
        if idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(idx));
        }
        match &mut internal.vec[idx] {
            CellOrFree::Cell(cell) if (!use_gen || cell.gen == gen) => {
                if cell.locked {
                    return Err(AccessError::IndexAlreadyBeingVisited(idx));
                }
                internal.visit_count += 1;
                cell.locked = true;
                let res = ff(idx, &mut cell.val);
                cell.locked = false;
                internal.visit_count -= 1;
                return res;
            },
            _ => return Err(AccessError::ValueDeleted(idx, gen)),
        }
    }

    #[doc(hidden)]
    fn visit_many_internal<FF>(&self, cell_keys: &[CellKey], use_gens: bool, mut ff: FF) -> Result<(), AccessError>
    where FF: FnMut(&[usize], &mut[&mut T]) -> Result<(), AccessError> {
        if cell_keys.len() == 0 {
            return Ok(());
        }
        let internal = internal!(self);
        let mut vals = Vec::new();
        let mut indices = Vec::new();
        let mut locks = Vec::new();
        let mut ret_value = Ok(());
        for key in cell_keys {
            if key.idx >= internal.vec.len() {
                ret_value = Err(AccessError::IndexOutOfRange(key.idx));
                break;
            }
            match &mut internal!(self).vec[key.idx] {
                CellOrFree::Cell(cell) if (!use_gens || cell.gen == key.gen) => {
                    if cell.locked {
                        ret_value = Err(AccessError::IndexAlreadyBeingVisited(key.idx));
                        break;
                    }
                    cell.locked = true;
                    locks.push(&mut cell.locked);
                    indices.push(key.idx);
                    vals.push(&mut cell.val);
                },
                _ => {
                    ret_value = Err(AccessError::ValueDeleted(key.idx, key.gen));
                    break;
                },
            }
        }
        if ret_value.is_ok() {
            internal.visit_count += 1;
            ret_value = ff(&indices, vals.as_mut_slice());
            internal.visit_count -= 1;
        }
        for lock in locks {
            *lock = false;
        }
        return ret_value;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq)]
    struct MyNoCopy(usize);

    #[test]
    fn insert_internal() {
        let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
        match prison.insert_internal(0, true, false, MyNoCopy(0)) {
            Err(e) if (e == AccessError::IndexOutOfRange(0)) => {},
            _ => panic!()
        };
        match prison.insert_internal(0, true, true, MyNoCopy(0)) {
            Err(e) if (e == AccessError::IndexOutOfRange(0)) => {},
            _ => panic!()
        };
        match prison.insert_internal(0, false, false, MyNoCopy(99)) {
            Ok(key) if (key.idx == 0 && key.gen == 0) => {},
            _ => panic!()
        };
        let key_0 = match prison.insert_internal(0, true, true, MyNoCopy(0)) {
            Ok(key) if (key.idx == 0 && key.gen == 1) => key,
            _ => panic!()
        };
        match &internal!(prison).vec[0] {
            CellOrFree::Cell(cell) if (cell.val == MyNoCopy(0)) => {},
            _ => panic!(),
        }
        assert!(prison.visit(key_0, |_| {
            match prison.insert_internal(0, true, false, MyNoCopy(1)) {
                Err(e) if (e == AccessError::IndexIsNotFree(0)) => {},
                _ => panic!()
            };
            match prison.insert_internal(0, true, true, MyNoCopy(1)) {
                Err(e) if (e == AccessError::IndexAlreadyBeingVisited(0)) => {},
                _ => panic!()
            };
            match prison.insert_internal(1, false, true, MyNoCopy(1)) {
                Ok(key) if (key.idx == 1 && key.gen == 1) => {},
                _ => panic!()
            };
            match prison.insert_internal(1, true, true, MyNoCopy(11)) {
                Ok(key) if (key.idx == 1 && key.gen == 2) => {},
                _ => panic!()
            };
            match prison.insert_internal(0, false, false, MyNoCopy(2)) {
                Ok(key) if (key.idx == 2 && key.gen == 2) => {},
                _ => panic!()
            };
            assert_eq!(internal!(prison).gen, 2);
            internal!(prison).vec[1] = CellOrFree::Free(FreeCell { next_free_idx: NO_FREE });
            internal!(prison).next_free = 1;
            match prison.insert_internal(0, false, false, MyNoCopy(111)) {
                Ok(key) if (key.idx == 1 && key.gen == 2) => {},
                _ => panic!()
            };
            assert_eq!(internal!(prison).next_free, NO_FREE);
            match &internal!(prison).vec[1] {
                CellOrFree::Cell(cell) if (cell.gen == 2 && cell.val == MyNoCopy(111)) => {},
                _ => panic!(),
            };
            match prison.insert_internal(0, false, false, MyNoCopy(4)) {
                Err(e) if (e == AccessError::InsertAtMaxCapacityWhileVisiting) => {},
                _ => panic!()
            };
            Ok(())
        }).is_ok());
        internal!(prison).vec[1] = CellOrFree::Cell(PrisonCellInternal { locked: false, gen: usize::MAX, val: MyNoCopy(9999) });
        match prison.insert_internal(1, true, true, MyNoCopy(1)) {
            Err(e) if (e == AccessError::MaxValueForGenerationReached) => {},
            _ => panic!()
        };
        // No good way to test a vec at len() == MAX_CAP == isize::MAX
    }

    #[test]
    fn remove_internal() {
        let prison: Prison<MyNoCopy> = Prison::with_capacity(10);
        match prison.remove_internal(0, 0, false) {
            Err(e) if (e == AccessError::IndexOutOfRange(0)) => {},
            _ => panic!()
        };
        for i in 0..10usize {
            assert!(prison.insert_internal(0, false, false, MyNoCopy(i)).is_ok());
        }
        assert!(prison.insert_internal(5, true, true, MyNoCopy(555)).is_ok());
        match prison.remove_internal(10, 0, true) {
            Err(e) if (e == AccessError::IndexOutOfRange(10)) => {},
            _ => panic!()
        };
        match prison.remove_internal(9, 0, true) {
            Ok(val) if (val == MyNoCopy(9)) => {},
            _ => panic!()
        };
        match prison.remove_internal(0, 0, false) {
            Ok(val) if (val == MyNoCopy(0)) => {},
            _ => panic!()
        };
        match prison.remove_internal(5, 0, true) {
            Err(e) if (e == AccessError::ValueDeleted(5, 0)) => {},
            _ => panic!()
        };
        match prison.remove_internal(5, 1, true) {
            Ok(val) if (val == MyNoCopy(555)) => {},
            _ => panic!()
        };
        assert!(prison.visit_idx(3, |_| {
            match prison.remove_internal(8, 0, true) {
                Ok(val) if (val == MyNoCopy(8)) => {},
                _ => panic!()
            };
            match prison.remove_internal(3, 0, true) {
                Err(e) if (e == AccessError::RemoveWhileIndexBeingVisited(3)) => {},
                _ => panic!()
            };
            Ok(())
        }).is_ok());
        internal!(prison).vec[4] = CellOrFree::Cell(PrisonCellInternal { locked: false, gen: usize::MAX, val: MyNoCopy(4444) });
        match prison.remove_internal(4, usize::MAX, true) {
            Err(e) if (e == AccessError::MaxValueForGenerationReached) => {},
            _ => panic!()
        };
    }

    #[test]
    #[allow(unused_variables)]
    fn visit_one_internal() {
        let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
        match prison.visit_one_internal(0, 0, false, |idx_1, val_1| Ok(())) {
            Err(e) if (e == AccessError::IndexOutOfRange(0)) => {},
            _ => panic!()
        };
        let key_0 = prison.insert_internal(0, false, false, MyNoCopy(0)).unwrap();
        let key_1 = prison.insert_internal(1, false, false, MyNoCopy(1)).unwrap();
        assert!(prison.visit_one_internal(key_0.idx, key_0.gen, true, |idx_0, val_0| {
            match prison.visit_one_internal(key_0.idx, 99, false, |idx_0_again, val_0_again| Ok(())) {
                Err(e) if (e == AccessError::IndexAlreadyBeingVisited(0)) => {},
                _ => panic!()
            };
            match prison.visit_one_internal(key_0.idx, key_0.gen, true, |idx_0_again, val_0_again| Ok(())) {
                Err(e) if (e == AccessError::IndexAlreadyBeingVisited(0)) => {},
                _ => panic!()
            };
            *val_0 = MyNoCopy(100);
            assert!(prison.visit_one_internal(key_1.idx, 99, false, |idx_1, val_1| {
                *val_1 = MyNoCopy(101);
                Ok(())
            }).is_ok());
            match &internal!(prison).vec[0] {
                CellOrFree::Cell(cell) if (cell.val == MyNoCopy(100)) => {},
                _ => panic!(),
            }
            match &internal!(prison).vec[1] {
                CellOrFree::Cell(cell) if (cell.val == MyNoCopy(101)) => {},
                _ => panic!(),
            }
            prison.remove_internal(key_1.idx, key_1.gen, true).unwrap();
            match prison.visit_one_internal(key_1.idx, key_1.gen, false, |idx_1, val_1| Ok(())) {
                Err(e) if (e == AccessError::ValueDeleted(key_1.idx, key_1.gen)) => {},
                _ => panic!()
            };
            Ok(())
        }).is_ok());
    }

    #[test]
    #[allow(unused_variables)]
    fn visit_many_internal() {
        let prison: Prison<MyNoCopy> = Prison::with_capacity(10);
        let mut keys = Vec::new();
        for i in 0..10usize {
            keys.push(prison.insert_internal(0, false, false, MyNoCopy(i)).unwrap());
        }
        assert!(prison.visit_many_internal(&[], true, |nothing, none| Ok(())).is_ok());
        assert!(prison.visit_many_internal(&keys[0..1], true, |idx_0, val_0| {
            assert!(prison.visit_many_internal(&keys[1..5], true, |idx_1_4, val_1_4| {
                match prison.visit_many_internal(&[CellKey{idx: 10, gen: 0}, CellKey{idx: 11, gen: 0}, CellKey{idx: 12, gen: 0}], true, |out_of_bounds, bad| Ok(())) {
                    Err(e) if (e == AccessError::IndexOutOfRange(10)) => {},
                    _ => panic!()
                };
                assert!(prison.visit_many_internal(&keys[5..10], false, |idx_5_9, val_5_9| {
                    match prison.visit_many_internal(&keys[2..9], true, |idx_1, val_1| Ok(())) {
                        Err(e) if (e == AccessError::IndexAlreadyBeingVisited(2)) => {},
                        _ => panic!()
                    };
                    assert!(prison.visit_many_internal(&[], true, |nothing, none| Ok(())).is_ok());
                    Ok(())
                }).is_ok());
                prison.remove_internal(9, 0, true).unwrap();
                match prison.visit_many_internal(&keys[5..10], true, |idx_1, val_1| Ok(())) {
                    Err(e) if (e == AccessError::ValueDeleted(9, 0)) => {},
                    _ => panic!()
                };
                Ok(())
            }).is_ok());
            Ok(())
        }).is_ok());
        match prison.visit_many_internal(&keys, true, |all_idx, all_vals| Ok(())) {
            Err(e) if (e == AccessError::ValueDeleted(9, 0)) => {},
            _ => panic!()
        };
        let new_key_9 = prison.insert_internal(9, true, true, MyNoCopy(9)).unwrap();
        keys[9] = new_key_9;
        assert!(prison.visit_many_internal(&keys, true, |all_idx, all_vals| Ok(())).is_ok());
    }
}