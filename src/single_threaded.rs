#[cfg(not(feature = "no_std"))]
use std::{ops::{RangeBounds, Deref, DerefMut, Index, IndexMut}, cell::UnsafeCell, mem};

#[cfg(feature = "no_std")]
use core::{ops::{RangeBounds, Deref, DerefMut, Index, IndexMut}, cell::UnsafeCell, mem};

use crate::{AccessError, CellKey, extract_true_start_end};

macro_rules! internal {
    ($p:ident) => {
        unsafe {&mut *$p.internal.get()}
    };
}

/// The single-threaded implementation of [Prison<T>]
/// 
/// This struct uses an underlying [Vec<T>] to store data, but provides full interior mutability
/// for each of its elements. It primarily acts like a Generational Arena using [CellKey]'s to index
/// into the vector, but allows accessing elements with only a plain [usize] as well.
/// 
/// It does this by using [UnsafeCell] to wrap its internals, simple [bool] locks,
/// and a master [usize] counter that are used to determine what cells (indexes) are currently
/// being accessed to prevent violating Rust's memory management rules (to the best of it's ability).
/// Each element also has a [usize] `generation` counter to determine if the value being requested
/// was created in the same context it is being requested in.
/// 
/// Removing elements does not shift all elements that come after it like a normal [Vec]. Instead,
/// it marks the element as `free` meaning the value was deleted or removed. Subsequent inserts into
/// the [Prison] will insert values into free spaces before they consider extending the [Vec],
/// minimizing reallocations when possible.
/// 
/// See the crate-level documentation or individual methods for more info
#[derive(Debug)]
pub struct Prison<T> {
    internal: UnsafeCell<PrisonInternal<T>>,
}

/**************************
 *    PUBLIC INTERFACE
 **************************/

impl<T> Prison<T> {

    /// Create a new [Prison<T>] with the default allocation strategy ([Vec::new()])
    /// 
    /// Because re-allocating the internal [Vec] comes with many restrictions when
    /// `visit()`-ing elements, it is recommended to use [Prison::with_capacity()]
    /// with a suitable best-guess starting value rather than [Prison::new()]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() {
    /// let my_prison: Prison<u32> = Prison::new();
    /// assert!(my_prison.vec_cap() < 100)
    /// # }
    /// ```
    #[inline(always)]
    pub fn new() -> Self {
        return Self { 
            internal: UnsafeCell::new(PrisonInternal { visit_count: 0, free: 0, gen: 0, next_free: NO_FREE, vec: Vec::new() })
        };
    }

    /// Create a new [Prison<T>] with a specific starting capacity ([Vec::with_capacity()])
    /// 
    /// Because re-allocating the internal [Vec] comes with many restrictions when
    /// `visit()`-ing elements, it is recommended to use [Prison::with_capacity()]
    /// with a suitable best-guess starting value rather than [Prison::new()]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() {
    /// let my_prison: Prison<u32> = Prison::with_capacity(1000);
    /// assert!(my_prison.vec_cap() == 1000)
    /// # }
    /// ```
    #[inline(always)]
    pub fn with_capacity(size: usize) -> Self {
        return Self { 
            internal: UnsafeCell::new(PrisonInternal { visit_count: 0, free: 0, gen: 0, next_free: NO_FREE, vec: Vec::with_capacity(size) })
        };
    }

    /// Return the length of the underlying [Vec]
    /// 
    /// Because a [Prison] may have values that are `free` or `deleted` that are still counted
    /// withing the `length` of the [Vec], this value should not be used to determine how many
    /// *valid* elements exist in the [Prison]
    #[inline(always)]
    pub fn vec_len(&self) -> usize {
        return internal!(self).vec.len();
    }

    /// Return the capacity of the underlying [Vec]
    /// 
    /// Capacity refers to the number of total spaces in memory reserved for the [Vec]
    /// 
    /// Because a [Prison] may have values that are `free` or `deleted` that are *not* counted
    /// withing the `capacity` of the [Vec], this value should not be used to determine how many
    /// *empty* spots exist to add elements into the [Prison]
    #[inline(always)]
    pub fn vec_cap(&self) -> usize {
        return internal!(self).vec.capacity();
    }

    /// Return the number of spaces available for elements to be added to the [Prison]
    /// without reallocating more memory.
    #[inline(always)]
    pub fn num_free(&self) -> usize {
        let internal = internal!(self);
        return internal.free + internal.vec.capacity() - internal.vec.len();
    }

    /// Return the number of spaces currently occupied by valid elements in the [Prison]
    #[inline(always)]
    pub fn num_used(&self) -> usize {
        let internal = internal!(self);
        return internal.vec.len() - internal.free;
    }

    /// Return the ratio of used space to total space in the [Prison]
    /// 
    /// 0.0 = 0% used, 1.0 = 100% used
    pub fn density(&self) -> f32 {
        let internal = internal!(self);
        let used = internal.vec.len() - internal.free;
        let cap = internal.vec.capacity();
        return (used as f32) / (cap as f32);
    }

