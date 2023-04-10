#[cfg(not(feature = "no_std"))]
use std::{ops::RangeBounds, cell::UnsafeCell};

#[cfg(feature = "no_std")]
use core::{ops::RangeBounds, cell::UnsafeCell};

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
    last_gen: usize,
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
            internal: UnsafeCell::new(PrisonInternal { visit_count: 0, gen: 0, next_free: 0, vec: Vec::new() })
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
            internal: UnsafeCell::new(PrisonInternal { visit_count: 0, gen: 0, next_free: 0, vec: Vec::with_capacity(size) })
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

    #[doc(hidden)]
    fn insert_internal(&self, value: T, index: usize) -> CellKey {
        let internal = self.internal();
        internal.vec[index] = CellOrFree::Cell(PrisonCellInternal { locked: false, gen: internal.gen, val: value });
        return CellKey { idx: index, gen: internal.gen }
    }

    #[doc(hidden)]
    fn push_internal(&self, value: T) -> CellKey {
        let internal = self.internal();
        let idx = internal.vec.len();
        internal.vec.push(CellOrFree::Cell(PrisonCellInternal { locked: false, gen: internal.gen, val: value }));
        return CellKey { idx, gen: internal.gen }
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
    pub fn insert(&self, value: T) -> Result<CellKey, AccessError> {
        let internal = self.internal();
        let mut new_idx = internal.next_free;
        let vec_len = internal.vec.len();
        if new_idx < vec_len {
            if let CellOrFree::Free(FreeCell { next_free_idx, last_gen }) = internal.vec[new_idx] {
                internal.next_free = next_free_idx;
                if internal.gen <= last_gen {
                    if last_gen == usize::MAX {
                        return Err(AccessError::MaxValueForGenerationReached)
                    }
                    internal.gen = last_gen + 1;
                }
                return Ok(self.insert_internal(value, new_idx));
            } else {
                new_idx = vec_len;
            }
        }
        if new_idx >= internal.vec.capacity() && internal.visit_count > 0 {
            return Err(AccessError::InsertAtMaxCapacityWhileVisiting);
        }
        return Ok(self.push_internal(value));
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
    /// // visit key 0, "Hello, "
    /// string_prison.visit(key_0, |hello| {
    ///     // remove key 1, "World!"
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
    pub fn remove(&self, key: CellKey) -> Result<T, AccessError> {
        let internal = self.internal();
        if key.idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(key.idx))
        }
        match internal.vec[key.idx] {
            CellOrFree::Cell(cell) if cell.gen == key.gen => {
                if cell.locked {
                    return Err(AccessError::RemoveWhileCellBeingVisited(key.idx));
                }
                let new_free = CellOrFree::Free(FreeCell { next_free_idx: internal.next_free, last_gen: cell.gen });
                if internal.gen <= cell.gen {
                    if cell.gen == usize::MAX {
                        return Err(AccessError::MaxValueForGenerationReached)
                    }
                    internal.gen = cell.gen + 1;
                }
                internal.next_free = key.idx;
                internal.vec[key.idx] = new_free;
                return Ok(cell.val);
            },
            _ => return Err(AccessError::ValueDeleted(key.idx, key.gen)),
        }
    }

    #[doc(hidden)]
    fn visit_one_internal<FF>(&self, idx: usize, gen: usize, use_gen: bool, ff: FF) -> Result<(), AccessError>
    where FF: FnMut(usize, &mut T) -> Result<(), AccessError>
     {
        let internal = self.internal();
        if idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(idx));
        }
        let cell_or_free = &mut internal.vec[idx];
        match cell_or_free {
            CellOrFree::Cell(cell) if (!use_gen || cell.gen == gen) => {
                if cell.locked {
                    return Err(AccessError::CellAlreadyBeingVisited(idx));
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
    /// u32_prison.visit_idx(key_0, |mut_ref_42| {
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
    pub fn visit_many<F: FnMut(&mut[&mut T])>(&self, cell_indices: &[usize], mut operation: F) -> Result<(), AccessError> {
        let internal = self.internal();
        let mut vals = Vec::new();
        let mut locks = Vec::new();
        let mut ret_value = Ok(());
        for idx in cell_indices {
            if *idx >= self.len() {
                ret_value = Err(AccessError::IndexOutOfRange(*idx));
                break;
            }
            let cell = &mut self.internal().vec[*idx];
            if cell.locked {
                ret_value = Err(AccessError::CellAlreadyBeingVisited(*idx));
                break;
            }
            cell.locked = true;
            locks.push(&mut cell.locked);
            vals.push(&mut cell.val);
        }
        if ret_value.is_ok() {
            internal.visit_count += 1;
            operation(vals.as_mut_slice());
            internal.visit_count -= 1;
        }
        for lock in locks {
            *lock = false;
        }
        return ret_value;
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
    pub fn visit_slice<R, F>(&self, range: R, operation: F) -> Result<(), AccessError>
    where
    R: RangeBounds<usize>,
    F:  FnMut(&mut[&mut T]) {
        let (start, end) = extract_true_start_end(range, self.len());
        let indices: Vec<usize> = (start..end).collect();
        println!("{:?}", indices); //DEBUG
        return self.visit_many(&indices, operation)
    }

    #[doc(hidden)]
    fn visit_with_index<F: FnMut(usize, &mut T)>(&self, cell_index: usize, mut operation: F) -> Result<(), AccessError> {
        let internal = self.internal();
        if cell_index >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(cell_index));
        }
        let cell = &mut internal.vec[cell_index];
        if cell.locked {
            return Err(AccessError::CellAlreadyBeingVisited(cell_index));
        }
        internal.visit_count += 1;
        cell.locked = true;
        operation(cell_index, &mut cell.val);
        cell.locked = false;
        internal.visit_count -= 1;
        return Ok(());
    }

    /// Visit every index in the [Prison] once, running the supplied closure on each of them individually.
    /// The closure takes the index *and* the value of the current cell being accessed
    /// to allow differentiation of each execution and help with accessing other indexes 
    /// relative to the current one.
    /// 
    /// (Note the idx is a plain `usize`, not a reference)
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
    /// u32_prison.visit_each(|idx, val| {
    ///     // check if index is odd
    ///     if idx % 2 == 1 {
    ///         // add one to value
    ///         *val += 1
    ///     }
    /// });
    /// # }
    /// ```
    /// Just like [Prison::visit()], any particular cell can only be visited once,
    /// but as long as the cells requested don't overlap you may make nested
    /// `visit()`-family calls. Since `visit_each()` only visits each cell
    /// individually, any other index other than the current one can be
    /// accessed during the supplied closure
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_each(|idx, val| {
    ///     // check if a valid index exist before this one
    ///     if idx > 0 {
    ///         assert!(u32_prison.visit(idx-1, |last_val| {}).is_ok());
    ///     }
    /// });
    /// # }
    /// ```
    /// Changing the length of the [Prison] before every cell has been visited
    /// will also change the number of visits that occur.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// let mut visit_count = 0;
    /// u32_prison.visit_each(|idx, val| {
    ///     visit_count += 1;
    ///     u32_prison.pop();
    /// });
    /// assert_eq!(visit_count, 2);
    /// # }
    /// ```
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::CellAlreadyBeingVisited(usize)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_each(|idx, val| {
    ///     assert!(u32_prison.visit(idx, |same_val| {}).is_err());
    /// });
    /// # }
    /// ```
    pub fn visit_each<F: FnMut(usize, &mut T)>(&self, mut operation: F) -> Result<(), AccessError> {
        for i in 0..self.len() {
            if i >= self.len() {
                break;
            }
            self.visit_with_index(i, &mut operation)?;
        }
        return Ok(())
    }

    /// Visit every index within the supplied range in the [Prison] once, running the
    /// supplied closure on each of them individually. The closure takes the index *and*
    /// the value of the current cell being accessed to allow differentiation of each
    /// execution and help with accessing other indexes relative to the current one.
    /// 
    /// (Note the idx is a plain `usize`, not a reference)
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
    /// u32_prison.visit_each_in_range(1..4, |idx, val| {
    ///     assert!(*val >= 43 && *val <= 45);
    /// });
    /// # }
    /// ```
    /// Just like [Prison::visit()], any particular cell can only be visited once,
    /// but as long as the cells requested don't overlap you may make nested
    /// `visit()`-family calls. Since `visit_each()` only visits each cell
    /// individually, any other index other than the current one can be
    /// accessed during the supplied closure
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_each_in_range(1.., |idx, val| {
    ///     assert!(u32_prison.visit(idx-1, |last_val| {}).is_ok());
    /// });
    /// # }
    /// ```
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::CellAlreadyBeingVisited(usize)], and attempting to visit an
    /// index that is out of range returns an [AccessError::IndexOutOfRange(usize)]
    /// without running on any of the indexes that may possibly be good
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.push(42);
    /// u32_prison.push(43);
    /// u32_prison.push(44);
    /// u32_prison.push(45);
    /// u32_prison.visit_each_in_range(.., |idx, val| {
    ///     assert!(u32_prison.visit(idx, |same_val| {}).is_err());
    /// });
    /// assert!(u32_prison.visit_each_in_range(0..10, |idx, val| { }).is_err());
    /// # }
    /// ```
    pub fn visit_each_in_range<R, F>(&self, range: R, mut operation: F) -> Result<(), AccessError>
    where
    R:  RangeBounds<usize>,
    F:  FnMut(usize, &mut T) {
        let (start, end) = extract_true_start_end(range, self.len());
        if start >= self.len() {
            return Err(AccessError::IndexOutOfRange(start));
        }
        if end >= self.len() {
            return Err(AccessError::IndexOutOfRange(end));
        }
        for i in start..end {
            if let Err(err) = self.visit_with_index(i, &mut operation) {
                return Err(err);
            }
        }
        return Ok(())
    }

    
}