    /// Insert a value into the [Prison] and recieve a CellKey that can be used to 
    /// reference it in the future
    /// 
    /// As long as there is sufficient free cells or vector capacity to do so,
    /// you may `insert()` to the [Prison] while in the middle of any `visit()`
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
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
    /// However, if the [Prison] is at maxumum capacity, attempting to `insert()`
    /// during a visit will cause the operation to fail and a [AccessError::InsertAtMaxCapacityWhileVisiting]
    /// to be returned
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
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

    /// Insert a value into the [Prison] at the specified index and recieve a 
    /// CellKey that can be used to reference it in the future
    /// 
    /// The index *must* be within range of the underlying [Vec] *AND* must reference
    /// a space tagged as free/deleted.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(10);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// let key_1 = string_prison.insert(String::from("World!"))?;
    /// string_prison.remove(key_1)?;
    /// let key_1 = string_prison.insert_at(1, String::from("Rust!!"))?;
    /// string_prison.visit_many(&[key_0, key_1], |vals| {
    ///     let hello_world = format!("{}{}", vals[0], vals[1]);
    ///     assert_eq!(hello_world, "Hello, Rust!!");
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// If the index is out of range the function will return an [AccessError::IndexOutOfRange(idx)],
    /// and if the index is not free/deleted, it will return an [AccessError::IndexIsNotFree(idx)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(10);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// let key_1 = string_prison.insert(String::from("World!"))?;
    /// assert!(string_prison.insert_at(1, String::from("Rust!!")).is_err());
    /// assert!(string_prison.insert_at(10, String::from("Oops...")).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn insert_at(&self, idx: usize, value: T) -> Result<CellKey, AccessError> {
        self.insert_internal(idx, true, false, value)
    }

    /// Insert or overwrite a value into the [Prison] at the specified index and recieve a 
    /// CellKey that can be used to reference it in the future
    /// 
    /// Similar to [Prison::insert_at()] but does not require the space be marked as free.
    /// 
    /// Note: Overwriting a value that isn't marked as free will invalidate any [CellKey]
    /// that could have been used to reference it and cause a lookup using the old
    /// key(s) to return a [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(10);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// let key_1_a = string_prison.insert(String::from("World!"))?;
    /// // string_prison.remove(key_1)?; // removal not needed
    /// let key_1_b = string_prison.overwrite(1, String::from("Rust!!"))?;
    /// string_prison.visit_many(&[key_0, key_1_b], |vals| {
    ///     let hello_world = format!("{}{}", vals[0], vals[1]);
    ///     assert_eq!(hello_world, "Hello, Rust!!");
    ///     Ok(())
    /// });
    /// assert!(string_prison.visit(key_1_a, |deleted_val| Ok(())).is_err());
    /// assert!(string_prison.overwrite(10, String::from("Oops...")).is_err());
    /// # Ok(())
    /// # }
    #[inline(always)]
    pub fn overwrite(&self, idx: usize, value: T) -> Result<CellKey, AccessError> {
        self.insert_internal(idx, true, true, value)
    }

    /// Remove and return the element indexed by the provided [CellKey]
    /// 
    /// As long as the element isn't being visited, you can `.remove()` it,
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
    /// However, if the element *is* being visited, `.remove()` will return an
    /// [AccessError::RemoveWhileIndexBeingVisited(idx)] error with the index in question
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
    /// value that is passed into a closure you provide.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
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
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to visit a value that was deleted (generation doesnt match) returns an 
    /// [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(69)?;
    /// u32_prison.remove(key_1)?;
    /// u32_prison.visit(key_0, |mut_ref_42| {
    ///     assert!(u32_prison.visit(key_0, |mut_ref_42_again| Ok(())).is_err());
    ///     assert!(u32_prison.visit(CellKey::from_raw_parts(5, 5), |doesnt_exist| Ok(())).is_err());
    ///     assert!(u32_prison.visit(key_1, |deleted| Ok(())).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// ### Example
    /// ```compile_fail
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
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
    /// # fn main() -> Result<(), AccessError> {
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
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to visit a value that was deleted returns an 
    /// [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(69)?;
    /// u32_prison.remove_idx(1)?;
    /// u32_prison.visit_idx(0, |mut_ref_42| {
    ///     assert!(u32_prison.visit_idx(0, |mut_ref_42_again| Ok(())).is_err());
    ///     assert!(u32_prison.visit_idx(5, |doesnt_exist| Ok(())).is_err());
    ///     assert!(u32_prison.visit_idx(1, |deleted| Ok(())).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// ### Example
    /// ```compile_fail
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
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
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(43)?;
    /// let key_2 = u32_prison.insert(44)?;
    /// let key_3 = u32_prison.insert(45)?;
    /// u32_prison.visit_many(&[key_3, key_2, key_1, key_0], |first_four_reversed| {
    ///     assert_eq!(*first_four_reversed[0], 45);
    ///     assert_eq!(*first_four_reversed[1], 44);
    ///     assert_eq!(*first_four_reversed[2], 43);
    ///     assert_eq!(*first_four_reversed[3], 42);
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// Just like `.visit()`, any particular cell can only be visited once,
    /// but as long as the cells requested don't overlap you may make nested
    /// `visit()`-family calls
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(43)?;
    /// let key_2 = u32_prison.insert(44)?;
    /// let key_3 = u32_prison.insert(45)?;
    /// u32_prison.visit_many(&[key_0, key_2], |evens| {
    ///     u32_prison.visit_many(&[key_1, key_3], |odds| {
    ///         assert_eq!(*evens[1], 44);
    ///         assert_eq!(*odds[1], 45);
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to visit a set of [CellKey]s with even *one* element free/deleted
    /// will return an [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(43)?;
    /// let key_2 = u32_prison.insert(44)?;
    /// let key_3 = u32_prison.insert(45)?;
    /// u32_prison.remove(key_1)?;
    /// let key_4 = CellKey::from_raw_parts(4, 0);
    /// assert!(u32_prison.visit_many(&[key_0, key_0], |double_key_zero| Ok(())).is_err());
    /// assert!(u32_prison.visit_many(&[key_1, key_2, key_3], |key_1_removed| Ok(())).is_err());
    /// assert!(u32_prison.visit_many(&[key_2, key_3, key_4], |key_4_invalid| Ok(())).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn visit_many<F>(&self, keys: &[CellKey], mut operation: F) -> Result<(), AccessError> 
    where F: FnMut(&[&mut T]) -> Result<(), AccessError> {
        self.visit_many_internal(keys, true, |_, vals| operation(vals))
    }

    /// Visit many values in the [Prison] at the same time, obtaining a mutable reference
    /// to all of them in the same closure and in the same order they were requested.
    /// 
    /// Similar to [Prison::visit_many()] except the generation tag on the elements are ignored
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.visit_many_idx(&[3, 2, 1, 0], |first_four_reversed| {
    ///     assert_eq!(*first_four_reversed[0], 45);
    ///     assert_eq!(*first_four_reversed[1], 44);
    ///     assert_eq!(*first_four_reversed[2], 43);
    ///     assert_eq!(*first_four_reversed[3], 42);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Just like `.visit_idx()`, any particular cell can only be visited once,
    /// but as long as the cells requested don't overlap you may make nested
    /// `visit()`-family calls
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.visit_many_idx(&[0, 2], |evens| {
    ///     u32_prison.visit_many_idx(&[1, 3], |odds| {
    ///         assert_eq!(*evens[1], 44);
    ///         assert_eq!(*odds[1], 45);
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to visit a set of indexes with even *one* element free/deleted
    /// will return an [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.remove_idx(1)?;
    /// assert!(u32_prison.visit_many_idx(&[0, 0], |double_idx_zero| Ok(())).is_err());
    /// assert!(u32_prison.visit_many_idx(&[1, 2, 3], |idx_1_removed| Ok(())).is_err());
    /// assert!(u32_prison.visit_many_idx(&[2, 3, 4], |idx_4_invalid| Ok(())).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn visit_many_idx<F>(&self, indexes: &[usize], mut operation: F) -> Result<(), AccessError> 
    where F: FnMut(&[&mut T]) -> Result<(), AccessError> {
        let keys: Vec<CellKey> = indexes.iter().map(|idx| CellKey{ idx: *idx, gen: 0}).collect();
        self.visit_many_internal(&keys, false, |_, vals| operation(vals))
    }

    /// Visit a slice of values in the [Prison] at the same time, obtaining a mutable reference
    /// to all of them in the same closure.
    /// 
    /// Internally this is identical to passing [Prison::visit_many_idx()] a list of all
    /// indexes in the slice range.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.insert(46)?;
    /// u32_prison.visit_slice(2..5, |last_three| {
    ///     assert_eq!(*last_three[0], 44);
    ///     assert_eq!(*last_three[1], 45);
    ///     assert_eq!(*last_three[2], 46);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Any standard [Range<usize>](std::ops::Range) notation is allowed as the first paramater,
    /// but care must be taken because it is not guaranteed every index within range is a valid
    /// value or is not being accessed by any other method
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.insert(46)?;
    /// assert!(u32_prison.visit_slice(2..5,  |last_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice(2..=4, |also_last_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice(2..,   |again_last_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice(..3,   |first_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice(..=3,  |first_four| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice(..,    |all| Ok(())).is_ok());
    /// u32_prison.remove_idx(2)?;
    /// assert!(u32_prison.visit_slice(..,    |all| Ok(())).is_err());
    /// # Ok(())
    /// # }
    /// ```
    /// Just like [Prison::visit_many_idx()], any particular cell can only be visited once,
    /// but as long as the cells requested don't overlap you may make nested
    /// `visit()`-family calls
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.visit_slice(..2, |first_half| {
    ///     u32_prison.visit_slice(2.., |second_half| {
    ///         assert_eq!(*first_half[1], 43);
    ///         assert_eq!(*second_half[0], 44);
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Attempting to visit the same cell twice will fail, returning an
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to visit an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to visit a slice with even *one* element free/deleted
    /// will return an [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.visit_slice(.., |all| {
    ///     assert!(u32_prison.visit_slice(0..1, |second_visit_to_first_idx| Ok(())).is_err());
    ///     Ok(())
    /// });
    /// assert!(u32_prison.visit_slice(0..10, |invalid_idx| Ok(())).is_err());
    /// u32_prison.remove_idx(1)?;
    /// assert!(u32_prison.visit_slice(.., |idx_1_removed| Ok(())).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn visit_slice<R, F>(&self, range: R, mut operation: F) -> Result<(), AccessError>
    where
    R: RangeBounds<usize>,
    F:  FnMut(&[&mut T]) -> Result<(), AccessError> {
        let (start, end) = extract_true_start_end(range, self.vec_len());
        let keys: Vec<CellKey> = (start..end).map(|idx| CellKey {idx, gen: 0}).collect();
        self.visit_many_internal(&keys, false, |_, vals| operation(vals))
    }

    /// Return an [EscortedValue] that locks the element and wraps it in guarding data that automatically
    /// unlocks it when it goes out of range.
    /// 
    /// Data within an [EscortedValue] can be accessed and mutated by dereferencing it. As long as the [EscortedValue]
    /// remains in scope, the element where it's value resides in the [Prison] will remain locked and unable to be accessed.
    /// You can manually drop the [EscortedValue] out of scope by calling the `unescort()` method on it
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// let key_0 = prison.insert(10)?;
    /// let mut esc_0 = prison.escort(key_0)?;
    /// assert_eq!(*esc_0, 10);
    /// *esc_0 = 20;
    /// esc_0.unescort();
    /// prison.visit(key_0, |val_0| {
    ///     assert_eq!(*val_0, 20);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Attempting to escort the same cell twice will fail, returning an
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to escort an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to escort a value that was free/deleted
    /// will return an [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::with_capacity(2);
    /// let key_0 = prison.insert(10)?;
    /// let key_1_a = prison.insert(20)?;
    /// let key_out_of_bounds = CellKey::from_raw_parts(10, 0);
    /// prison.remove(key_1_a)?;
    /// let key_1_b = prison.insert(30)?;
    /// let mut esc_0 = prison.escort(key_0)?;
    /// assert!(prison.escort(key_0).is_err());
    /// assert!(prison.escort(key_out_of_bounds).is_err());
    /// assert!(prison.escort(key_1_a).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn escort<'a>(&'a self, key: CellKey) -> Result<EscortedValue<'a, T>, AccessError> {
        return self.escort_internal(key.idx, key.gen, true)
    }

    /// Return an [EscortedValue] that locks the element and wraps it in guarding data that automatically
    /// unlocks it when it goes out of range.
    /// 
    /// Similar to [Prison::escort()], but ignores the generation counter
    /// 
    /// Data within an [EscortedValue] can be accessed and mutated by dereferencing it. As long as the [EscortedValue]
    /// remains in scope, the element where it's value resides in the [Prison] will remain locked and unable to be accessed.
    /// You can manually drop the [EscortedValue] out of scope by calling the `unescort()` method on it
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// let mut esc_0 = prison.escort_idx(0)?;
    /// assert_eq!(*esc_0, 10);
    /// *esc_0 = 20;
    /// esc_0.unescort();
    /// prison.visit_idx(0, |val_0| {
    ///     assert_eq!(*val_0, 20);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Attempting to escort the same cell twice will fail, returning an
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to escort an index
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to escort a value that was free/deleted
    /// will return an [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::with_capacity(2);
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.remove_idx(1)?;
    /// let mut esc_0 = prison.escort_idx(0)?;
    /// assert!(prison.escort_idx(0).is_err());
    /// assert!(prison.escort_idx(10).is_err());
    /// assert!(prison.escort_idx(1).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn escort_idx<'a>(&'a self, idx: usize) -> Result<EscortedValue<'a, T>, AccessError> {
        return self.escort_internal(idx, 0, false)
    }

    /// Return an [EscortedSlice] that locks the elements and wraps them in guarding data that automatically
    /// unlocks them all when it goes out of range.
    /// 
    /// Data within an [EscortedSlice] can be accessed and mutated by indexing into it. As long as the [EscortedSlice]
    /// remains in scope, the elements where it's values reside in the [Prison] will remain locked and unable to be accessed.
    /// You can manually drop the [EscortedSlice] out of scope by calling the `unescort()` method on it
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// let key_0 = prison.insert(10)?;
    /// let key_1 = prison.insert(20)?;
    /// let key_2 = prison.insert(30)?;
    /// let mut esc_0_1_2 = prison.escort_many(&[key_0, key_1, key_2])?;
    /// assert_eq!(esc_0_1_2[0], 10);
    /// esc_0_1_2[0] = 20;
    /// esc_0_1_2.unescort();
    /// prison.visit_many(&[key_0, key_1, key_2], |vals_0_1_2| {
    ///     assert_eq!(*vals_0_1_2[0], 20);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Attempting to escort the same element twice will fail, returning an
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to escort an element
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to escort an element that was free/deleted
    /// will return an [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// let key_0 = prison.insert(10)?;
    /// let key_1 = prison.insert(20)?;
    /// let key_2 = prison.insert(30)?;
    /// let key_4_a = prison.insert(40)?;
    /// prison.remove(key_4_a)?;
    /// let key_4_b = prison.insert(44)?;
    /// let key_out_of_bounds = CellKey::from_raw_parts(10, 1);
    /// let mut esc_0_1_2 = prison.escort_many(&[key_0, key_1, key_2])?;
    /// assert!(prison.escort_many(&[key_0, key_1, key_2, key_4_b]).is_err());
    /// assert!(prison.escort_many(&[key_out_of_bounds]).is_err());
    /// assert!(prison.escort_many(&[key_4_a]).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn escort_many<'a>(&'a self, keys: &[CellKey]) -> Result<EscortedSlice<'a, T>, AccessError> {
        return self.escort_many_internal(keys, true)
    }

    /// Return an [EscortedSlice] that locks the elements and wraps them in guarding data that automatically
    /// unlocks them all when it goes out of range.
    /// 
    /// Similar to [Prison::escort_many()] but disregards the generation counter
    /// 
    /// Data within an [EscortedSlice] can be accessed and mutated by indexing into it. As long as the [EscortedSlice]
    /// remains in scope, the elements where it's values reside in the [Prison] will remain locked and unable to be accessed.
    /// You can manually drop the [EscortedSlice] out of scope by calling the `unescort()` method on it
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.insert(30)?;
    /// let mut esc_0_1_2 = prison.escort_many_idx(&[0, 1, 2])?;
    /// assert_eq!(esc_0_1_2[0], 10);
    /// esc_0_1_2[0] = 20;
    /// esc_0_1_2.unescort();
    /// prison.visit_many_idx(&[0, 1, 2], |vals_0_1_2| {
    ///     assert_eq!(*vals_0_1_2[0], 20);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Attempting to escort the same element twice will fail, returning an
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to escort an element
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to escort an element that was free/deleted
    /// will return an [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.insert(30)?;
    /// prison.insert(40)?;
    /// prison.remove_idx(3)?;
    /// let mut esc_0_1_2 = prison.escort_many_idx(&[0, 1, 2])?;
    /// assert!(prison.escort_many_idx(&[0, 1, 2]).is_err());
    /// assert!(prison.escort_many_idx(&[10]).is_err());
    /// assert!(prison.escort_many_idx(&[3]).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn escort_many_idx<'a>(&'a self, indexes: &[usize]) -> Result<EscortedSlice<'a, T>, AccessError> {
        let keys: Vec<CellKey> = indexes.iter().map(|idx| CellKey{ idx: *idx, gen: 0}).collect();
        return self.escort_many_internal(&keys, false)
    }

    /// Return an [EscortedSlice] that locks the elements and wraps them in guarding data that automatically
    /// unlocks them all when it goes out of range.
    /// 
    /// Internally this is identical to calling [Prison::escort_many_idx()] with a list of consecutive
    /// indexes.
    /// 
    /// Data within an [EscortedSlice] can be accessed and mutated by indexing into it. As long as the [EscortedSlice]
    /// remains in scope, the elements where it's values reside in the [Prison] will remain locked and unable to be accessed.
    /// You can manually drop the [EscortedSlice] out of scope by calling the `unescort()` method on it
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.insert(30)?;
    /// let mut esc_all = prison.escort_slice(..)?;
    /// assert_eq!(esc_all[0], 10);
    /// esc_all[0] = 20;
    /// esc_all.unescort();
    /// prison.visit_slice(.., |all_vals| {
    ///     assert_eq!(*all_vals[0], 20);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Any standard [Range<usize>](std::ops::Range) notation is allowed,
    /// but care must be taken because it is not guaranteed every index within range is a valid
    /// value or is not being accessed by any other method
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.insert(46)?;
    /// let mut esc = u32_prison.escort_slice(2..5)?;
    /// esc.unescort();
    /// esc = u32_prison.escort_slice(2..=4)?;
    /// esc.unescort();
    /// esc = u32_prison.escort_slice(2..)?;
    /// esc.unescort();
    /// esc = u32_prison.escort_slice(..3)?;
    /// esc.unescort();
    /// esc = u32_prison.escort_slice(..=3)?;
    /// esc.unescort();
    /// esc = u32_prison.escort_slice(..)?;
    /// esc.unescort();
    /// u32_prison.remove_idx(2)?;
    /// assert!(u32_prison.escort_slice(..).is_err());
    /// # Ok(())
    /// # }
    /// ```
    /// Attempting to escort the same element twice will fail, returning an
    /// [AccessError::IndexAlreadyBeingVisited(idx)], attempting to escort an element
    /// that is out of range returns an [AccessError::IndexOutOfRange(idx)],
    /// and attempting to escort an element that was free/deleted
    /// will return an [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.insert(30)?;
    /// prison.insert(40)?;
    /// prison.insert(50)?;
    /// let esc_first_two = prison.escort_slice(0..2)?;
    /// assert!(prison.escort_slice(0..3).is_err());
    /// assert!(prison.escort_slice(2..10).is_err());
    /// prison.remove_idx(3)?;
    /// assert!(prison.escort_slice(..).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn escort_slice<'a, R>(&'a self, range: R) -> Result<EscortedSlice<'a, T>, AccessError>
    where R: RangeBounds<usize> {
        let (start, end) = extract_true_start_end(range, self.vec_len());
        let keys: Vec<CellKey> = (start..end).map(|idx| CellKey {idx, gen: 0}).collect();
        return self.escort_many_internal(&keys, false)
    }
}

/// Struct representing a value that has been allowed to leave the [Prison] temporarily,
/// and is escorted by guarding data to prevent it from leaking or never unlocking
/// 
/// The element is locked and the [Prison] `visit_count`is incremented when the [EscortedValue] is created,
/// and it has an implementation of [Drop] that decrements the [Prison] `visit_count` by one and
/// unlocks the element for further use, meaning the value remains locked *only* as long as its 
/// [EscortedValue] remains in scope
/// 
/// You may also manually return the value to the [Prison] calling `unescort()` on it
/// 
/// You can obtain a [EscortedValue] by calling `escort()` or `escort_idx()` on a [Prison], and accessing the
/// value it wraps is a simple matter of dereferencing it
/// ### Example
/// ```rust
/// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
/// # fn main() -> Result<(), AccessError> {
/// let prison: Prison<u32> = Prison::new();
/// let key_0 = prison.insert(10)?;
/// let mut esc_0 = prison.escort(key_0)?;
/// assert_eq!(*esc_0, 10);
/// *esc_0 = 20;
/// esc_0.unescort();
/// prison.visit(key_0, |val_0| {
///     assert_eq!(*val_0, 20);
///     Ok(())
/// });
/// # Ok(())
/// # }
/// ```
pub struct EscortedValue<'a, T> {
    lock: &'a mut bool,
    visits: &'a mut usize,
    value: &'a mut T,
}

impl<'a, T> EscortedValue<'a, T> {
    /// Manually end this value's temporary escorted absence from the [Prison]
    /// 
    /// This method simply takes ownership of the [EscortedValue] and immediately lets it go out of scope,
    /// causing it's `drop()` method to be called and unlocking it's corresponding element in the [Prison]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// let esc_0 = prison.escort_idx(0)?;
    /// // index 0 CANNOT be accessed here because it is being escorted outside the prison
    /// assert!(prison.visit_idx(0, |ref_0| Ok(())).is_err());
    /// esc_0.unescort();
    /// // index 0 CAN be accessed here because it was returned to the prison
    /// assert!(prison.visit_idx(0, |ref_0| Ok(())).is_ok());
    /// # Ok(())
    /// # }
    /// ```
    pub fn unescort(self) {}
}

impl<'a, T> Drop for EscortedValue<'a, T> {
    fn drop(&mut self) {
        prison_unlock_one_internal(self.lock, self.visits)
    }
}

impl<'a, T> Deref for EscortedValue<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<'a, T> DerefMut for EscortedValue<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
    }
}

impl<'a, T> AsRef<T> for EscortedValue<'a, T>
{
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

impl<'a, T> AsMut<T> for EscortedValue<'a, T>
{
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut()
    }
}

/// Struct representing a slice of values that have been allowed to leave the [Prison] temporarily,
/// and are escorted by guarding data to prevent them from leaking or never unlocking
/// 
/// The elements are all locked and the [Prison] `visit_count`is incremented when the [EscortedSlice] is created,
/// and it has an implementation of [Drop] that decrements the [Prison] `visit_count` by one and
/// unlocks all the elements for further use, meaning the values remain locked *only* as long as its 
/// [EscortedSlice] remains in scope
/// 
/// You may also manually return the values to the [Prison] calling `unescort()` on it
/// 
/// You can obtain a [EscortedSlice] by calling `escort_many()`, `escort_many_idx()` or `escort_slice()`
/// on a [Prison], and accessing the values it wraps is a simple matter of indexing into it
/// ### Example
/// ```rust
/// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedSlice}};
/// # fn main() -> Result<(), AccessError> {
/// let prison: Prison<u32> = Prison::new();
/// let key_0 = prison.insert(10)?;
/// let key_1 = prison.insert(20)?;
/// let key_2 = prison.insert(30)?;
/// let mut esc_0_1_2 = prison.escort_many(&[key_0, key_1, key_2])?;
/// assert_eq!(esc_0_1_2[1], 20);
/// esc_0_1_2[1] = 42;
/// esc_0_1_2.unescort();
/// prison.visit(key_1, |val_1| {
///     assert_eq!(*val_1, 42);
///     Ok(())
/// });
/// # Ok(())
/// # }
/// ```
pub struct EscortedSlice<'a, T> {
    visits: &'a mut usize,
    locks: Vec<&'a mut bool>,
    values: Vec<&'a mut T>,
}

impl<'a, T> EscortedSlice<'a, T> {
    /// Manually end this slice of values' temporary escorted absence from the [Prison]
    /// 
    /// This method simply takes ownership of the [EscortedSlice] and immediately lets it go out of scope,
    /// causing it's `drop()` method to be called and unlocking all it's corresponding elements in the [Prison]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.insert(30)?;
    /// let esc_0_1_2 = prison.escort_many_idx(&[0, 1, 2])?;
    /// // indexes CANNOT be accessed here because they are being escorted outside of the prison
    /// assert!(prison.visit_many_idx(&[0, 1, 2], |vals| Ok(())).is_err());
    /// esc_0_1_2.unescort();
    /// // indexes CAN be accessed here because they were returned and unlocked
    /// assert!(prison.visit_many_idx(&[0, 1, 2], |vals| Ok(())).is_ok());
    /// # Ok(())
    /// # }
    /// ```
    pub fn unescort(self) {}
}

impl<'a, T> Drop for EscortedSlice<'a, T> {
    fn drop(&mut self) {
        prison_unlock_many_internal(&mut self.locks, self.visits)
    }
}

impl<'a, T> Index<usize> for EscortedSlice<'a, T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        self.values[index] // Is this witchcraft?
    }
}

impl<'a, T> IndexMut<usize> for EscortedSlice<'a, T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        self.values[index] // Is this witchcraft?
    }
}

/**************************
 * INTERNAL IMPLEMENTATIONS
 **************************/

const NO_FREE: usize = usize::MAX;
const MAX_CAP: usize = isize::MAX as usize;

#[doc(hidden)]
#[derive(Debug)]
struct PrisonInternal<T> {
    visit_count: usize,
    gen: usize,
    free: usize,
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
                internal.free -= 1;
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
        internal.free += 1;
        let old_cell = mem::replace(&mut internal.vec[idx], new_free);
        return if let CellOrFree::Cell(cell) = old_cell {
            Ok(cell.val)
        } else {
            Err(AccessError::ValueDeleted(idx, gen))
        }
    }

    #[doc(hidden)]
    fn lock_one_internal(&self, idx: usize, gen: usize, use_gen: bool) -> Result<(&mut bool, &mut T, &mut usize), AccessError> {
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
                return Ok((&mut cell.locked, &mut cell.val, &mut internal.visit_count))
            },
            _ => return Err(AccessError::ValueDeleted(idx, gen)),
        }
    }

    #[doc(hidden)]
    fn lock_many_internal(&self, cell_keys: &[CellKey], use_gens: bool) -> Result<(Vec<&mut bool>, Vec<usize>, Vec<&mut T>, &mut usize), AccessError> {
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
        internal.visit_count += 1;
        match ret_value {
            Ok(_) => {
                return Ok((locks, indices, vals, &mut internal.visit_count));
            },
            Err(acc_err) => {
                prison_unlock_many_internal(&mut locks, &mut internal.visit_count);
                return Err(acc_err);
            },
        }
    }

    #[doc(hidden)]
    fn visit_one_internal<FF>(&self, idx: usize, gen: usize, use_gen: bool, mut ff: FF) -> Result<(), AccessError>
    where FF: FnMut(usize, &mut T) -> Result<(), AccessError> {
        let (lock, val, visits) = self.lock_one_internal(idx, gen, use_gen)?;
        let res = ff(idx, val);
        prison_unlock_one_internal(lock, visits);
        return res;
    }

    #[doc(hidden)]
    fn visit_many_internal<FF>(&self, cell_keys: &[CellKey], use_gens: bool, mut ff: FF) -> Result<(), AccessError>
    where FF: FnMut(&[usize], &[&mut T]) -> Result<(), AccessError> {
        let (mut locks, indices, vals, visits) = self.lock_many_internal(cell_keys, use_gens)?;
        let result = ff(&indices, &vals);
        prison_unlock_many_internal(&mut locks, visits);
        return result;
    }

    #[doc(hidden)]
    fn escort_internal<'a>(&'a self, idx: usize, gen: usize, use_gen: bool,) -> Result<EscortedValue<'a, T>, AccessError> {
        let (lock, value, visits) = self.lock_one_internal(idx, gen, use_gen)?;
        return Ok(EscortedValue{ lock, visits, value });
    }

    #[doc(hidden)]
    fn escort_many_internal<'a>(&'a self, cell_keys: &[CellKey], use_gens: bool) -> Result<EscortedSlice<'a, T>, AccessError> {
        let (locks, _, values, visits) = self.lock_many_internal(cell_keys, use_gens)?;
        return Ok(EscortedSlice { visits, locks, values });
    }
}

#[doc(hidden)]
#[inline(always)]
fn prison_unlock_one_internal(lock: &mut bool, visits: &mut usize) {
    *lock = false;
    *visits -= 1;
}

#[doc(hidden)]
#[inline(always)]
fn prison_unlock_many_internal(locks: &mut [&mut bool], visits: &mut usize) {
    for lock in locks {
        **lock = false;
    }
    *visits -= 1;
}

/**************************
 *        TESTING
 **************************/

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Eq, PartialEq)]
    struct MyNoCopy(usize);

    impl MyNoCopy {
        fn val(&self) -> usize {
            self.0
        }
    }

    fn extract_usize(mnc: &MyNoCopy) -> usize {
        mnc.val()
    }

    #[allow(dead_code)]
    struct SizeEmptyPrisonCell(CellOrFree<()>); // Size 16, Align 8
    #[allow(dead_code)]
    struct SizeU8PrisonCell(CellOrFree<u8>); // Size 16, Align 8
    #[allow(dead_code)]
    struct SizeU16PrisonCell(CellOrFree<u16>); // Size 16, Align 8
    #[allow(dead_code)]
    struct Size3BPrisonCell(CellOrFree<(u8, u8, u8)>); // Size 16, Align 8
    #[allow(dead_code)]
    struct SizeU32PrisonCell(CellOrFree<u32>); // Size 16, Align 8
    #[allow(dead_code)]
    struct Size5BPrisonCell(CellOrFree<(u8, u8, u8, u8, u8)>); // Size 16, Align 8
    #[allow(dead_code)]
    struct Size6BPrisonCell(CellOrFree<(u8, u8, u8, u8, u8, u8)>); // Size 16, Align 8
    #[allow(dead_code)]
    struct Size7BPrisonCell(CellOrFree<(u8, u8, u8, u8, u8, u8, u8)>); // Size 16, Align 8
    #[allow(dead_code)]
    struct SizeU64PrisonCell(CellOrFree<u64>); // Size 24, Align 8
    #[allow(dead_code)]
    struct SizeU128PrisonCell(CellOrFree<u128>); // Size 32, Align 8
    
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
            internal!(prison).free = 1;
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

    #[test]
    #[allow(unused_variables)]
    fn escort_internal() {
        let prison: Prison<MyNoCopy> = Prison::with_capacity(2);
        let key_0 = prison.insert(MyNoCopy(0)).unwrap();
        let key_1 = prison.insert(MyNoCopy(1)).unwrap();
        let mut esc_0 = prison.escort_internal(key_0.idx, key_0.gen, true).unwrap();
        let mut esc_1 = prison.escort_internal(1, 0, false).unwrap();
        assert!(prison.escort_internal(key_0.idx, key_0.gen, true).is_err());
        assert!(prison.visit(key_1, |val_1| Ok(())).is_err());
        let extract_0 = extract_usize(&esc_0);
        let extract_1 = extract_usize(&esc_1);
        assert_eq!(extract_0, 0);
        assert_eq!(extract_1, 1);
        *esc_0 = MyNoCopy(42);
        *esc_1 = MyNoCopy(69);
        esc_0.unescort();
        esc_1.unescort();
        assert!(prison.visit(key_1, |val_1| Ok(())).is_ok());
        {
            let mut esc_0 = prison.escort_internal(0, 0, false).unwrap();
            let esc_1 = prison.escort_internal(key_1.idx, key_1.gen, true).unwrap();
            assert_eq!((*esc_0).0 + (*esc_1).0, 111);
            let mut_ref_0: &mut MyNoCopy = &mut *esc_0;
            *mut_ref_0 = MyNoCopy(52);
            assert_eq!((*esc_0).0 + (*esc_1).0, 121);
            let ref_1: & MyNoCopy = & *esc_1;
            assert_eq!((*ref_1).0, 69);
            assert!(prison.remove(key_0).is_err());
            assert!(prison.insert(MyNoCopy(99999)).is_err());
        }
        assert!(prison.visit_many(&[key_0, key_1], |vals| Ok(())).is_ok());
        assert!(prison.remove(key_0).is_ok());
        assert!(prison.insert(MyNoCopy(99999)).is_ok());
    }

    #[test]
    #[allow(unused_variables)]
    fn escort_many_internal() {
        let prison: Prison<MyNoCopy> = Prison::with_capacity(5);
        let key_0 = prison.insert(MyNoCopy(0)).unwrap();
        let key_1 = prison.insert(MyNoCopy(1)).unwrap();
        let key_2 = prison.insert(MyNoCopy(2)).unwrap();
        let key_3 = prison.insert(MyNoCopy(3)).unwrap();
        let key_4 = prison.insert(MyNoCopy(4)).unwrap();
        let mut esc_0_1_2 = prison.escort_many_internal(&[key_0, key_1, key_2], true).unwrap();
        let mut esc_3_4 = prison.escort_many_internal(&[key_3, key_4], false).unwrap();
        assert!(prison.escort_internal(key_0.idx, key_0.gen, true).is_err());
        assert!(prison.escort_many_internal(&[key_0, key_1, key_2, key_3, key_4], true).is_err());
        assert!(prison.visit(key_1, |val_1| Ok(())).is_err());
        let extract_0 = extract_usize(&esc_0_1_2[0]);
        let extract_1 = extract_usize(&esc_0_1_2[1]);
        assert_eq!(extract_0, 0);
        assert_eq!(extract_1, 1);
        esc_0_1_2[0] = MyNoCopy(42);
        esc_3_4[1] = MyNoCopy(69);
        esc_0_1_2.unescort();
        esc_3_4.unescort();
        assert!(prison.visit(key_1, |val_1| Ok(())).is_ok());
        {
            let mut esc_0_1_2 = prison.escort_many_internal(&[key_0, key_1, key_2], true).unwrap();
            let esc_3_4 = prison.escort_many_internal(&[key_3, key_4], false).unwrap();
            assert_eq!(esc_0_1_2[0].0 + esc_3_4[1].0, 111);
            let mut_ref_0: &mut MyNoCopy = &mut esc_0_1_2[0];
            *mut_ref_0 = MyNoCopy(52);
            assert_eq!(esc_0_1_2[0].0 + esc_3_4[1].0, 121);
            let ref_1: & MyNoCopy = &esc_3_4[1];
            assert_eq!((*ref_1).0, 69);
            assert!(prison.remove(key_0).is_err());
            assert!(prison.insert(MyNoCopy(99999)).is_err());
        }
        assert!(prison.visit_many(&[key_0, key_4], |vals| Ok(())).is_ok());
        assert!(prison.remove(key_0).is_ok());
        assert!(prison.insert(MyNoCopy(99999)).is_ok());
    }
}