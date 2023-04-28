use crate::{
    extract_true_start_end, internal, MaybeUninit, mem_replace, AccessError, Borrow, BorrowMut, CellKey, Debug, Deref,
    DerefMut, RangeBounds, UnsafeCell, major_malfunction, unreachable_unchecked
};

//REGION Misc Types
//STRUCT Refs
struct Refs {}
impl Refs {
    const MUT: usize = usize::MAX;
    const MAX_IMMUT: usize = Self::MUT - 1;
}

//STRUCT IdxD
#[allow(non_camel_case_types)]
struct IdxD {}
#[allow(dead_code)]
impl IdxD {
    const MAX_CAP: usize = usize::MAX >> 1;
    const MAX_GEN: usize = Self::MAX_CAP;
    const MAX_IDX: usize = Self::MAX_CAP - 1;
    const INVALID: usize = Self::MAX_CAP;
    const DISCRIMINANT_MASK: usize = Self::MAX_CAP + 1;
    const DISCRIMINANT_SHIFT: u32 = usize::BITS - 1;
    const VALUE_MASK: usize = Self::MAX_CAP;
    
    const fn val(val: usize) -> usize {
        val & Self::VALUE_MASK
    }

    const fn is_type_a(val: usize) -> bool {
        val & Self::DISCRIMINANT_MASK == 0
    }

    const fn is_type_b(val: usize) -> bool {
        val & Self::DISCRIMINANT_MASK == Self::DISCRIMINANT_MASK
    }

    const fn new_type_a(val: usize) -> usize {
        val & Self::VALUE_MASK
    }

    const fn new_type_b(val: usize) -> usize {
        (val & Self::VALUE_MASK) | Self::DISCRIMINANT_MASK
    }
}


//REGION Prison Public
//STRUCT Prison
/// The single-threaded implementation of [Prison]
///
/// This struct uses an underlying [Vec<T>] to store data, but provides full interior mutability
/// for each of its elements. It primarily acts like a Generational Arena using [CellKey]'s to index
/// into the vector, but allows accessing elements with only a plain [usize] as well.
///
/// It does this by using [UnsafeCell] to wrap its internals, a ref-counting [usize] on each element,
/// and a master [usize] access-counter that are used to determine what cells (indexes) are currently
/// being accessed to prevent violating Rust's memory management rules.
/// Each element also has a [usize] generation counter to determine if the value being requested
/// was created in the same context it is being requested in.
///
/// Removing elements does not shift all elements that come after it like a normal [Vec]. Instead,
/// it marks the element as "free", meaning the value was deleted or removed. Subsequent inserts into
/// the [Prison] will insert values into free spaces before they consider extending the [Vec],
/// minimizing reallocations when possible.
///
/// See the crate-level documentation or individual methods for more info
#[derive(Debug)]
pub struct Prison<T> {
    internal: UnsafeCell<PrisonMutable<T>>,
}

impl<T> Prison<T> {
    //FN Prison::new()
    /// Create a new [Prison] with the default allocation strategy ([Vec::new()])
    ///
    /// Because [Prison] accepts values that may or may not be implement [Copy], [Clone],
    /// or [Default] and because indexes are simply marked as "free" when their values are removed
    /// from the [Prison], a closure must be provided upon creation of a new prison
    /// that supplies it default values to replace the removed ones with safely ([mem::replace()])
    /// without running into double-frees or use-after-frees or resorting to things like
    /// [ManuallyDrop](std::mem::ManuallyDrop) or [MaybeUninit](std::mem::MaybeUninit)
    ///
    /// Because re-allocating the internal [Vec] comes with many restrictions when
    /// accessing references to its elements, it is recommended to use [Prison::with_capacity()]
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
            internal: UnsafeCell::new(PrisonMutable {
                access_count: 0,
                free_count: 0,
                generation: 0,
                next_free: IdxD::INVALID,
                vec: Vec::new(),
            }),
        };
    }

    //FN Prison::with_capacity()
    /// Create a new [Prison<T>] with a specific starting capacity ([Vec::with_capacity()])
    ///
    /// Because [Prison<T>] accepts values that may or may not be implement [Copy], [Clone],
    /// or [Default] and because indexes are simply marked as "free" when their values are removed
    /// from the [Prison], a closure must be provided upon creation of a new prison
    /// that supplies it default values to replace the removed ones with safely ([mem::replace()])
    /// without running into double-frees or use-after-frees or resorting to things like
    /// [ManuallyDrop](std::mem::ManuallyDrop) or [MaybeUninit](std::mem::MaybeUninit)
    ///
    /// Because re-allocating the internal [Vec] comes with many restrictions when
    /// accessing references to its elements, it is recommended to use [Prison::with_capacity()]
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
            internal: UnsafeCell::new(PrisonMutable {
                access_count: 0,
                free_count: 0,
                generation: 0,
                next_free: IdxD::INVALID,
                vec: Vec::with_capacity(size),
            }),
        };
    }

    //FN Prison::vec_len()
    /// Return the length of the underlying [Vec]
    ///
    /// Because a [Prison] may have values that are free/deleted that are still counted
    /// within the length of the [Vec], this value should not be used to determine how many
    /// *valid* elements exist in the [Prison]
    #[inline(always)]
    pub fn vec_len(&self) -> usize {
        return internal!(self).vec.len();
    }

    //FN Prison::vec_cap()
    /// Return the capacity of the underlying [Vec]
    ///
    /// Capacity refers to the number of total spaces in memory reserved for the [Vec]
    ///
    /// Because a [Prison] may have values that are free/deleted that are *not* counted
    /// withing the capacity of the [Vec], this value should not be used to determine how many
    /// *empty* spots exist to add elements into the [Prison]
    #[inline(always)]
    pub fn vec_cap(&self) -> usize {
        return internal!(self).vec.capacity();
    }

    //FN Prison::num_free()
    /// Return the number of spaces available for elements to be added to the [Prison]
    /// without reallocating more memory.
    #[inline(always)]
    pub fn num_free(&self) -> usize {
        let internal = internal!(self);
        return internal.free_count + internal.vec.capacity() - internal.vec.len();
    }

    //FN Prison::num_used()
    /// Return the number of spaces currently occupied by valid elements in the [Prison]
    #[inline(always)]
    pub fn num_used(&self) -> usize {
        let internal = internal!(self);
        return internal.vec.len() - internal.free_count;
    }

    //FN Prison::density()
    /// Return the ratio of used space to total space in the [Prison]
    ///
    /// 0.0 = 0% used, 1.0 = 100% used
    pub fn density(&self) -> f32 {
        let internal = internal!(self);
        let used = internal.vec.len() - internal.free_count;
        let cap = internal.vec.capacity();
        return (used as f32) / (cap as f32);
    }

    //FN Prison::insert()
    /// Insert a value into the [Prison] and recieve a [CellKey] that can be used to
    /// reference it in the future
    ///
    /// As long as there are sufficient free cells or vector capacity to do so,
    /// you may `insert()` to the [Prison] while any of its elements have active references
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(10);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// string_prison.visit_ref(key_0, |first_string| {
    ///     let key_1 = string_prison.insert(String::from("World!"))?;
    ///     string_prison.visit_ref(key_1, |second_string| {
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
    /// during while there are active references to any element will cause the operation to fail and a
    /// [AccessError::InsertAtMaxCapacityWhileAValueIsReferenced] to be returned
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(1);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// string_prison.visit_ref(key_0, |first_string| {
    ///     assert!(string_prison.insert(String::from("World!")).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn insert(&self, value: T) -> Result<CellKey, AccessError> {
        let internal = internal!(self);
        if internal.next_free == IdxD::INVALID {
            if internal.vec.capacity() <= internal.vec.len() {
                if internal.access_count > 0 {
                    return Err(AccessError::InsertAtMaxCapacityWhileAValueIsReferenced);
                }
                if internal.vec.capacity() == IdxD::MAX_CAP {
                    return Err(AccessError::MaximumCapacityReached);
                }
            }
            internal.vec.push(PrisonCell::new_cell(value, internal.generation));
            return Ok(CellKey {
                idx: internal.vec.len() - 1,
                gen: internal.generation,
            });
        }
        let new_idx = internal.next_free;
        match &mut internal.vec[new_idx] {
            free if free.is_free() => {
                internal.free_count -= 1;
                internal.next_free = free.refs_or_next;
                free.make_cell_unchecked(value, internal.generation);
                Ok(CellKey {
                    idx: new_idx,
                    gen: internal.generation,
                })
            }
            _ => major_malfunction!(format!("`Prison` had a recorded `next_free` index ({}) that WAS NOT FREE", new_idx))
        }
    }

    //FN Prison::insert_at()
    /// #### This operation has O(N) time complexity
    /// 
    /// Insert a value into the [Prison] at the specified index and recieve a
    /// [CellKey] that can be used to reference it in the future
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
    /// string_prison.visit_many_ref(&[key_0, key_1], |vals| {
    ///     let hello_world = format!("{}{}", vals[0], vals[1]);
    ///     assert_eq!(hello_world, "Hello, Rust!!");
    ///     Ok(())
    /// })?;
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
        let internal: &mut PrisonMutable<T> = internal!(self);
        if idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(idx));
        }
        match &mut internal.vec[idx] {
            free if free.is_free() => {
                let prev = IdxD::val(free.d_gen_or_prev);
                if prev != IdxD::INVALID {
                    match &mut internal!(self).vec[prev] {
                        prev_free if prev_free.is_free() => prev_free.refs_or_next = free.refs_or_next,
                        _ => major_malfunction!(format!("a `Free` index ({}) had a `prev_free` that pointed to an index ({}) that WAS NOT FREE", idx, prev))
                    }
                } else if internal.next_free == idx {
                    internal.next_free = free.refs_or_next;
                } else {
                    major_malfunction!(format!("a `Free` index ({}) had a `prev_free` value that indicated `INVALID`, meaning it should have been the top of the `free` stack, but `Prison.next_free` ({}) did not match its index", prev, internal.next_free))
                }
                if free.refs_or_next != IdxD::INVALID {
                    match &mut internal!(self).vec[free.refs_or_next] {
                        next_free if next_free.is_free() => next_free.d_gen_or_prev = IdxD::new_type_b(prev),
                        _ => major_malfunction!(format!("a `Free` index ({}) had a `next_free` that pointed to an index ({}) that WAS NOT FREE", idx, free.refs_or_next))
                    }
                }
                internal.free_count -= 1;
                free.make_cell_unchecked(value, internal.generation);
                return Ok(CellKey { idx, gen: internal.generation });
            },
            _ => return Err(AccessError::IndexIsNotFree(idx))
        }
    }

    //FN Prison::overwrite()
    /// Insert or overwrite a value in the [Prison] at the specified index and recieve a
    /// [CellKey] that can be used to reference it in the future
    ///
    /// Similar to [Prison::insert_at()] but does not require the space be marked as free.
    ///
    /// Note: Overwriting a value that isn't marked as free will invalidate any [CellKey]
    /// that could have been used to reference it and cause a lookup using the old
    /// key(s) to return an [AccessError::ValueDeleted(idx, gen)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(10);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// let key_1_a = string_prison.insert(String::from("World!"))?;
    /// // string_prison.remove(key_1)?; // removal not needed
    /// let key_1_b = string_prison.overwrite(1, String::from("Rust!!"))?;
    /// string_prison.visit_many_ref(&[key_0, key_1_b], |vals| {
    ///     let hello_world = format!("{}{}", vals[0], vals[1]);
    ///     assert_eq!(hello_world, "Hello, Rust!!");
    ///     Ok(())
    /// });
    /// assert!(string_prison.visit_ref(key_1_a, |deleted_val| Ok(())).is_err());
    /// assert!(string_prison.overwrite(10, String::from("Oops...")).is_err());
    /// # Ok(())
    /// # }
    #[inline(always)]
    pub fn overwrite(&self, idx: usize, value: T) -> Result<CellKey, AccessError> {
        let internal: &mut PrisonMutable<T> = internal!(self);
        if idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(idx));
        }
        match &mut internal.vec[idx] {
            cell if cell.is_cell() => {
                if cell.refs_or_next > 0 {
                    return Err(AccessError::OverwriteWhileValueReferenced(idx));
                }
                let cell_gen = IdxD::val(cell.d_gen_or_prev);
                if cell_gen >= internal.generation {
                    if cell_gen == IdxD::MAX_GEN {
                        return Err(AccessError::MaxValueForGenerationReached);
                    }
                    internal.generation = cell_gen + 1;
                }
                cell.overwrite_cell_unchecked(value, internal.generation);
                return Ok(CellKey { idx, gen: internal.generation });
            },
            free  => {
                let prev = IdxD::val(free.d_gen_or_prev);
                if prev != IdxD::INVALID {
                    match &mut internal!(self).vec[prev] {
                        prev_free if prev_free.is_free() => prev_free.refs_or_next = free.refs_or_next,
                        _ => major_malfunction!(format!("a `Free` index ({}) had a `prev_free` that pointed to an index ({}) that WAS NOT FREE", idx, prev))
                    }
                } else if internal.next_free == idx {
                    internal.next_free = free.refs_or_next;
                } else {
                    major_malfunction!(format!("a `free` index ({}) had a `prev_free` value that indicated `INVALID`, meaning it should have been the top of the `free` stack, but `Prison.next_free` ({}) did not match its index", prev, internal.next_free))
                }
                if free.refs_or_next != IdxD::INVALID {
                    match &mut internal!(self).vec[free.refs_or_next] {
                        next_free if next_free.is_free() => next_free.d_gen_or_prev = IdxD::new_type_b(prev),
                        _ => major_malfunction!(format!("a `Free` index ({}) had a `next_free` that pointed to an index ({}) that WAS NOT FREE", idx, free.refs_or_next))
                    }
                }
                internal.free_count -= 1;
                free.make_cell_unchecked(value, internal.generation);
                return Ok(CellKey { idx, gen: internal.generation });
            },
        }
    }

    //FN Prison::remove()
    /// Remove and return the element indexed by the provided [CellKey]
    ///
    /// As long as the element doesn't have an active reference you can `.remove()` it
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(15);
    /// let key_0 = string_prison.insert(String::from("Hello, "))?;
    /// let key_1 = string_prison.insert(String::from("World!"))?;
    /// let mut take_world = String::new();
    /// string_prison.visit_ref(key_0, |hello| {
    ///     take_world = string_prison.remove(key_1)?;
    ///     Ok(())
    /// })?;
    /// assert_eq!(take_world, "World!");
    /// # Ok(())
    /// # }
    /// ```
    /// However, if the element *does* have an active reference, either from `visit()` or `guard()`,
    /// `remove()` will return an [AccessError::RemoveWhileValueReferenced(idx)] with the index in question
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError>  {
    /// let string_prison: Prison<String> = Prison::with_capacity(15);
    /// let key_0 = string_prison.insert(String::from("Everything"))?;
    /// string_prison.visit_ref(key_0, |everything| {
    ///     assert!(string_prison.remove(key_0).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn remove(&self, key: CellKey) -> Result<T, AccessError> {
        let internal = internal!(self);
        if key.idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(key.idx));
        }
        let removed_val = match &mut internal.vec[key.idx] {
            cell if cell.is_cell_and_gen_match(key.gen) => {
                if cell.refs_or_next > 0 {
                    return Err(AccessError::RemoveWhileValueReferenced(key.idx));
                }
                let cell_gen = IdxD::val(cell.d_gen_or_prev);
                if cell_gen >= internal.generation {
                    if cell_gen == IdxD::MAX_GEN {
                        return Err(AccessError::MaxValueForGenerationReached);
                    }
                    internal.generation = cell_gen + 1;
                }
                cell.make_free_unchecked(internal.next_free, IdxD::INVALID)
            }
            _ => return Err(AccessError::ValueDeleted(key.idx, key.gen)),
        };
        if internal.next_free != IdxD::INVALID {
            match &mut internal.vec[internal.next_free] {
                free if free.is_free() => {
                    free.d_gen_or_prev = IdxD::new_type_b(key.idx);
                },
                _ => major_malfunction!(format!("the `prison.next_free` index ({}) pointed to an element that WAS NOT FREE", internal.next_free))
            }
        }
        internal.next_free = key.idx;
        internal.free_count += 1;
        return Ok(removed_val);
    }

    //FN Prison::remove_idx()
    /// Remove and return the element at the specified index
    ///
    /// Like `remove()` but disregards the generation counter
    ///
    /// As long as the element doesnt have an active reference you can `.remove_idx()` it
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_prison: Prison<String> = Prison::with_capacity(15);
    /// string_prison.insert(String::from("Hello, "))?;
    /// string_prison.insert(String::from("World!"))?;
    /// let mut take_world = String::new();
    /// string_prison.visit_ref_idx(0, |hello| {
    ///     take_world = string_prison.remove_idx(1)?;
    ///     Ok(())
    /// })?;
    /// assert_eq!(take_world, "World!");
    /// # Ok(())
    /// # }
    /// ```
    /// However, if the element *does* have an active reference, either from `visit()` or `guard()`,
    /// `.remove_idx()` will return an [AccessError::RemoveWhileValueReferenced(idx)] with the index in question
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError>  {
    /// let string_prison: Prison<String> = Prison::with_capacity(15);
    /// string_prison.insert(String::from("Everything"))?;
    /// string_prison.visit_ref_idx(0, |everything| {
    ///     assert!(string_prison.remove_idx(0).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn remove_idx(&self, idx: usize) -> Result<T, AccessError> {
        let internal = internal!(self);
        if idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(idx));
        }
        let removed_val = match &mut internal.vec[idx] {
            cell if cell.is_cell() => {
                if cell.refs_or_next > 0 {
                    return Err(AccessError::RemoveWhileValueReferenced(idx));
                }
                let cell_gen = IdxD::val(cell.d_gen_or_prev);
                if cell_gen >= internal.generation {
                    if cell_gen == IdxD::MAX_GEN {
                        return Err(AccessError::MaxValueForGenerationReached);
                    }
                    internal.generation = cell_gen + 1;
                }
                cell.make_free_unchecked(internal.next_free, IdxD::INVALID)
            }
            _ => return Err(AccessError::ValueDeleted(idx, 0)),
        };
        internal.next_free = idx;
        internal.free_count += 1;
        return Ok(removed_val);
    }

    //FN Prison::visit_mut()
    /// Visit a single value in the [Prison], obtaining a mutable reference to the
    /// value that is passed into a closure you provide.
    ///
    /// You can only obtain a single mutable reference to an element at any given time, and cannot move the mutable
    /// reference out of the closure, meaning there is only one mutable reference to it at
    /// any time (and zero immutable references).
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// u32_prison.visit_mut(key_0, |mut_ref_42| {
    ///     *mut_ref_42 = 69; // nice
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if element is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(idx)] if element has any number of immutable references
    /// - [AccessError::IndexOutOfRange(idx)] if the [CellKey] index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if the cell is marked as free/deleted *OR* the [CellKey] generation doe not match
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(69)?;
    /// u32_prison.remove(key_1)?;
    /// u32_prison.visit_mut(key_0, |mut_ref_42| {
    ///     assert!(u32_prison.visit_mut(key_0, |mut_ref_42_again| Ok(())).is_err());
    ///     assert!(u32_prison.visit_ref(key_0, |mut_ref_42_again| Ok(())).is_err());
    ///     assert!(u32_prison.visit_mut(CellKey::from_raw_parts(5, 5), |doesnt_exist| Ok(())).is_err());
    ///     assert!(u32_prison.visit_mut(key_1, |deleted| Ok(())).is_err());
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
    /// u32_prison.visit_mut(key_0, |mut_ref_42| {
    ///     // will not compile: (error[E0521]: borrowed data escapes outside of closure)
    ///     try_to_take_the_ref = mut_ref_42;
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn visit_mut<F>(&self, key: CellKey, mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&mut T) -> Result<(), AccessError>,
    {
        let (cell, accesses) = self._add_mut_ref(key.idx, key.gen, true)?;
        let res = operation(unsafe {cell.val.assume_init_mut()});
        _remove_mut_ref(&mut cell.refs_or_next, accesses);
        return res;
    }

    //FN Prison::visit_ref()
    /// Visit a single value in the [Prison], obtaining an immutable reference to the
    /// value that is passed into a closure you provide.
    ///
    /// You obtain any number of simultaneous immutable references to an element,
    /// cannot obtain a mutable reference while any immutable references are active,
    /// and cannot move the immutable references out of the closure,
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// u32_prison.visit_ref(key_0, |ref_42_a| {
    ///     u32_prison.visit_ref(key_0, |ref_42_b| {
    ///         assert_eq!(*ref_42_a, *ref_42_b);
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if element is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(idx)] if you created [usize::MAX] - 2 immutable references already
    /// - [AccessError::IndexOutOfRange(idx)] if the [CellKey] index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if the cell is marked as free/deleted *OR* the [CellKey] generation doe not match
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(69)?;
    /// u32_prison.remove(key_1)?;
    /// u32_prison.visit_ref(key_0, |ref_42| {
    ///     assert!(u32_prison.visit_mut(key_0, |mut_ref_42| Ok(())).is_err());
    ///     assert!(u32_prison.visit_ref(CellKey::from_raw_parts(5, 5), |doesnt_exist| Ok(())).is_err());
    ///     assert!(u32_prison.visit_ref(key_1, |deleted| Ok(())).is_err());
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
    /// let mut try_to_take_the_ref: & u32 = & 0;
    /// u32_prison.visit_ref(key_0, |ref_42| {
    ///     // will not compile: (error[E0521]: borrowed data escapes outside of closure)
    ///     try_to_take_the_ref = ref_42;
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn visit_ref<F>(&self, key: CellKey, mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&T) -> Result<(), AccessError>,
    {
        let (cell, accesses) = self._add_imm_ref(key.idx, key.gen, true)?;
        let res = operation(unsafe {cell.val.assume_init_ref()});
        _remove_imm_ref(&mut cell.refs_or_next, accesses);
        return res;
    }

    //FN Prison::visit_mut_idx()
    /// Visit a single value in the [Prison], obtaining a mutable reference to the
    /// value that is passed into a closure you provide.
    ///
    /// Similar to `visit_mut()` but ignores the generation counter
    ///
    /// You can only obtain a single mutable reference to an element at any given time, and cannot move the mutable
    /// reference out of the closure, meaning there is only one mutable reference to it at
    /// any time (and zero immutable references).
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.visit_mut_idx(0, |mut_ref_42| {
    ///     *mut_ref_42 = 69; // nice
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if element is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(idx)] if element has any number of immutable references
    /// - [AccessError::IndexOutOfRange(idx)] if the index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if the cell is marked as free/deleted
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(69)?;
    /// u32_prison.remove_idx(1)?;
    /// u32_prison.visit_mut_idx(0, |mut_ref_42| {
    ///     assert!(u32_prison.visit_mut_idx(0, |mut_ref_42_again| Ok(())).is_err());
    ///     assert!(u32_prison.visit_ref_idx(0, |mut_ref_42_again| Ok(())).is_err());
    ///     assert!(u32_prison.visit_mut_idx(5, |doesnt_exist| Ok(())).is_err());
    ///     assert!(u32_prison.visit_mut_idx(1, |deleted| Ok(())).is_err());
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
    /// u32_prison.visit_mut_idx(0, |mut_ref_42| {
    ///     // will not compile: (error[E0521]: borrowed data escapes outside of closure)
    ///     try_to_take_the_ref = mut_ref_42;
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn visit_mut_idx<F>(&self, idx: usize, mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&mut T) -> Result<(), AccessError>,
    {
        let (cell, accesses) = self._add_mut_ref(idx, 0, false)?;
        let res = operation(unsafe {cell.val.assume_init_mut()});
        _remove_mut_ref(&mut cell.refs_or_next, accesses);
        return res;
    }

    //FN Prison::visit_ref_idx()
    /// Visit a single value in the [Prison], obtaining an immutable reference to the
    /// value that is passed into a closure you provide.
    ///
    /// Similar to `visit_ref()` but ignores the generation counter
    ///
    /// You obtain any number of simultaneous immutable references to an element,
    /// cannot obtain a mutable reference while any immutable references are active,
    /// and cannot move the immutable references out of the closure,
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.visit_ref_idx(0, |ref_42_a| {
    ///     u32_prison.visit_ref_idx(0, |ref_42_b| {
    ///         assert_eq!(*ref_42_a, *ref_42_b);
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if element is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(idx)] if you created [usize::MAX] - 2 immutable references already
    /// - [AccessError::IndexOutOfRange(idx)] if the [CellKey] index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if the cell is marked as free/deleted
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(69)?;
    /// u32_prison.remove_idx(1)?;
    /// u32_prison.visit_ref_idx(0, |ref_42| {
    ///     assert!(u32_prison.visit_mut_idx(0, |mut_ref_42| Ok(())).is_err());
    ///     assert!(u32_prison.visit_ref_idx(5, |doesnt_exist| Ok(())).is_err());
    ///     assert!(u32_prison.visit_ref_idx(1, |deleted| Ok(())).is_err());
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
    /// let mut try_to_take_the_ref: & u32 = & 0;
    /// u32_prison.visit_ref_idx(0, |ref_42| {
    ///     // will not compile: (error[E0521]: borrowed data escapes outside of closure)
    ///     try_to_take_the_ref = ref_42;
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn visit_ref_idx<F>(&self, idx: usize, mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&T) -> Result<(), AccessError>,
    {
        let (cell, accesses) = self._add_imm_ref(idx, 0, false)?;
        let res = operation(unsafe {cell.val.assume_init_ref()});
        _remove_imm_ref(&mut cell.refs_or_next, accesses);
        return res;
    }

    //FN Prison::visit_many_mut()
    /// Visit many values in the [Prison] at the same time, obtaining a mutable reference
    /// to all of them in the same closure and in the same order they were requested.
    ///
    /// While you can obtain multiple unrelated mutable references simultaneously,
    /// you can only obtain a single mutable reference to the same element at any given time, and cannot move the mutable
    /// reference out of the closure, meaning there is only one mutable reference to it at
    /// any time (and zero immutable references).
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(43)?;
    /// let key_2 = u32_prison.insert(44)?;
    /// let key_3 = u32_prison.insert(45)?;
    /// u32_prison.visit_many_mut(&[key_3, key_2, key_1, key_0], |first_four_reversed| {
    ///     assert_eq!(*first_four_reversed[0], 45);
    ///     assert_eq!(*first_four_reversed[1], 44);
    ///     assert_eq!(*first_four_reversed[2], 43);
    ///     assert_eq!(*first_four_reversed[3], 42);
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// Just like `.visit_mut()`, any particular element can only have one mutable reference,
    /// but as long as the elements requested don't overlap you may make nested
    /// `visit()` or `guard()` calls
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(43)?;
    /// let key_2 = u32_prison.insert(44)?;
    /// let key_3 = u32_prison.insert(45)?;
    /// u32_prison.visit_many_mut(&[key_0, key_2], |evens| {
    ///     u32_prison.visit_many_mut(&[key_1, key_3], |odds| {
    ///         assert_eq!(*evens[1], 44);
    ///         assert_eq!(*odds[1], 45);
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if any element is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(idx)] if any element has any number of immutable references
    /// - [AccessError::IndexOutOfRange(idx)] if any index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if any cell is marked as free/deleted *OR* the [CellKey] generation doesnt match
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
    /// let key_4 = CellKey::from_raw_parts(4, 1);
    /// assert!(u32_prison.visit_many_mut(&[key_0, key_0], |double_key_zero| Ok(())).is_err());
    /// assert!(u32_prison.visit_many_mut(&[key_1, key_2, key_3], |key_1_removed| Ok(())).is_err());
    /// assert!(u32_prison.visit_many_mut(&[key_2, key_3, key_4], |key_4_invalid| Ok(())).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn visit_many_mut<F>(&self, keys: &[CellKey], mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&[&mut T]) -> Result<(), AccessError>,
    {
        let (vals, mut refs, accesses) = self._add_many_mut_refs(keys)?;
        let result = operation(&vals);
        _remove_many_mut_refs(&mut refs, accesses);
        return result;
    }

    //FN Prison::visit_many_ref()
    /// Visit many values in the [Prison] at the same time, obtaining an immutable reference
    /// to all of them in the same closure and in the same order they were requested.
    ///
    /// As long as the element does not have any mutable references, you can obtain multiple
    /// immutable references to the same element
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(43)?;
    /// let key_2 = u32_prison.insert(44)?;
    /// let key_3 = u32_prison.insert(45)?;
    /// u32_prison.visit_many_ref(&[key_3, key_2, key_1, key_0], |first_four_reversed| {
    ///     assert_eq!(*first_four_reversed[0], 45);
    ///     assert_eq!(*first_four_reversed[1], 44);
    ///     assert_eq!(*first_four_reversed[2], 43);
    ///     assert_eq!(*first_four_reversed[3], 42);
    ///     u32_prison.visit_many_ref(&[key_0, key_1, key_2, key_3], |first_four_original| {
    ///         assert_eq!(*first_four_original[0], 42);
    ///         assert_eq!(*first_four_original[1], 43);
    ///         assert_eq!(*first_four_original[2], 44);
    ///         assert_eq!(*first_four_original[3], 45);
    ///         Ok(())
    ///     })?;
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// Just like `.visit_ref()`, any particular element can have multile immutable references to
    /// it as long as it has no mutable, meaning you can make nested
    /// `visit()` or `guard()` calls to the same element if desired
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// let key_0 = u32_prison.insert(42)?;
    /// let key_1 = u32_prison.insert(43)?;
    /// let key_2 = u32_prison.insert(44)?;
    /// let key_3 = u32_prison.insert(45)?;
    /// u32_prison.visit_many_ref(&[key_0, key_2], |evens| {
    ///     u32_prison.visit_many_ref(&[key_1, key_3], |odds| {
    ///         assert_eq!(*evens[1], 44);
    ///         assert_eq!(*odds[1], 45);
    ///         assert!(u32_prison.visit_many_ref(&[key_0, key_1, key_2, key_3], |all| Ok(())).is_ok());
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if any element is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(idx)] if you created [usize::MAX] - 2 immutable references to any element
    /// - [AccessError::IndexOutOfRange(idx)] if any index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if any cell is marked as free/deleted *OR* if the [CellKey] generation doesn't match
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
    /// let key_4 = CellKey::from_raw_parts(4, 1);
    /// u32_prison.visit_mut(key_0, |mut_0| {
    ///     assert!(u32_prison.visit_many_ref(&[key_0], |zero_already_mut| Ok(())).is_err());
    ///     assert!(u32_prison.visit_many_ref(&[key_1, key_2, key_3], |key_1_removed| Ok(())).is_err());
    ///     assert!(u32_prison.visit_many_ref(&[key_2, key_3, key_4], |key_4_invalid| Ok(())).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn visit_many_ref<F>(&self, keys: &[CellKey], mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&[&T]) -> Result<(), AccessError>,
    {
        let (vals, mut refs, accesses) = self._add_many_imm_refs(keys)?;
        let result = operation(&vals);
        _remove_many_imm_refs(&mut refs, accesses);
        return result;
    }

    //FN Prison::visit_many_mut_idx()
    /// Visit many values in the [Prison] at the same time, obtaining a mutable reference
    /// to all of them in the same closure and in the same order they were requested.
    ///
    /// Similar to `visit_many_mut()` but ignores the generation counter
    ///
    /// While you can obtain multiple unrelated mutable references simultaneously,
    /// you can only obtain a single mutable reference to the same element at any given time, and cannot move the mutable
    /// reference out of the closure, meaning there is only one mutable reference to it at
    /// any time (and zero immutable references).
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.visit_many_mut_idx(&[3, 2, 1, 0], |first_four_reversed| {
    ///     assert_eq!(*first_four_reversed[0], 45);
    ///     assert_eq!(*first_four_reversed[1], 44);
    ///     assert_eq!(*first_four_reversed[2], 43);
    ///     assert_eq!(*first_four_reversed[3], 42);
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// Just like `.visit_mut_idx()`, any particular element can only have one mutable reference,
    /// but as long as the elements requested don't overlap you may make nested
    /// `visit()` or `guard()` calls
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.visit_many_mut_idx(&[0, 2], |evens| {
    ///     u32_prison.visit_many_mut_idx(&[1, 3], |odds| {
    ///         assert_eq!(*evens[1], 44);
    ///         assert_eq!(*odds[1], 45);
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if any element is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(idx)] if any element has any number of immutable references
    /// - [AccessError::IndexOutOfRange(idx)] if any index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if any cell is marked as free/deleted
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
    /// assert!(u32_prison.visit_many_mut_idx(&[0, 0], |double_idx_zero| Ok(())).is_err());
    /// assert!(u32_prison.visit_many_mut_idx(&[1, 2, 3], |idx_1_removed| Ok(())).is_err());
    /// assert!(u32_prison.visit_many_mut_idx(&[2, 3, 4], |idx_4_invalid| Ok(())).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn visit_many_mut_idx<F>(&self, indexes: &[usize], mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&[&mut T]) -> Result<(), AccessError>,
    {
        let (vals, mut refs, accesses) = self._add_many_mut_refs_idx(indexes)?;
        let result = operation(&vals);
        _remove_many_mut_refs(&mut refs, accesses);
        return result;
    }

    //FN Prison::visit_many_ref_idx()
    /// Visit many values in the [Prison] at the same time, obtaining an immutable reference
    /// to all of them in the same closure and in the same order they were requested.
    ///
    /// Similar to `visit_many_ref()` but ignores the generation counter
    ///
    /// As long as the element does not have any mutable references, you can obtain multiple
    /// immutable references to the same element
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.visit_many_ref_idx(&[3, 2, 1, 0], |first_four_reversed| {
    ///     assert_eq!(*first_four_reversed[0], 45);
    ///     assert_eq!(*first_four_reversed[1], 44);
    ///     assert_eq!(*first_four_reversed[2], 43);
    ///     assert_eq!(*first_four_reversed[3], 42);
    ///     u32_prison.visit_many_ref_idx(&[0, 1, 2, 3], |first_four_original| {
    ///         assert_eq!(*first_four_original[0], 42);
    ///         assert_eq!(*first_four_original[1], 43);
    ///         assert_eq!(*first_four_original[2], 44);
    ///         assert_eq!(*first_four_original[3], 45);
    ///         Ok(())
    ///     })?;
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// Just like `.visit_ref_idx()`, any particular element can have multiple immutable references to
    /// it as long as it has no mutable references, meaning you can make nested
    /// `visit()` or `guard()` calls to the same element if desired
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.visit_many_ref_idx(&[0, 2], |evens| {
    ///     u32_prison.visit_many_ref_idx(&[1, 3], |odds| {
    ///         assert_eq!(*evens[1], 44);
    ///         assert_eq!(*odds[1], 45);
    ///         assert!(u32_prison.visit_many_ref_idx(&[0, 1, 2, 3], |all| Ok(())).is_ok());
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if any element is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(idx)] if you created [usize::MAX] - 2 immutable references to any element
    /// - [AccessError::IndexOutOfRange(idx)] if any index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if any cell is marked as free/deleted
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
    /// u32_prison.visit_mut_idx(0, |mut_0| {
    ///     assert!(u32_prison.visit_many_ref_idx(&[0], |zero_already_mut| Ok(())).is_err());
    ///     assert!(u32_prison.visit_many_ref_idx(&[1, 2, 3], |idx_1_removed| Ok(())).is_err());
    ///     assert!(u32_prison.visit_many_ref_idx(&[2, 3, 4], |idx_4_invalid| Ok(())).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn visit_many_ref_idx<F>(&self, indexes: &[usize], mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&[&T]) -> Result<(), AccessError>,
    {
        let (vals, mut refs, accesses) = self._add_many_imm_refs_idx(indexes)?;
        let result = operation(&vals);
        _remove_many_imm_refs(&mut refs, accesses);
        return result;
    }

    //FN Prison::visit_slice_mut()
    /// Visit a slice of values in the [Prison] at the same time, obtaining a mutable reference
    /// to all of them in the same closure.
    ///
    /// Internally this is strictly identical to passing [Prison::visit_many_mut_idx()] a list of all
    /// indexes in the slice range, and is subject to all the same restrictions and errors
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
    /// u32_prison.visit_slice_mut(2..5, |last_three| {
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
    /// value or does not have any other references to it that would violate Rust's memory safety.
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
    /// assert!(u32_prison.visit_slice_mut(2..5,  |last_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_mut(2..=4, |also_last_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_mut(2..,   |again_last_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_mut(..3,   |first_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_mut(..=3,  |first_four| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_mut(..,    |all| Ok(())).is_ok());
    /// u32_prison.remove_idx(2)?;
    /// assert!(u32_prison.visit_slice_mut(..,    |all| Ok(())).is_err());
    /// # Ok(())
    /// # }
    /// ```
    /// See [Prison::visit_many_mut_idx()] for more info
    pub fn visit_slice_mut<R, F>(&self, range: R, operation: F) -> Result<(), AccessError>
    where
        R: RangeBounds<usize>,
        F: FnMut(&[&mut T]) -> Result<(), AccessError>,
    {
        let (start, end) = extract_true_start_end(range, self.vec_len());
        let idxs: Vec<usize> = (start..end).into_iter().collect();
        self.visit_many_mut_idx(&idxs, operation)
    }

    //FN Prison::visit_slice_ref()
    /// Visit a slice of values in the [Prison] at the same time, obtaining an immutable reference
    /// to all of them in the same closure.
    ///
    /// Internally this is strictly identical to passing [Prison::visit_many_ref_idx()] a list of all
    /// indexes in the slice range, and is subject to all the same restrictions and errors
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
    /// u32_prison.visit_slice_ref(2..5, |last_three| {
    ///     assert_eq!(*last_three[0], 44);
    ///     assert_eq!(*last_three[1], 45);
    ///     assert_eq!(*last_three[2], 46);
    ///     u32_prison.visit_slice_ref(0..3, |first_three| {
    ///         assert_eq!(*first_three[0], 42);
    ///         assert_eq!(*first_three[1], 43);
    ///         assert_eq!(*first_three[2], 44);
    ///         Ok(())
    ///     });
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// Any standard [Range<usize>](std::ops::Range) notation is allowed as the first paramater,
    /// but care must be taken because it is not guaranteed every index within range is a valid
    /// value or does not have any other references to it that would violate Rust's memory safety.
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
    /// assert!(u32_prison.visit_slice_ref(2..5,  |last_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_ref(2..=4, |also_last_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_ref(2..,   |again_last_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_ref(..3,   |first_three| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_ref(..=3,  |first_four| Ok(())).is_ok());
    /// assert!(u32_prison.visit_slice_ref(..,    |all| Ok(())).is_ok());
    /// u32_prison.remove_idx(2)?;
    /// assert!(u32_prison.visit_slice_ref(..,    |all| Ok(())).is_err());
    /// # Ok(())
    /// # }
    /// ```
    /// See [Prison::visit_many_ref_idx()] for more info
    pub fn visit_slice_ref<R, F>(&self, range: R, operation: F) -> Result<(), AccessError>
    where
        R: RangeBounds<usize>,
        F: FnMut(&[&T]) -> Result<(), AccessError>,
    {
        let (start, end) = extract_true_start_end(range, self.vec_len());
        let idxs: Vec<usize> = (start..end).into_iter().collect();
        self.visit_many_ref_idx(&idxs, operation)
    }

    //FN Prison::guard_mut()
    /// Return a [PrisonValueMut] that contains a mutable reference to the element and wraps it in
    /// guarding data that automatically frees its reference count it when it goes out of scope.
    ///
    /// [PrisonValueMut<T>] implements [Deref<Target = T>], [DerefMut<Target = T>], [AsRef<T>], [AsMut<T>],
    /// [Borrow<T>], and [BorrowMut<T>] to allow transparent access to its underlying value
    ///
    /// As long as the [PrisonValueMut] remains in scope, the element where it's value resides in the
    /// [Prison] will remain marked as mutably referenced and unable to be referenced a second time.
    /// You can manually drop the [PrisonValueMut] out of scope by passing it as the first parameter
    /// to the function [PrisonValueMut::unguard(p_val_mut)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// let key_0 = prison.insert(10)?;
    /// let mut grd_0 = prison.guard_mut(key_0)?;
    /// assert_eq!(*grd_0, 10);
    /// *grd_0 = 20;
    /// PrisonValueMut::unguard(grd_0);
    /// prison.visit_ref(key_0, |val_0| {
    ///     assert_eq!(*val_0, 20);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if element is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(idx)] if element has any number of immutable references
    /// - [AccessError::IndexOutOfRange(idx)] if the [CellKey] index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if the cell is marked as free/deleted *OR* the [CellKey] generation does not match
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::with_capacity(2);
    /// let key_0 = prison.insert(10)?;
    /// let key_1_a = prison.insert(20)?;
    /// let key_out_of_bounds = CellKey::from_raw_parts(10, 0);
    /// prison.remove(key_1_a)?;
    /// let key_1_b = prison.insert(30)?;
    /// let mut grd_0 = prison.guard_mut(key_0)?;
    /// assert!(prison.guard_mut(key_0).is_err());
    /// assert!(prison.guard_mut(key_out_of_bounds).is_err());
    /// assert!(prison.guard_mut(key_1_a).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_mut<'a>(&'a self, key: CellKey) -> Result<PrisonValueMut<'a, T>, AccessError> {
        let (cell, visits) = self._add_mut_ref(key.idx, key.gen, true)?;
        return Ok(PrisonValueMut {
            cell,
            prison_accesses: visits,
        });
    }

    //FN Prison::guard_ref()
    /// Return a [PrisonValueRef] that contains an immutable reference to the element and wraps it in
    /// guarding data that automatically decrements its reference count it when it goes out of scope.
    ///
    /// [PrisonValueRef<T>] implements [Deref<Target = T>], [AsRef<T>], and
    /// [Borrow<T>] to allow transparent access to its underlying value
    ///
    /// As long as the [PrisonValueRef] remains in scope, the element where it's value resides in the
    /// [Prison] will remain marked as immutably referenced and unable to be mutably referenced.
    /// You can manually drop the [PrisonValueRef] out of scope by passing it as the first parameter
    /// to the function [PrisonValueRef::unguard(p_val_ref)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// let key_0 = prison.insert(10)?;
    /// let grd_0 = prison.guard_ref(key_0)?;
    /// assert_eq!(*grd_0, 10);
    /// prison.visit_ref(key_0, |val_0| {
    ///     assert_eq!(*val_0, 10);
    ///     Ok(())
    /// });
    /// assert_eq!(*grd_0, 10);
    /// PrisonValueRef::unguard(grd_0);
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if element is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(idx)] if you created [usize::MAX] - 2 immutable references already
    /// - [AccessError::IndexOutOfRange(idx)] if the [CellKey] index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if the cell is marked as free/deleted *OR* the [CellKey] generation doe not match
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::with_capacity(2);
    /// let key_0 = prison.insert(10)?;
    /// let key_1_a = prison.insert(20)?;
    /// let key_out_of_bounds = CellKey::from_raw_parts(10, 0);
    /// prison.remove(key_1_a)?;
    /// let key_1_b = prison.insert(30)?;
    /// let grd_0 = prison.guard_ref(key_0)?;
    /// assert!(prison.guard_mut(key_0).is_err());
    /// assert!(prison.guard_ref(key_out_of_bounds).is_err());
    /// assert!(prison.guard_ref(key_1_a).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_ref<'a>(&'a self, key: CellKey) -> Result<PrisonValueRef<'a, T>, AccessError> {
        let (cell, visits) = self._add_imm_ref(key.idx, key.gen, true)?;
        return Ok(PrisonValueRef {
            cell,
            prison_accesses: visits,
        });
    }

    //FN Prison::guard_mut_idx()
    /// Return a [PrisonValueMut] that contains a mutable reference to the element and wraps it in
    /// guarding data that automatically frees its reference count it when it goes out of scope.
    ///
    /// Smilar to `guard_mut()` but ignores the generation counter
    ///
    /// [PrisonValueMut<T>] implements [Deref<Target = T>], [DerefMut<Target = T>], [AsRef<T>], [AsMut<T>],
    /// [Borrow<T>], and [BorrowMut<T>] to allow transparent access to its underlying value
    ///
    /// As long as the [PrisonValueMut] remains in scope, the element where it's value resides in the
    /// [Prison] will remain marked as mutably referenced and unable to be referenced a second time.
    /// You can manually drop the [PrisonValueMut] out of scope by passing it as the first parameter
    /// to the function [PrisonValueMut::unguard(p_val_mut)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// let mut grd_0 = prison.guard_mut_idx(0)?;
    /// assert_eq!(*grd_0, 10);
    /// *grd_0 = 20;
    /// PrisonValueMut::unguard(grd_0);
    /// prison.visit_ref_idx(0, |val_0| {
    ///     assert_eq!(*val_0, 20);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if element is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(idx)] if element has any number of immutable references
    /// - [AccessError::IndexOutOfRange(idx)] if the [CellKey] index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if the cell is marked as free/deleted
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::with_capacity(2);
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.remove_idx(1)?;
    /// let mut grd_0 = prison.guard_mut_idx(0)?;
    /// assert!(prison.guard_mut_idx(0).is_err());
    /// assert!(prison.guard_mut_idx(5).is_err());
    /// assert!(prison.guard_mut_idx(1).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_mut_idx<'a>(&'a self, idx: usize) -> Result<PrisonValueMut<'a, T>, AccessError> {
        let (cell, visits) = self._add_mut_ref(idx, 0, false)?;
        return Ok(PrisonValueMut {
            cell,
            prison_accesses: visits,
        });
    }

    //FN Prison::guard_ref_idx()
    /// Return a [PrisonValueRef] that contains an immutable reference to the element and wraps it in
    /// guarding data that automatically decrements its reference count it when it goes out of scope.
    ///
    /// Similar to `guard_ref()` but ignores the generation counter
    ///
    /// [PrisonValueRef<T>] implements [Deref<Target = T>], [AsRef<T>], and
    /// [Borrow<T>] to allow transparent access to its underlying value
    ///
    /// As long as the [PrisonValueRef] remains in scope, the element where it's value resides in the
    /// [Prison] will remain marked as immutably referenced and unable to be mutably referenced.
    /// You can manually drop the [PrisonValueRef] out of scope by passing it as the first parameter
    /// to the function [PrisonValueRef::unguard(p_val_ref)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// let grd_0 = prison.guard_ref_idx(0)?;
    /// assert_eq!(*grd_0, 10);
    /// prison.visit_ref_idx(0, |val_0| {
    ///     assert_eq!(*val_0, 10);
    ///     Ok(())
    /// });
    /// assert_eq!(*grd_0, 10);
    /// PrisonValueRef::unguard(grd_0);
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if element is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(idx)] if you created [usize::MAX] - 2 immutable references already
    /// - [AccessError::IndexOutOfRange(idx)] if the [CellKey] index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if the cell is marked as free/deleted *OR* the [CellKey] generation doe not match
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::with_capacity(2);
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.remove_idx(1)?;
    /// let grd_0 = prison.guard_ref_idx(0)?;
    /// assert!(prison.guard_mut_idx(0).is_err());
    /// assert!(prison.guard_ref_idx(5).is_err());
    /// assert!(prison.guard_ref_idx(1).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_ref_idx<'a>(&'a self, idx: usize) -> Result<PrisonValueRef<'a, T>, AccessError> {
        let (cell, visits) = self._add_imm_ref(idx, 0, false)?;
        return Ok(PrisonValueRef {
            cell,
            prison_accesses: visits,
        });
    }

    
    //FN Prison::guard_many_mut()
    /// Return a [PrisonSliceMut] that marks all the elements as mutably referenced and wraps
    /// them in guarding data that automatically frees their mutable reference counts when it goes out of range.
    ///
    /// [PrisonSliceMut<T>] implements [Deref<Target = \[&mut T\]>](Deref), [DerefMut<Target = \[&mut T\]>](DerefMut), [AsRef<\[&mut T\]>](AsRef), [AsMut<\[&mut T\]>](AsMut),
    /// [Borrow<\[&mut T\]>](Borrow), and [BorrowMut<\[&mut T\]>](BorrowMut) to allow transparent access to its underlying slice of values
    ///
    /// As long as the [PrisonSliceMut] remains in scope, the elements where it's values reside in the
    /// [Prison] will remain marked as mutably referenced and unable to be referenced a second time.
    /// You can manually drop the [PrisonSliceMut] out of scope by passing it as the first parameter
    /// to the function [PrisonSliceMut::unguard(p_sli_mut)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// let key_0 = prison.insert(10)?;
    /// let key_1 = prison.insert(20)?;
    /// let key_2 = prison.insert(30)?;
    /// let mut grd_0_1_2 = prison.guard_many_mut(&[key_0, key_1, key_2])?;
    /// assert_eq!(*grd_0_1_2[0], 10);
    /// *grd_0_1_2[0] = 20;
    /// PrisonSliceMut::unguard(grd_0_1_2);
    /// prison.visit_many_ref(&[key_0, key_1, key_2], |vals_0_1_2| {
    ///     assert_eq!(*vals_0_1_2[0], 20);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if any element is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(idx)] if any element has any number of immutable references
    /// - [AccessError::IndexOutOfRange(idx)] if any index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if any cell is marked as free/deleted *OR* the [CellKey] generation doesnt match
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// let key_0 = prison.insert(10)?;
    /// let key_1 = prison.insert(20)?;
    /// let key_2 = prison.insert(30)?;
    /// let key_4_a = prison.insert(40)?;
    /// prison.remove(key_4_a)?;
    /// let key_4_b = prison.insert(44)?;
    /// let key_out_of_bounds = CellKey::from_raw_parts(10, 1);
    /// let mut grd_0_1_2 = prison.guard_many_mut(&[key_0, key_1, key_2])?;
    /// assert!(prison.guard_many_mut(&[key_0, key_1, key_2, key_4_b]).is_err());
    /// assert!(prison.guard_many_mut(&[key_out_of_bounds]).is_err());
    /// assert!(prison.guard_many_mut(&[key_4_a]).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_many_mut<'a>(
        &'a self,
        keys: &[CellKey],
    ) -> Result<PrisonSliceMut<'a, T>, AccessError> {
        let (vals, refs, prison_accesses) = self._add_many_mut_refs(keys)?;
        return Ok(PrisonSliceMut {
            vals,
            refs,
            prison_accesses,
        });
    }

    //FN Prison::guard_many_ref()
    /// Return a [PrisonSliceRef] that marks all the elements as immutably referenced and wraps
    /// them in guarding data that automatically decreases their immutable reference counts when it goes out of range.
    ///
    /// [PrisonSliceRef<T>] implements [Deref<Target = \[&T\]>](Deref), [AsRef<\[&T\]>](AsRef),
    /// and [Borrow<\[&T\]>](Borrow), to allow transparent access to its underlying slice of values
    ///
    /// As long as the [PrisonSliceRef] remains in scope, the elements where it's values reside in the
    /// [Prison] will remain marked as immutably referenced and unable to be mutably referenced.
    /// You can manually drop the [PrisonSliceRef] out of scope by passing it as the first parameter
    /// to the function [PrisonSliceRef::unguard(p_sli_ref)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// let key_0 = prison.insert(10)?;
    /// let key_1 = prison.insert(20)?;
    /// let key_2 = prison.insert(30)?;
    /// let mut grd_0_1_2 = prison.guard_many_ref(&[key_0, key_1, key_2])?;
    /// assert_eq!(*grd_0_1_2[0], 10);
    /// prison.visit_many_ref(&[key_0, key_1, key_2], |vals_0_1_2| {
    ///     assert_eq!(*vals_0_1_2[0], 10);
    ///     Ok(())
    /// });
    /// PrisonSliceRef::unguard(grd_0_1_2);
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if any element is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(idx)] if you created [usize::MAX] - 2 immutable references to any element
    /// - [AccessError::IndexOutOfRange(idx)] if any index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if any cell is marked as free/deleted *OR* if the [CellKey] generation doesn't match
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// let key_0 = prison.insert(10)?;
    /// let key_1 = prison.insert(20)?;
    /// let key_2 = prison.insert(30)?;
    /// let key_4_a = prison.insert(40)?;
    /// prison.remove(key_4_a)?;
    /// let key_4_b = prison.insert(44)?;
    /// let key_out_of_bounds = CellKey::from_raw_parts(10, 1);
    /// let grd_0_1_2 = prison.guard_many_ref(&[key_0, key_1, key_2])?;
    /// assert!(prison.guard_many_mut(&[key_0]).is_err());
    /// assert!(prison.guard_many_ref(&[key_out_of_bounds]).is_err());
    /// assert!(prison.guard_many_ref(&[key_4_a]).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_many_ref<'a>(
        &'a self,
        keys: &[CellKey],
    ) -> Result<PrisonSliceRef<'a, T>, AccessError> {
        let (vals, refs, prison_accesses) = self._add_many_imm_refs(keys)?;
        return Ok(PrisonSliceRef {
            vals,
            refs,
            prison_accesses,
        });
    }

    //FN Prison::guard_many_mut_idx()
    /// Return a [PrisonSliceMut] that marks all the elements as mutably referenced and wraps
    /// them in guarding data that automatically frees their mutable reference counts when it goes out of range.
    ///
    /// Similar to `guard_many_mut()` but ignores the generation counter
    ///
    /// [PrisonSliceMut<T>] implements [Deref<Target = \[&mut T\]>](Deref), [DerefMut<Target = \[&mut T\]>](DerefMut), [AsRef<\[&mut T\]>](AsRef), [AsMut<\[&mut T\]>](AsMut),
    /// [Borrow<\[&mut T\]>](Borrow), and [BorrowMut<\[&mut T\]>](BorrowMut) to allow transparent access to its underlying slice of values
    ///
    /// As long as the [PrisonSliceMut] remains in scope, the elements where it's values reside in the
    /// [Prison] will remain marked as mutably referenced and unable to be referenced a second time.
    /// You can manually drop the [PrisonSliceMut] out of scope by passing it as the first parameter
    /// to the function [PrisonSliceMut::unguard(p_sli_mut)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.insert(30)?;
    /// let mut grd_0_1_2 = prison.guard_many_mut_idx(&[0, 1, 2])?;
    /// assert_eq!(*grd_0_1_2[0], 10);
    /// *grd_0_1_2[0] = 20;
    /// PrisonSliceMut::unguard(grd_0_1_2);
    /// prison.visit_many_ref_idx(&[0, 1, 2], |vals_0_1_2| {
    ///     assert_eq!(*vals_0_1_2[0], 20);
    ///     Ok(())
    /// });
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if any element is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(idx)] if any element has any number of immutable references
    /// - [AccessError::IndexOutOfRange(idx)] if any index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if any cell is marked as free/deleted
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.insert(30)?;
    /// prison.insert(40)?;
    /// prison.remove_idx(3)?;
    /// let mut grd_0_1_2 = prison.guard_many_mut_idx(&[0, 1, 2])?;
    /// assert!(prison.guard_many_mut_idx(&[0, 1, 2]).is_err());
    /// assert!(prison.guard_many_mut_idx(&[5]).is_err());
    /// assert!(prison.guard_many_mut_idx(&[3]).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_many_mut_idx<'a>(
        &'a self,
        indexes: &[usize],
    ) -> Result<PrisonSliceMut<'a, T>, AccessError> {
        let (vals, refs, prison_accesses) = self._add_many_mut_refs_idx(indexes)?;
        return Ok(PrisonSliceMut {
            vals,
            refs,
            prison_accesses,
        });
    }

    //FN Prison::guard_many_ref_idx()
    /// Return a [PrisonSliceRef] that marks all the elements as immutably referenced and wraps
    /// them in guarding data that automatically decreases their immutable reference counts when it goes out of range.
    ///
    /// Similar to `guard_many_ref()` but ignores the generation counter
    ///
    /// [PrisonSliceRef<T>] implements [Deref<Target = \[&T\]>](Deref), [AsRef<\[&T\]>](AsRef),
    /// and [Borrow<\[&T\]>](Borrow), to allow transparent access to its underlying slice of values
    ///
    /// As long as the [PrisonSliceRef] remains in scope, the elements where it's values reside in the
    /// [Prison] will remain marked as immutably referenced and unable to be mutably referenced.
    /// You can manually drop the [PrisonSliceRef] out of scope by passing it as the first parameter
    /// to the function [PrisonSliceRef::unguard(p_sli_ref)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.insert(30)?;
    /// let mut grd_0_1_2 = prison.guard_many_ref_idx(&[0, 1, 2])?;
    /// assert_eq!(*grd_0_1_2[0], 10);
    /// prison.visit_many_ref_idx(&[0, 1, 2], |vals_0_1_2| {
    ///     assert_eq!(*vals_0_1_2[0], 10);
    ///     Ok(())
    /// });
    /// PrisonSliceRef::unguard(grd_0_1_2);
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(idx)] if any element is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(idx)] if you created [usize::MAX] - 2 immutable references to any element
    /// - [AccessError::IndexOutOfRange(idx)] if any index is out of range
    /// - [AccessError::ValueDeleted(idx, gen)] if any cell is marked as free/deleted
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// prison.insert(30)?;
    /// prison.insert(40)?;
    /// prison.remove_idx(3)?;
    /// let grd_0_1_2 = prison.guard_many_ref_idx(&[0, 1, 2])?;
    /// assert!(prison.guard_many_mut_idx(&[0]).is_err());
    /// assert!(prison.guard_many_ref_idx(&[5]).is_err());
    /// assert!(prison.guard_many_ref_idx(&[3]).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_many_ref_idx<'a>(
        &'a self,
        indexes: &[usize],
    ) -> Result<PrisonSliceRef<'a, T>, AccessError> {
        let (vals, refs, prison_accesses) = self._add_many_imm_refs_idx(indexes)?;
        return Ok(PrisonSliceRef {
            vals,
            refs,
            prison_accesses,
        });
    }

    //FN Prison::guard_slice_mut()
    /// Return a [PrisonSliceMut] that marks all the elements as mutably referenced and wraps
    /// them in guarding data that automatically frees their mutable reference counts when it goes out of range.
    ///
    /// Internally this is strictly identical to passing [Prison::guard_many_mut_idx()] a list of all
    /// indexes in the slice range, and is subject to all the same restrictions and errors
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.insert(46)?;
    /// let grd_last_three = u32_prison.guard_slice_mut(2..5)?;
    /// assert_eq!(*grd_last_three[0], 44);
    /// assert_eq!(*grd_last_three[1], 45);
    /// assert_eq!(*grd_last_three[2], 46);
    /// # Ok(())
    /// # }
    /// ```
    /// Any standard [Range<usize>](std::ops::Range) notation is allowed as the first paramater,
    /// but care must be taken because it is not guaranteed every index within range is a valid
    /// value or does not have any other references to it that would violate Rust's memory safety.
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
    /// assert!(u32_prison.guard_slice_mut(2..5).is_ok());
    /// assert!(u32_prison.guard_slice_mut(2..=4).is_ok());
    /// assert!(u32_prison.guard_slice_mut(2..).is_ok());
    /// assert!(u32_prison.guard_slice_mut(..3).is_ok());
    /// assert!(u32_prison.guard_slice_mut(..=3).is_ok());
    /// assert!(u32_prison.guard_slice_mut(..).is_ok());
    /// u32_prison.remove_idx(2)?;
    /// assert!(u32_prison.guard_slice_mut(..).is_err());
    /// # Ok(())
    /// # }
    /// ```
    /// See [Prison::guard_many_mut_idx()] for more info
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_slice_mut<'a, R>(&'a self, range: R) -> Result<PrisonSliceMut<'a, T>, AccessError>
    where
        R: RangeBounds<usize>,
    {
        let (start, end) = extract_true_start_end(range, self.vec_len());
        let idxs: Vec<usize> = (start..end).into_iter().collect();
        return self.guard_many_mut_idx(&idxs);
    }

    //FN Prison::guard_slice_ref()
    /// Return a [PrisonSliceRef] that marks all the elements as immutably referenced and wraps
    /// them in guarding data that automatically decreases their immutable reference counts when it goes out of range.
    ///
    /// Internally this is strictly identical to passing [Prison::guard_many_ref_idx()] a list of all
    /// indexes in the slice range, and is subject to all the same restrictions and errors
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.insert(46)?;
    /// let grd_last_three = u32_prison.guard_slice_ref(2..5)?;
    /// let grd_all = u32_prison.guard_slice_ref(..)?;
    /// assert_eq!(*grd_all[0], 42);
    /// assert_eq!(*grd_all[1], 43);
    /// assert_eq!(*grd_last_three[0], 44);
    /// assert_eq!(*grd_last_three[1], 45);
    /// assert_eq!(*grd_last_three[2], 46);
    /// # Ok(())
    /// # }
    /// ```
    /// Any standard [Range<usize>](std::ops::Range) notation is allowed as the first paramater,
    /// but care must be taken because it is not guaranteed every index within range is a valid
    /// value or does not have any other references to it that would violate Rust's memory safety.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let u32_prison: Prison<u32> = Prison::new();
    /// u32_prison.insert(42)?;
    /// u32_prison.insert(43)?;
    /// u32_prison.insert(44)?;
    /// u32_prison.insert(45)?;
    /// u32_prison.insert(46)?;
    /// assert!(u32_prison.guard_slice_ref(2..5).is_ok());
    /// assert!(u32_prison.guard_slice_ref(2..=4).is_ok());
    /// assert!(u32_prison.guard_slice_ref(2..).is_ok());
    /// assert!(u32_prison.guard_slice_ref(..3).is_ok());
    /// assert!(u32_prison.guard_slice_ref(..=3).is_ok());
    /// assert!(u32_prison.guard_slice_ref(..).is_ok());
    /// u32_prison.remove_idx(2)?;
    /// assert!(u32_prison.guard_slice_ref(..).is_err());
    /// # Ok(())
    /// # }
    /// ```
    /// See [Prison::guard_many_ref_idx()] for more info
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_slice_ref<'a, R>(&'a self, range: R) -> Result<PrisonSliceRef<'a, T>, AccessError>
    where
        R: RangeBounds<usize>,
    {
        let (start, end) = extract_true_start_end(range, self.vec_len());
        let idxs: Vec<usize> = (start..end).into_iter().collect();
        return self.guard_many_ref_idx(&idxs);
    }

    //FN Prison::clone_val()
    /// Clones the requested value out of the [Prison] into a new variable
    ///
    /// Only available when elements of type T implement [Clone] (it is assumed that the implementation of `T::clone()` is memory safe).
    ///
    /// Because cloning does not alter the original, and because the new variable to hold the clone does not have any presumtions about the value, it
    /// is safe (in a single-threaded context) to clone out the value even if it is being visited or guarded.
    ///
    /// This method *will* still return an error if the index or generation of the [CellKey] are invalid
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<String> = Prison::new();
    /// let key_0 = prison.insert(String::from("Foo"))?;
    /// let key_1 = prison.insert(String::from("Bar"))?;
    /// let mut take_foo = String::new();
    /// let mut take_bar = String::new();
    /// prison.visit_mut(key_0, |val_0| {
    ///     take_foo = prison.clone_val(key_0)?;
    ///     Ok(())
    /// });
    /// let grd_1 = prison.guard_mut(key_1)?;
    /// take_bar = prison.clone_val(key_1)?;
    /// PrisonValueMut::unguard(grd_1);
    /// assert_eq!(take_foo, String::from("Foo"));
    /// assert_eq!(take_bar, String::from("Bar"));
    /// prison.remove(key_1)?;
    /// assert!(prison.clone_val(CellKey::from_raw_parts(10, 10)).is_err());
    /// assert!(prison.clone_val(key_1).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn clone_val(&self, key: CellKey) -> Result<T, AccessError>
    where
        T: Clone,
    {
        let internal = internal!(self);
        if key.idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(key.idx));
        }
        match &internal.vec[key.idx] {
            cell if cell.is_cell_and_gen_match(key.gen) => {
                return Ok(unsafe {cell.val.assume_init_ref().clone()});
            }
            _ => return Err(AccessError::ValueDeleted(key.idx, key.gen)),
        }
    }

    //FN Prison::clone_val_idx()
    /// Clones the requested value out of the [Prison] into a new variable
    ///
    /// Same as `clone_val()` but ignores the generation counter
    ///
    /// Only available when elements of type T implement [Clone] (it is assumed that the implementation of `T::clone()` is memory safe).
    ///
    /// Because cloning does not alter the original, and because the new variable to hold the clone does not have any presumtions about the value, it
    /// is safe (in a single-threaded context) to clone out the value even if it is being visited or guarded.
    ///
    /// This method *will* still return an error if the index is invalid or the value is free/deleted
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<String> = Prison::new();
    /// prison.insert(String::from("Foo"))?;
    /// prison.insert(String::from("Bar"))?;
    /// let mut take_foo = String::new();
    /// let mut take_bar = String::new();
    /// prison.visit_mut_idx(0, |val_0| {
    ///     take_foo = prison.clone_val_idx(0)?;
    ///     Ok(())
    /// });
    /// let grd_1 = prison.guard_mut_idx(1)?;
    /// take_bar = prison.clone_val_idx(1)?;
    /// PrisonValueMut::unguard(grd_1);
    /// assert_eq!(take_foo, String::from("Foo"));
    /// assert_eq!(take_bar, String::from("Bar"));
    /// prison.remove_idx(1)?;
    /// assert!(prison.clone_val_idx(10).is_err());
    /// assert!(prison.clone_val_idx(1).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn clone_val_idx(&self, idx: usize) -> Result<T, AccessError>
    where
        T: Clone,
    {
        let internal = internal!(self);
        if idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(idx));
        }
        match &internal.vec[idx] {
            cell if cell.is_cell() => {
                return Ok(unsafe {cell.val.assume_init_ref().clone()});
            }
            _ => return Err(AccessError::ValueDeleted(idx, 0)),
        }
    }

    //FN Prison::clone_many_vals()
    /// Clones the requested values out of the [Prison] into a new [Vec<T>]
    ///
    /// Only available when elements of type T implement [Clone] (it is assumed that the implementation of `T::clone()` is memory safe).
    ///
    /// Because cloning does not alter the originals, and because the new variables to hold the clones do not have any presumtions about the values, it
    /// is safe (in a single-threaded context) to clone out the values even if they are being visited or guarded.
    ///
    /// This method *will* still return an error if any index or generation of the [CellKey]s are invalid
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<String> = Prison::new();
    /// let key_0 = prison.insert(String::from("Foo"))?;
    /// let key_1 = prison.insert(String::from("Bar"))?;
    /// let mut take_foobar: Vec<String> = Vec::new();
    /// prison.visit_mut(key_0, |val_0| {
    ///     let grd_1 = prison.guard_mut(key_1)?;
    ///     take_foobar = prison.clone_many_vals(&[key_0, key_1])?;
    ///     PrisonValueMut::unguard(grd_1);
    ///     Ok(())
    /// });
    /// assert_eq!(take_foobar[0], String::from("Foo"));
    /// assert_eq!(take_foobar[1], String::from("Bar"));
    /// prison.remove(key_1)?;
    /// assert!(prison.clone_many_vals(&[CellKey::from_raw_parts(10, 10)]).is_err());
    /// assert!(prison.clone_many_vals(&[key_1]).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn clone_many_vals(&self, keys: &[CellKey]) -> Result<Vec<T>, AccessError>
    where
        T: Clone,
    {
        let mut vals = Vec::with_capacity(keys.len());
        for key in keys {
            vals.push(self.clone_val(*key)?);
        }
        return Ok(vals);
    }

    //FN Prison::clone_many_vals_idx()
    /// Clones the requested values out of the [Prison] into a new [Vec<T>]
    ///
    /// Same as `clone_many_vals()` but ignores the generation counter
    ///
    /// Only available when elements of type T implement [Clone] (it is assumed that the implementation of `T::clone()` is memory safe).
    ///
    /// Because cloning does not alter the originals, and because the new variables to hold the clones do not have any presumtions about the values, it
    /// is safe (in a single-threaded context) to clone out the values even if they are being visited or guarded.
    ///
    /// This method *will* still return an error if any index is out-of-range or free/deleted
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<String> = Prison::new();
    /// prison.insert(String::from("Foo"))?;
    /// prison.insert(String::from("Bar"))?;
    /// let mut take_foobar: Vec<String> = Vec::new();
    /// prison.visit_mut_idx(0, |val_0| {
    ///     let grd_1 = prison.guard_mut_idx(1)?;
    ///     take_foobar = prison.clone_many_vals_idx(&[0, 1])?;
    ///     PrisonValueMut::unguard(grd_1);
    ///     Ok(())
    /// });
    /// assert_eq!(take_foobar[0], String::from("Foo"));
    /// assert_eq!(take_foobar[1], String::from("Bar"));
    /// prison.remove_idx(1)?;
    /// assert!(prison.clone_many_vals_idx(&[10]).is_err());
    /// assert!(prison.clone_many_vals_idx(&[1]).is_err());
    /// # Ok(())
    /// # }
    /// ```
    pub fn clone_many_vals_idx(&self, indexes: &[usize]) -> Result<Vec<T>, AccessError>
    where
        T: Clone,
    {
        let mut vals = Vec::with_capacity(indexes.len());
        for idx in indexes {
            vals.push(self.clone_val_idx(*idx)?);
        }
        return Ok(vals);
    }

    //REGION Prison Private
    //FN Prison::_add_mut_ref()
    #[doc(hidden)]
    fn _add_mut_ref(
        &self,
        idx: usize,
        gen: usize,
        use_gen: bool,
    ) -> Result<(&mut PrisonCell<T>, &mut usize), AccessError> {
        let internal = internal!(self);
        if idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(idx));
        }
        match &mut internal.vec[idx] {
            cell if cell.is_cell_and_gen_match_opt(gen, use_gen) => {
                if cell.refs_or_next == Refs::MUT {
                    return Err(AccessError::ValueAlreadyMutablyReferenced(idx));
                }
                if cell.refs_or_next > 0 {
                    return Err(AccessError::ValueStillImmutablyReferenced(idx));
                }
                cell.refs_or_next = Refs::MUT;
                internal.access_count += 1;
                return Ok((cell, &mut internal.access_count));
            },
            _ => return Err(AccessError::ValueDeleted(idx, gen)),
        }
    }

    //FN Prison::_add_imm_ref()
    #[doc(hidden)]
    fn _add_imm_ref(
        &self,
        idx: usize,
        gen: usize,
        use_gen: bool,
    ) -> Result<(&mut PrisonCell<T>, &mut usize), AccessError> {
        let internal = internal!(self);
        if idx >= internal.vec.len() {
            return Err(AccessError::IndexOutOfRange(idx));
        }
        match &mut internal.vec[idx] {
            cell if cell.is_cell_and_gen_match_opt(gen, use_gen) => {
                if cell.refs_or_next == Refs::MUT {
                    return Err(AccessError::ValueAlreadyMutablyReferenced(idx));
                }
                if cell.refs_or_next == Refs::MAX_IMMUT {
                    return Err(AccessError::MaximumImmutableReferencesReached(idx));
                }
                if cell.refs_or_next == 0 {
                    internal.access_count += 1;
                }
                cell.refs_or_next += 1;
                return Ok((cell, &mut internal.access_count));
            },
            _ => return Err(AccessError::ValueDeleted(idx, gen)),
        }
    }

    //FN Prison::_add_many_mut_refs()
    #[doc(hidden)]
    fn _add_many_mut_refs(
        &self,
        cell_keys: &[CellKey],
    ) -> Result<(Vec<&mut T>, Vec<&mut usize>, &mut usize), AccessError> {
        let internal = internal!(self);
        let mut vals = Vec::new();
        let mut refs = Vec::new();
        let mut ref_all_result = Ok(());
        for key in cell_keys {
            let ref_result = self._add_mut_ref(key.idx, key.gen, true);
            match ref_result {
                Ok((cell, _)) => {
                    vals.push(unsafe { cell.val.assume_init_mut() });
                    refs.push(&mut cell.refs_or_next);
                }
                Err(e) => {
                    ref_all_result = Err(e);
                    break;
                }
            }
        }
        match ref_all_result {
            Ok(_) => {
                return Ok((vals, refs, &mut internal.access_count));
            }
            Err(acc_err) => {
                _remove_many_mut_refs(&mut refs, &mut internal.access_count);
                return Err(acc_err);
            }
        }
    }

    //FN Prison::_add_many_mut_refs_idx()
    #[doc(hidden)]
    fn _add_many_mut_refs_idx(
        &self,
        idxs: &[usize],
    ) -> Result<(Vec<&mut T>, Vec<&mut usize>, &mut usize), AccessError> {
        let internal = internal!(self);
        let mut vals = Vec::new();
        let mut refs = Vec::new();
        let mut ref_all_result = Ok(());
        for idx in idxs {
            let ref_result = self._add_mut_ref(*idx, 0, false);
            match ref_result {
                Ok((cell, _)) => {
                    vals.push(unsafe { cell.val.assume_init_mut() });
                    refs.push(&mut cell.refs_or_next);
                }
                Err(e) => {
                    ref_all_result = Err(e);
                    break;
                }
            }
        }
        match ref_all_result {
            Ok(_) => {
                return Ok((vals, refs, &mut internal.access_count));
            }
            Err(acc_err) => {
                _remove_many_mut_refs(&mut refs, &mut internal.access_count);
                return Err(acc_err);
            }
        }
    }

    //FN Prison::_add_many_imm_refs()
    #[doc(hidden)]
    fn _add_many_imm_refs(
        &self,
        cell_keys: &[CellKey],
    ) -> Result<(Vec<&T>, Vec<&mut usize>, &mut usize), AccessError> {
        let internal = internal!(self);
        let mut vals = Vec::new();
        let mut refs = Vec::new();
        let mut ref_all_result = Ok(());
        for key in cell_keys {
            let ref_result = self._add_imm_ref(key.idx, key.gen, true);
            match ref_result {
                Ok((cell, _)) => {
                    vals.push(unsafe { cell.val.assume_init_ref() });
                    refs.push(&mut cell.refs_or_next);
                }
                Err(e) => {
                    ref_all_result = Err(e);
                    break;
                }
            }
        }
        match ref_all_result {
            Ok(_) => {
                return Ok((vals, refs, &mut internal.access_count));
            }
            Err(acc_err) => {
                _remove_many_imm_refs(&mut refs, &mut internal.access_count);
                return Err(acc_err);
            }
        }
    }

    //FN Prison::_add_many_imm_refs_idx()
    #[doc(hidden)]
    fn _add_many_imm_refs_idx(
        &self,
        idxs: &[usize],
    ) -> Result<(Vec<&T>, Vec<&mut usize>, &mut usize), AccessError> {
        let internal = internal!(self);
        let mut vals = Vec::new();
        let mut refs = Vec::new();
        let mut ref_all_result = Ok(());
        for idx in idxs {
            let ref_result = self._add_imm_ref(*idx, 0, false);
            match ref_result {
                Ok((cell, _)) => {
                    vals.push(unsafe { cell.val.assume_init_ref() });
                    refs.push(&mut cell.refs_or_next);
                }
                Err(e) => {
                    ref_all_result = Err(e);
                    break;
                }
            }
        }
        match ref_all_result {
            Ok(_) => {
                return Ok((vals, refs, &mut internal.access_count));
            }
            Err(acc_err) => {
                _remove_many_imm_refs(&mut refs, &mut internal.access_count);
                return Err(acc_err);
            }
        }
    }
}



//FN _remove_mut_ref()
#[doc(hidden)]
#[inline(always)]
fn _remove_mut_ref(refs: &mut usize, accesses: &mut usize) {
    *refs = 0;
    *accesses -= 1;
}

//FN _remove_imm_ref()
#[doc(hidden)]
#[inline(always)]
fn _remove_imm_ref(refs: &mut usize, accesses: &mut usize) {
    *refs -= 1;
    if *refs == 0 {
        *accesses -= 1
    }
}

//FN _remove_many_mut_refs()
#[doc(hidden)]
#[inline(always)]
fn _remove_many_mut_refs(refs_list: &mut [&mut usize], accesses: &mut usize) {
    for refs in refs_list {
        _remove_mut_ref(refs, accesses)
    }
}

//FN _remove_many_imm_refs()
#[doc(hidden)]
#[inline(always)]
fn _remove_many_imm_refs(refs_list: &mut [&mut usize], accesses: &mut usize) {
    for refs in refs_list {
        _remove_imm_ref(refs, accesses)
    }
}

//IMPL Default for Prison
impl<T> Default for Prison<T> {
    fn default() -> Self {
        Self::new()
    }
}

//STRUCT PrisonInternal
#[doc(hidden)]
#[derive(Debug)]
struct PrisonMutable<T> {
    access_count: usize,
    generation: usize,
    free_count: usize,
    next_free: usize,
    vec: Vec<PrisonCell<T>>,
}

//STRUCT PrisonCell
#[doc(hidden)]
#[derive(Debug)]
struct PrisonCell<T> {
    refs_or_next: usize,
    d_gen_or_prev: usize,
    val: MaybeUninit<T>,
}

//IMPL Drop for PrisonCell
impl<T> Drop for PrisonCell<T> {
    fn drop(&mut self) {
        if self.is_cell() {
            unsafe {self.val.assume_init_drop()}
        }
    }
}

impl<T> PrisonCell<T> {
    #[inline(always)]
    fn is_cell_and_gen_match_opt(&self, gen: usize, use_gen: bool) -> bool {
        IdxD::is_type_a(self.d_gen_or_prev) && (!use_gen || IdxD::val(self.d_gen_or_prev) == gen)
    }
    #[inline(always)]
    fn is_cell_and_gen_match(&self, gen: usize) -> bool {
        IdxD::is_type_a(self.d_gen_or_prev) && IdxD::val(self.d_gen_or_prev) == gen
    }
    #[inline(always)]
    fn is_cell(&self) -> bool {
        IdxD::is_type_a(self.d_gen_or_prev)
    }
    #[inline(always)]
    fn is_free(&self) -> bool {
        IdxD::is_type_b(self.d_gen_or_prev)
    }

    fn new_cell(val: T, gen: usize) -> PrisonCell<T> {
        PrisonCell { refs_or_next: 0, d_gen_or_prev: IdxD::new_type_a(gen), val: MaybeUninit::new(val) }
    }

    fn make_free_unchecked(&mut self, next: usize, prev: usize) -> T {
        self.d_gen_or_prev = IdxD::new_type_b(prev);
        self.refs_or_next = next;
        unsafe { mem_replace(&mut self.val, MaybeUninit::uninit()).assume_init() }
    }

    fn make_cell_unchecked(&mut self, val: T, gen: usize) {
        self.d_gen_or_prev = IdxD::new_type_a(gen);
        self.refs_or_next = 0;
        self.val = MaybeUninit::new(val);
    }

    fn overwrite_cell_unchecked(&mut self, val: T, gen: usize) {
        self.d_gen_or_prev = IdxD::new_type_a(gen);
        self.refs_or_next = 0;
        unsafe { self.val.assume_init_drop() };
        self.val = MaybeUninit::new(val);
    }
}

//REGION Guarded Prison Values
//STRUCT PrisonValueMut
/// Struct representing a mutable reference to a value that has been allowed to leave the
/// [Prison] temporarily, but remains guarded by a wrapper to prevent it from leaking or never unlocking
///
/// [PrisonValueMut<T>] implements [Deref<Target = T>], [DerefMut<Target = T>], [AsRef<T>], [AsMut<T>],
/// [Borrow<T>], and [BorrowMut<T>] to allow transparent access to its underlying value
///
/// As long as the [PrisonValueMut] remains in scope, the element where it's value resides in the
/// [Prison] will remain marked as mutably referenced and unable to be referenced a second time.
/// You can manually drop the [PrisonValueMut] out of scope by passing it as the first parameter
/// to the function [PrisonValueMut::unguard(p_val_mut)]
///
/// You can obtain a [PrisonValueMut] by calling `guard_mut()` or `guard_mut_idx()` on a [Prison]
/// ### Example
/// ```rust
/// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
/// # fn main() -> Result<(), AccessError> {
/// let prison: Prison<u32> = Prison::new();
/// let key_0 = prison.insert(10)?;
/// let mut grd_0 = prison.guard_mut(key_0)?;
/// assert_eq!(*grd_0, 10);
/// *grd_0 = 20;
/// PrisonValueMut::unguard(grd_0);
/// prison.visit_ref(key_0, |val_0| {
///     assert_eq!(*val_0, 20);
///     Ok(())
/// });
/// # Ok(())
/// # }
/// ```
pub struct PrisonValueMut<'a, T> {
    cell: &'a mut PrisonCell<T>,
    prison_accesses: &'a mut usize,
}

impl<'a, T> PrisonValueMut<'a, T> {
    //FN PrisonValueMut::unguard()
    /// Manually end a [PrisonValueMut] value's temporary guarded absence from the [Prison]
    ///
    /// This method simply takes ownership of the [PrisonValueMut] and immediately lets it go out of scope,
    /// causing it's `drop()` method to be called and clearing its mutable reference in the [Prison]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// let grd_0 = prison.guard_mut_idx(0)?;
    /// // index 0 CANNOT be accessed here because it is being guarded outside the prison
    /// assert!(prison.visit_ref_idx(0, |ref_0| Ok(())).is_err());
    /// PrisonValueMut::unguard(grd_0);
    /// // index 0 CAN be accessed here because it was returned to the prison
    /// assert!(prison.visit_ref_idx(0, |ref_0| Ok(())).is_ok());
    /// # Ok(())
    /// # }
    /// ```
    pub fn unguard(_prison_val_mut: Self) {}
}

//IMPL Drop for PrisonValueMut
impl<'a, T> Drop for PrisonValueMut<'a, T> {
    fn drop(&mut self) {
        _remove_mut_ref(&mut self.cell.refs_or_next, self.prison_accesses)
    }
}

//IMPL Deref for PrisonValueMut
impl<'a, T> Deref for PrisonValueMut<'a, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe {self.cell.val.assume_init_ref()}
    }
}

//IMPL DerefMut for PrisonValueMut
impl<'a, T> DerefMut for PrisonValueMut<'a, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe {self.cell.val.assume_init_mut()}
    }
}

//IMPL AsRef for PrisonValueMut
impl<'a, T> AsRef<T> for PrisonValueMut<'a, T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        unsafe {self.cell.val.assume_init_ref()}
    }
}

//IMPL AsMut for PrisonValueMut
impl<'a, T> AsMut<T> for PrisonValueMut<'a, T> {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut T {
        unsafe {self.cell.val.assume_init_mut()}
    }
}

//IMPL Borrow for PrisonValueMut
impl<'a, T> Borrow<T> for PrisonValueMut<'a, T> {
    #[inline(always)]
    fn borrow(&self) -> &T {
        unsafe {self.cell.val.assume_init_ref()}
    }
}

//IMPL BorrowMut for PrisonValueMut
impl<'a, T> BorrowMut<T> for PrisonValueMut<'a, T> {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut T {
        unsafe {self.cell.val.assume_init_mut()}
    }
}

//STRUCT PrisonValueRef
/// Struct representing an immutable reference to a value that has been allowed to leave the
/// [Prison] temporarily, but remains guarded by a wrapper to prevent it from leaking or never unlocking
///
/// [PrisonValueRef<T>] implements [Deref<Target = T>], [AsRef<T>], and [Borrow<T>]
/// to allow transparent access to its underlying value
///
/// As long as the [PrisonValueRef] remains in scope, the element where it's value resides in the
/// [Prison] will remain marked as immutably referenced and unable to be mutably referenced.
/// You can manually drop the [PrisonValueRef] out of scope by passing it as the first parameter
/// to the function [PrisonValueRef::unguard(p_val_ref)]
///
/// You can obtain a [PrisonValueRef] by calling `guard_ref()` or `guard_ref_idx()` on a [Prison]
/// ### Example
/// ```rust
/// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef}};
/// # fn main() -> Result<(), AccessError> {
/// let prison: Prison<u32> = Prison::new();
/// let key_0 = prison.insert(10)?;
/// let mut grd_0 = prison.guard_ref(key_0)?;
/// assert_eq!(*grd_0, 10);
/// prison.visit_ref(key_0, |val_0| {
///     assert_eq!(*val_0, 10);
///     Ok(())
/// });
/// PrisonValueRef::unguard(grd_0);
/// # Ok(())
/// # }
/// ```
pub struct PrisonValueRef<'a, T> {
    cell: &'a mut PrisonCell<T>,
    prison_accesses: &'a mut usize,
}

impl<'a, T> PrisonValueRef<'a, T> {
    //FN PrisonValueRef::unguard()
    /// Manually end a [PrisonValueRef] value's temporary guarded absence from the [Prison]
    ///
    /// This method simply takes ownership of the [PrisonValueRef] and immediately lets it go out of scope,
    /// causing it's `drop()` method to be called and decreasing its immutable reference count in the [Prison]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// let grd_0 = prison.guard_ref_idx(0)?;
    /// // index 0 CANNOT be accessed here because it is being guarded outside the prison
    /// assert!(prison.visit_mut_idx(0, |ref_0| Ok(())).is_err());
    /// PrisonValueRef::unguard(grd_0);
    /// // index 0 CAN be accessed here because it was returned to the prison
    /// assert!(prison.visit_mut_idx(0, |ref_0| Ok(())).is_ok());
    /// # Ok(())
    /// # }
    /// ```
    pub fn unguard(_prison_val_ref: Self) {}
}

//IMPL Drop for PrisonValueRef
impl<'a, T> Drop for PrisonValueRef<'a, T> {
    fn drop(&mut self) {
        _remove_imm_ref(&mut self.cell.refs_or_next, self.prison_accesses)
    }
}

//IMPL Deref for PrisonValueRef
impl<'a, T> Deref for PrisonValueRef<'a, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        unsafe {self.cell.val.assume_init_ref()}
    }
}

//IMPL AsRef for PrisonValueRef
impl<'a, T> AsRef<T> for PrisonValueRef<'a, T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        unsafe {self.cell.val.assume_init_ref()}
    }
}

//IMPL Borrow for PrisonValueRef
impl<'a, T> Borrow<T> for PrisonValueRef<'a, T> {
    #[inline(always)]
    fn borrow(&self) -> &T {
        unsafe {self.cell.val.assume_init_ref()}
    }
}

//STRUCT PrisonSliceMut
/// Struct representing a slice of mutable references to values that have been allowed to leave the
/// [Prison] temporarily, but remain guarded by a wrapper to prevent them from leaking or never unlocking
///
/// [PrisonSliceMut<T>] implements [Deref<Target = \[&mut T\]>](Deref), [DerefMut<Target = \[&mut T\]>](DerefMut), [AsRef<\[&mut T\]>](AsRef), [AsMut<\[&mut T\]>](AsMut),
/// [Borrow<\[&mut T\]>](Borrow), and [BorrowMut<\[&mut T\]>](BorrowMut) to allow transparent access to its underlying slice of values
///
/// As long as the [PrisonSliceMut] remains in scope, the elements where it's values reside in the
/// [Prison] will remain marked as mutably referenced and unable to be referenced a second time.
/// You can manually drop the [PrisonSliceMut] out of scope by passing it as the first parameter
/// to the function [PrisonSliceMut::unguard(p_sli_mut)]
///
/// You can obtain a [PrisonSliceMut] by calling `guard_many_mut()` or `guard_many_mut_idx()` on a [Prison]
/// ### Example
/// ```rust
/// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceMut}};
/// # fn main() -> Result<(), AccessError> {
/// let prison: Prison<u32> = Prison::new();
/// let key_0 = prison.insert(10)?;
/// let key_1 = prison.insert(20)?;
/// let key_2 = prison.insert(30)?;
/// let mut grd_0_1_2 = prison.guard_many_mut(&[key_0, key_1, key_2])?;
/// assert_eq!(*grd_0_1_2[1], 20);
/// *grd_0_1_2[1] = 42;
/// PrisonSliceMut::unguard(grd_0_1_2);
/// prison.visit_ref(key_1, |val_1| {
///     assert_eq!(*val_1, 42);
///     Ok(())
/// });
/// # Ok(())
/// # }
/// ```
pub struct PrisonSliceMut<'a, T> {
    prison_accesses: &'a mut usize,
    refs: Vec<&'a mut usize>,
    vals: Vec<&'a mut T>,
}

impl<'a, T> PrisonSliceMut<'a, T> {
    //FN PrisonSliceMut::unguard()
    /// Manually end a [PrisonSliceMut] value's temporary guarded absence from the [Prison]
    ///
    /// This method simply takes ownership of the [PrisonSliceMut] and immediately lets it go out of scope,
    /// causing it's `drop()` method to be called and decreasing its immutable reference count in the [Prison]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// let grd_all = prison.guard_many_mut_idx(&[0, 1])?;
    /// assert!(prison.visit_many_mut_idx(&[0, 1], |ref_all| Ok(())).is_err());
    /// PrisonSliceMut::unguard(grd_all);
    /// assert!(prison.visit_many_mut_idx(&[0, 1], |ref_all| Ok(())).is_ok());
    /// # Ok(())
    /// # }
    /// ```
    pub fn unguard(_prison_sli_mut: Self) {}
}

//IMPL Drop for PrisonSliceMut
impl<'a, T> Drop for PrisonSliceMut<'a, T> {
    fn drop(&mut self) {
        _remove_many_mut_refs(&mut self.refs, self.prison_accesses)
    }
}

//IMPL Deref for PrisonSliceMut
impl<'a, T> Deref for PrisonSliceMut<'a, T> {
    type Target = [&'a mut T];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.vals.as_slice()
    }
}

//IMPL DerefMut for PrisonSliceMut
impl<'a, T> DerefMut for PrisonSliceMut<'a, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.vals.as_mut_slice()
    }
}

//IMPL AsRef for PrisonSliceMut
impl<'a, T> AsRef<[&'a mut T]> for PrisonSliceMut<'a, T> {
    #[inline(always)]
    fn as_ref(&self) -> &[&'a mut T] {
        self.vals.as_slice()
    }
}

//IMPL AsMut for PrisonSliceMut
impl<'a, T> AsMut<[&'a mut T]> for PrisonSliceMut<'a, T> {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut [&'a mut T] {
        self.vals.as_mut_slice()
    }
}

//IMPL Borrow for PrisonSliceMut
impl<'a, T> Borrow<[&'a mut T]> for PrisonSliceMut<'a, T> {
    #[inline(always)]
    fn borrow(&self) -> &[&'a mut T] {
        self.vals.as_slice()
    }
}

//IMPL BorrowMut for PrisonSliceMut
impl<'a, T> BorrowMut<[&'a mut T]> for PrisonSliceMut<'a, T> {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut [&'a mut T] {
        self.vals.as_mut_slice()
    }
}

//STRUCT PrisonSliceRef
/// Struct representing a slice of immutable references to values that have been allowed to leave the
/// [Prison] temporarily, but remain guarded by a wrapper to prevent them from leaking or never unlocking
///
/// [PrisonSliceRef<T>] implements [Deref<Target = \[&T\]>](Deref), [AsRef<\[&T\]>](AsRef),
/// and [Borrow<\[&T\]>](Borrow) to allow transparent access to its underlying slice of values
///
/// As long as the [PrisonSliceRef] remains in scope, the elements where it's values reside in the
/// [Prison] will remain marked as immutably referenced and unable to be mutably referenced.
/// You can manually drop the [PrisonSliceRef] out of scope by passing it as the first parameter
/// to the function [PrisonSliceRef::unguard(p_sli_ref)]
///
/// You can obtain a [PrisonSliceRef] by calling `guard_many_ref()` or `guard_many_ref_idx()` on a [Prison]
/// ### Example
/// ```rust
/// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceRef}};
/// # fn main() -> Result<(), AccessError> {
/// let prison: Prison<u32> = Prison::new();
/// let key_0 = prison.insert(10)?;
/// let key_1 = prison.insert(20)?;
/// let key_2 = prison.insert(30)?;
/// let mut grd_0_1_2 = prison.guard_many_ref(&[key_0, key_1, key_2])?;
/// assert_eq!(*grd_0_1_2[1], 20);
/// prison.visit_ref(key_1, |val_1| {
///     assert_eq!(*val_1, 20);
///     Ok(())
/// });
/// PrisonSliceRef::unguard(grd_0_1_2);
/// # Ok(())
/// # }
/// ```
pub struct PrisonSliceRef<'a, T> {
    prison_accesses: &'a mut usize,
    refs: Vec<&'a mut usize>,
    vals: Vec<&'a T>,
}

impl<'a, T> PrisonSliceRef<'a, T> {
    //FN PrisonSliceRef::unguard()
    /// Manually end a [PrisonSliceRef] value's temporary guarded absence from the [Prison]
    ///
    /// This method simply takes ownership of the [PrisonSliceRef] and immediately lets it go out of scope,
    /// causing it's `drop()` method to be called and decreasing its immutable reference count in the [Prison]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonSliceRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let prison: Prison<u32> = Prison::new();
    /// prison.insert(10)?;
    /// prison.insert(20)?;
    /// let grd_all = prison.guard_many_ref_idx(&[0, 1])?;
    /// assert!(prison.visit_many_mut_idx(&[0, 1], |ref_all| Ok(())).is_err());
    /// PrisonSliceRef::unguard(grd_all);
    /// assert!(prison.visit_many_mut_idx(&[0, 1], |ref_all| Ok(())).is_ok());
    /// # Ok(())
    /// # }
    /// ```
    pub fn unguard(_prison_sli_ref: Self) {}
}

//IMPL Drop for PrisonSliceRef
impl<'a, T> Drop for PrisonSliceRef<'a, T> {
    fn drop(&mut self) {
        _remove_many_imm_refs(&mut self.refs, self.prison_accesses)
    }
}

//IMPL Deref for PrisonSliceRef
impl<'a, T> Deref for PrisonSliceRef<'a, T> {
    type Target = [&'a T];

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.vals.as_slice()
    }
}

//IMPL AsRef for PrisonSliceRef
impl<'a, T> AsRef<[&'a T]> for PrisonSliceRef<'a, T> {
    #[inline(always)]
    fn as_ref(&self) -> &[&'a T] {
        self.vals.as_slice()
    }
}

//IMPL Borrow for PrisonSliceRef
impl<'a, T> Borrow<[&'a T]> for PrisonSliceRef<'a, T> {
    #[inline(always)]
    fn borrow(&self) -> &[&'a T] {
        self.vals.as_slice()
    }
}

//REGION JailCell
//STRUCT JailCell
/// Represents a single standalone value that allows interior mutability while upholding memory safety
/// with a reference counting [usize]
///
/// This is a very simple implementation of the principles found in [Prison]
///
/// It has a single [UnsafeCell] to allow interior mutability. The [UnsafeCell] holds
/// one single [usize] to track mutable and immutable references, and the value itself
/// of type `T`
///
/// It has `visit_ref()`, `visit_mut()`, `guard_ref()`, and `guard_mut()` methods, just like [Prison],
/// but with drastically simpler requirements for safety checking.
/// ### Example
/// ```rust
/// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueRef}};
/// # fn main() -> Result<(), AccessError> {
/// let string_jail: JailCell<String> = JailCell::new(String::from("'Bad-Guy' Bert"));
/// string_jail.visit_mut(|criminal| {
///     let bigger_bad = String::from("Dr. Lego-Step");
///     println!("Breaking News: {} to be set free to make room for {}", *criminal, bigger_bad);
///     *criminal = bigger_bad;
///     Ok(())
/// })?;
/// let guarded_criminal = string_jail.guard_ref()?;
/// println!("{} will now be paraded around town for public shaming", *guarded_criminal);
/// assert_eq!(*guarded_criminal, String::from("Dr. Lego-Step"));
/// JailValueRef::unguard(guarded_criminal);
/// # Ok(())
/// # }
/// ```
pub struct JailCell<T> {
    internal: UnsafeCell<JailCellMutable<T>>,
}

impl<T> JailCell<T> {
    //FN JailCell::new()
    /// Creates a new [JailCell] with the supplied value of type `T`
    ///
    /// After creation, mutable or immutable references to it's value can only be obtained
    /// through its `visit_*()` or `guard_*()` methods
    pub fn new(value: T) -> JailCell<T> {
        return JailCell {
            internal: UnsafeCell::new(JailCellMutable {
                refs: 0,
                val: value,
            }),
        };
    }

    //FN JailCell::visit_mut()
    /// Obtain a mutable reference to the [JailCell]'s internal value that gets passed to
    /// a closure you provide.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell}};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_jail: JailCell<String> = JailCell::new(String::from("'Bad-Guy' Bert"));
    /// string_jail.visit_mut(|criminal| {
    ///     let bigger_bad = String::from("Dr. Lego-Step");
    ///     println!("Breaking News: {} to be set free to make room for {}", *criminal, bigger_bad);
    ///     *criminal = bigger_bad;
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(0)] if value is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(0)] if value has any number of immutable references
    ///  ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell}};
    /// # fn main() -> Result<(), AccessError> {
    /// let jail: JailCell<u32> = JailCell::new(42);
    /// jail.visit_mut(|val| {
    ///     assert!(jail.visit_mut(|val| Ok(())).is_err());
    ///     Ok(())
    /// })?;
    /// jail.visit_ref(|val| {
    ///     assert!(jail.visit_mut(|val| Ok(())).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn visit_mut<F>(&self, mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&mut T) -> Result<(), AccessError>,
    {
        let internal = internal!(self);
        internal.add_ref_internal(true)?;
        let result = operation(&mut internal.val);
        internal.remove_ref_internal();
        return result;
    }

    //FN JailCell::visit_ref()
    /// Obtain an immutable reference to the [JailCell]'s internal value that gets passed to
    /// a closure you provide.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell}};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_jail: JailCell<String> = JailCell::new(String::from("'Bad-Guy' Bert"));
    /// string_jail.visit_ref(|criminal| {
    ///     println!("Breaking News: {} was just captured!", *criminal);
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(0)] if value is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(0)] if value has usize::MAX - 2 immutable references already
    ///  ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell}};
    /// # fn main() -> Result<(), AccessError> {
    /// let jail: JailCell<u32> = JailCell::new(42);
    /// jail.visit_mut(|val| {
    ///     assert!(jail.visit_ref(|val| Ok(())).is_err());
    ///     Ok(())
    /// })?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn visit_ref<F>(&self, mut operation: F) -> Result<(), AccessError>
    where
        F: FnMut(&T) -> Result<(), AccessError>,
    {
        let internal = internal!(self);
        internal.add_ref_internal(false)?;
        let result = operation(&internal.val);
        internal.remove_ref_internal();
        return result;
    }

    //FN JailCell::guard_mut()
    /// Obtain an [JailValueMut] that marks the [JailCell] mutably referenced as long as it remains
    /// in scope and automatically unlocks it when it falls out of scope
    ///
    /// [JailValueMut<T>] implements [Deref<Target = T>], [DerefMut<Target = T>], [AsRef<T>], [AsMut<T>],
    /// [Borrow<T>], and [BorrowMut<T>] to allow transparent access to its underlying value
    ///
    /// You may manually drop the [JailValueMut] out of scope by passing it to the function
    /// [JailValueMut::unguard(_jail_val_mut)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_jail: JailCell<String> = JailCell::new(String::from("'Bad-Guy' Bert"));
    /// let mut grd_criminal = string_jail.guard_mut()?;
    /// let bigger_bad = String::from("Dr. Lego-Step");
    /// println!("Breaking News: {} to be set free to make room for {}", *grd_criminal, bigger_bad);
    /// *grd_criminal = bigger_bad;
    /// JailValueMut::unguard(grd_criminal);
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(0)] if value is already mutably referenced
    /// - [AccessError::ValueStillImmutablyReferenced(0)] if value has any number of immutable references
    ///  ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueRef, JailValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let jail: JailCell<u32> = JailCell::new(42);
    /// let guard_ref = jail.guard_ref()?;
    /// assert!(jail.guard_mut().is_err());
    /// JailValueRef::unguard(guard_ref);
    /// let guard_mut = jail.guard_mut()?;
    /// assert!(jail.guard_mut().is_err());
    /// JailValueMut::unguard(guard_mut);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_mut<'a>(&'a self) -> Result<JailValueMut<'a, T>, AccessError> {
        let internal = internal!(self);
        internal.add_ref_internal(true)?;
        return Ok(JailValueMut {
            ref_internal: internal,
        });
    }

    //FN JailCell::guard_ref()
    /// Obtain an [JailValueRef] that marks the [JailCell] mutably referenced as long as it remains
    /// in scope and automatically unlocks it when it falls out of scope
    ///
    /// [JailValueRef<T>] implements [Deref<Target = T>], [AsRef<T>], and [Borrow<T>]
    /// to allow transparent access to its underlying value
    ///
    /// You may manually drop the [JailValueRef] out of scope by passing it to the function
    /// [JailValueRef::unguard(_jail_val_ref)]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let string_jail: JailCell<String> = JailCell::new(String::from("'Bad-Guy' Bert"));
    /// let grd_criminal = string_jail.guard_ref()?;
    /// println!("Breaking News: {} has been captured!", *grd_criminal);
    /// JailValueRef::unguard(grd_criminal);
    /// # Ok(())
    /// # }
    /// ```
    /// ## Errors
    /// - [AccessError::ValueAlreadyMutablyReferenced(0)] if value is already mutably referenced
    /// - [AccessError::MaximumImmutableReferencesReached(0)] if value has usize::MAX - 2 immutable references already
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let jail: JailCell<u32> = JailCell::new(42);
    /// let guard_mut = jail.guard_mut()?;
    /// assert!(jail.guard_ref().is_err());
    /// JailValueMut::unguard(guard_mut);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use = "guarded reference will immediately fall out of scope"]
    pub fn guard_ref<'a>(&'a self) -> Result<JailValueRef<'a, T>, AccessError> {
        let internal = internal!(self);
        internal.add_ref_internal(false)?;
        return Ok(JailValueRef {
            ref_internal: internal,
        });
    }
    //FN JailCell::clone_val()
    /// Clones the requested value out of the [JailCell] into a new variable
    ///
    /// Only available when type T implements [Clone] (it is assumed that the implementation of `T::clone()` is memory safe).
    ///
    /// Because cloning does not alter the original, and because the new variable to hold the clone does not have any presumtions about the value, it
    /// is safe (in a single-threaded context) to clone out the value even if it is being visited or guarded.
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let jail: JailCell<String> = JailCell::new(String::from("Dolly"));
    /// let guard_mut = jail.guard_mut()?;
    /// let dolly_2 = jail.clone_val();
    /// JailValueMut::unguard(guard_mut);
    /// assert_eq!(dolly_2, String::from("Dolly"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn clone_val(&self) -> T
    where
        T: Clone,
    {
        internal!(self).val.clone()
    }
}

//IMPL Default for JailCell
impl<T> Default for JailCell<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::new(T::default())
    }
}

//STRUCT JailCellMutable
#[doc(hidden)]
struct JailCellMutable<T> {
    refs: usize,
    val: T,
}

impl<T> JailCellMutable<T> {
    //FN JailCellMutable::add_ref_internal()
    fn add_ref_internal(&mut self, mutable: bool) -> Result<(), AccessError> {
        if self.refs == Refs::MUT {
            return Err(AccessError::ValueAlreadyMutablyReferenced(0));
        }
        if mutable && self.refs > 0{
            return Err(AccessError::ValueStillImmutablyReferenced(0));
        }
        if self.refs == Refs::MAX_IMMUT {
            return Err(AccessError::MaximumImmutableReferencesReached(0));
        }
        if mutable {
            self.refs = Refs::MUT;
        } else {
            self.refs += 1;
        }
        return Ok(());
    }

    //FN JailCellMutable::remove_ref_internal()
    fn remove_ref_internal(&mut self) {
        if self.refs == Refs::MUT {
            self.refs = 0;
        } else if self.refs > 0 {
            self.refs -= 1;
        }
    }
}

//REGION Guarded JailCell Values
//STRUCT JailValueMut
/// A guarded wrapper around a mutable reference to the value contained in a [JailCell]
///
/// [JailValueMut<T>] implements [Deref<Target = T>], [DerefMut<Target = T>], [AsRef<T>], [AsMut<T>],
/// [Borrow<T>], and [BorrowMut<T>] to allow transparent access to its underlying value
///
/// As long as the [JailValueMut] remains in scope, the value in [JailCell] will
/// remain marked as mutably referenced and unable to be referenced a second time.
/// You can manually drop the [JailValueMut] out of scope by passing it as the first parameter
/// to the function [JailValueMut::unguard(jail_val_mut)]
///
/// You can obtain a [JailValueMut] by calling `guard_mut()` on a [JailCell]
/// ### Example
/// ```rust
/// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueMut}};
/// # fn main() -> Result<(), AccessError> {
/// let jail: JailCell<u32> = JailCell::new(42);
/// let mut grd_mut = jail.guard_mut()?;
/// assert_eq!(*grd_mut, 42);
/// *grd_mut = 69;
/// JailValueMut::unguard(grd_mut);
/// jail.visit_ref(|val| {
///     assert_eq!(*val, 69);
///     Ok(())
/// });
/// # Ok(())
/// # }
/// ```
pub struct JailValueMut<'a, T> {
    ref_internal: &'a mut JailCellMutable<T>,
}

impl<'a, T> JailValueMut<'a, T> {
    //FN JailValueMut::unguard()
    /// Manually end a [JailValueMut] value's temporary guarded absence from the [JailCell]
    ///
    /// This method simply takes ownership of the [JailValueMut] and immediately lets it go out of scope,
    /// causing it's `drop()` method to be called and clearing its mutable reference in the [JailCell]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueMut}};
    /// # fn main() -> Result<(), AccessError> {
    /// let jail: JailCell<u32> = JailCell::new(42);
    /// let grd_mut = jail.guard_mut()?;
    /// // val CANNOT be referenced again because the mutable reference is still in scope
    /// assert!(jail.visit_ref(|val| Ok(())).is_err());
    /// JailValueMut::unguard(grd_mut);
    /// // val CAN be referenced again because the mutable reference was dropped
    /// assert!(jail.visit_ref(|val| Ok(())).is_ok());
    /// # Ok(())
    /// # }
    /// ```
    pub fn unguard(_guarded_jail_value: JailValueMut<'a, T>) {}
}

//IMPL Drop for JailValueMut
impl<'a, T> Drop for JailValueMut<'a, T> {
    fn drop(&mut self) {
        self.ref_internal.remove_ref_internal();
    }
}

//IMPL Deref for JailValueMut
impl<'a, T> Deref for JailValueMut<'a, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.ref_internal.val
    }
}

//IMPL DerefMut for JailValueMut
impl<'a, T> DerefMut for JailValueMut<'a, T> {
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.ref_internal.val
    }
}

//IMPL AsRef for JailValueMut
impl<'a, T> AsRef<T> for JailValueMut<'a, T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        &self.ref_internal.val
    }
}

//IMPL AsMut for JailValueMut
impl<'a, T> AsMut<T> for JailValueMut<'a, T> {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut T {
        &mut self.ref_internal.val
    }
}

//IMPL Borrow for JailValueMut
impl<'a, T> Borrow<T> for JailValueMut<'a, T> {
    #[inline(always)]
    fn borrow(&self) -> &T {
        &self.ref_internal.val
    }
}

//IMPL BorrowMut for JailValueMut
impl<'a, T> BorrowMut<T> for JailValueMut<'a, T> {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut T {
        &mut self.ref_internal.val
    }
}

//STRUCT JailValueRef
/// A guarded wrapper around an immutable reference to the value contained in a [JailCell]
///
/// [JailValueRef<T>] implements [Deref<Target = T>], [AsRef<T>], and [Borrow<T>]
/// to allow transparent access to its underlying value
///
/// As long as the [JailValueRef] remains in scope, the value in [JailCell] will
/// remain marked as immutably referenced and unable to be mutably referenced.
/// You can manually drop the [JailValueRef] out of scope by passing it as the first parameter
/// to the function [JailValueRef::unguard(jail_val_ref)]
///
/// You can obtain a [JailValueRef] by calling `guard_ref()` on a [JailCell]
/// ### Example
/// ```rust
/// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueRef}};
/// # fn main() -> Result<(), AccessError> {
/// let jail: JailCell<u32> = JailCell::new(42);
/// let mut grd_ref = jail.guard_ref()?;
/// assert_eq!(*grd_ref, 42);
/// jail.visit_ref(|val| {
///     assert_eq!(*val, 42);
///     Ok(())
/// });
/// JailValueRef::unguard(grd_ref);
/// # Ok(())
/// # }
/// ```
pub struct JailValueRef<'a, T> {
    ref_internal: &'a mut JailCellMutable<T>,
}

impl<'a, T> JailValueRef<'a, T> {
    //FN JailValueRef::unguard()
    /// Manually end a [JailValueRef] value's temporary guarded absence from the [JailCell]
    ///
    /// This method simply takes ownership of the [JailValueRef] and immediately lets it go out of scope,
    /// causing it's `drop()` method to be called and decreasing its immutable reference count in the [JailCell]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueRef}};
    /// # fn main() -> Result<(), AccessError> {
    /// let jail: JailCell<u32> = JailCell::new(42);
    /// let grd_ref = jail.guard_ref()?;
    /// // val CANNOT be mutably referenced because the immutable reference is still in scope
    /// assert!(jail.visit_mut(|val| Ok(())).is_err());
    /// JailValueRef::unguard(grd_ref);
    /// // val CAN be mutably referenced because the immutable reference was dropped
    /// assert!(jail.visit_mut(|val| Ok(())).is_ok());
    /// # Ok(())
    /// # }
    /// ```
    pub fn unguard(_guarded_jail_value: Self) {}
}

//IMPL Drop for JailValueRef
impl<'a, T> Drop for JailValueRef<'a, T> {
    fn drop(&mut self) {
        self.ref_internal.remove_ref_internal();
    }
}

//IMPL Deref for JailValueRef
impl<'a, T> Deref for JailValueRef<'a, T> {
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &self.ref_internal.val
    }
}

//IMPL AsRef for JailValueRef
impl<'a, T> AsRef<T> for JailValueRef<'a, T> {
    #[inline(always)]
    fn as_ref(&self) -> &T {
        &self.ref_internal.val
    }
}

//IMPL Borrow for JailValueRef
impl<'a, T> Borrow<T> for JailValueRef<'a, T> {
    #[inline(always)]
    fn borrow(&self) -> &T {
        &self.ref_internal.val
    }
}

//REGION Testing
#[cfg(test)]
mod tests {
    #![allow(dead_code)]
    #![allow(unused_variables)]
    use std::{fmt::Display, mem};

    use super::*;

    /// prison, access_count, gen, next_free, free_count, vec_len
    macro_rules! assert_prison_state {
        ($P:ident, $A_CNT:expr, $GEN:expr, $NEXT:expr, $F_CNT:expr, $LEN:expr) => {
            let p = &internal!($P);
            if p.access_count != $A_CNT
                || p.generation != $GEN
                || p.next_free != $NEXT
                || p.free_count != $F_CNT
                || p.vec.len() != $LEN {
                    panic!("\nIncorrect prison state:\n\tEXP:\taccess_count: {}, gen: {}, next_free: {}, free_count: {}, vec_len: {}\n\tGOT:\taccess_count: {}, gen: {}, next_free: {}, free_count: {}, vec_len: {}\n",
                    $A_CNT, $GEN, $NEXT, $F_CNT, $LEN,
                    p.access_count, p.generation, p.next_free, p.free_count, p.vec.len());
                }
        };
    }

    /// prison, index, refs, gen, val
    macro_rules! assert_cell_state {
        ($P:ident, $IDX:expr, $REFS:expr, $GEN:expr, $VAL:expr) => {
            match &internal!($P).vec[$IDX] {
                cell if (cell.is_cell() && cell.refs_or_next == $REFS && IdxD::val(cell.d_gen_or_prev) == $GEN && unsafe {cell.val.assume_init_ref()} == &$VAL) => {},
                cell if cell.is_cell() => panic!("\nIndex {} unexpected state:\n\tEXP:\trefs = {}, gen = {}, val = {}\n\tGOT:\trefs = {}, gen = {}, val = {}\n", $IDX, $REFS, $GEN, $VAL, cell.refs_or_next, IdxD::val(cell.d_gen_or_prev), unsafe {cell.val.assume_init_ref()}),
                _ => panic!("\nIndex {} wrong variant:\n\tEXP:\t`Cell`\n\tGOT:\t`Free`\n", $IDX)
            }
        };
    }

    /// prison, index, prev_free, next_free
    macro_rules! assert_free_state {
        ($P:ident, $IDX:expr, $PREV:expr, $NEXT:expr) => {
            match &internal!($P).vec[$IDX] {
                free if (free.is_free() && free.refs_or_next == $NEXT && IdxD::val(free.d_gen_or_prev) == $PREV) => {},
                free if free.is_free() => panic!("\nIndex {} unexpected state:\n\tEXP:\tprev_free = {}, next_free = {}\n\tGOT:\tprev_free = {}, next_free = {}\n", $IDX, $PREV, $NEXT, IdxD::val(free.d_gen_or_prev), free.refs_or_next),
                _ => panic!("\nIndex {} wrong variant:\n\tEXP:\t`Free`\n\tGOT:\t`Cell`\n", $IDX)
            }
        };
    }

    /// operation, error
    macro_rules! assert_access_err {
        ($OP:expr, $ERR:expr) => {
            match $OP {
                Err(e) if (e == $ERR) => {},
                Err(e) => panic!("\nOperation returned incorrect error:\n\tEXP:\t{}\n\tGOT:\t{}\n", $ERR.kind(), e.kind()),
                _ => panic!("\nOperation failed to return error:\n\tEXP:\tErr({})\n\tGOT:\tOk(*)\n", $ERR.kind())
            }
        };
    }

    /// operation, index, gen
    macro_rules! assert_cell_key {
        ($OP:expr, $IDX:expr, $GEN:expr) => {
            match $OP {
                Ok(key) if (key.idx == $IDX && key.gen == $GEN) => key,
                Ok(key) => panic!("\nOperation returned incorrect CellKey:\n\tEXP:\tidx = {}, gen = {}\n\tGOT:\tidx = {}, gen = {}\n", $IDX, $GEN, key.idx, key.gen),
                Err(e) => panic!("\nOperation failed to return CellKey:\n\tEXP:\tCellKey{{ idx: {}, gen: {}}}\n\tGOT:\tErr({})\n", $IDX, $GEN, e.kind())
            }
        };
    }

    #[derive(Debug, Eq, PartialEq)]
    struct MyNoCopy(usize);

    impl Display for MyNoCopy {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl MyNoCopy {
        fn val(&self) -> usize {
            self.0
        }
    }

    fn extract_usize(mnc: &MyNoCopy) -> usize {
        mnc.val()
    }

    struct SizeEmptyPrisonCell(PrisonCell<()>); // Size 16, Align 8
    struct SizeU8PrisonCell(PrisonCell<u8>); // Size 24, Align 8
    struct SizeU16PrisonCell(PrisonCell<u16>); // Size 24, Align 8
    struct Size3BPrisonCell(PrisonCell<(u8, u8, u8)>); // Size 24, Align 8
    struct SizeU32PrisonCell(PrisonCell<u32>); // Size 24, Align 8
    struct Size5BPrisonCell(PrisonCell<(u8, u8, u8, u8, u8)>); // Size 24, Align 8
    struct Size6BPrisonCell(PrisonCell<(u8, u8, u8, u8, u8, u8)>); // Size 24, Align 8
    struct Size7BPrisonCell(PrisonCell<(u8, u8, u8, u8, u8, u8, u8)>); // Size 24, Align 8
    struct SizeU64PrisonCell(PrisonCell<u64>); // Size 24, Align 8
    struct Size9BPrisonCell(PrisonCell<(u8, u8, u8, u8, u8, u8, u8, u8, u8)>); // Size 32, Align 8
    struct SizeU128PrisonCell(PrisonCell<u128>); // Size 32, Align 8

    //REGION Prison tests
    //FN TEST: memory footprint
    #[test]
    #[ignore]
    fn memory_footprint() -> Result<(), AccessError> {
        assert_eq!(mem::size_of::<PrisonCell<()>>(), 16);
        assert_eq!(mem::size_of::<PrisonCell<u8>>(), 24);
        assert_eq!(mem::size_of::<PrisonCell<u64>>(), 24);
        assert_eq!(mem::size_of::<PrisonCell<(u8, u8, u8, u8, u8, u8, u8, u8, u8)>>(), 32);
        assert_eq!(mem::size_of::<PrisonCell<u128>>(), 32);
        let vec_size = mem::size_of::<Vec<u8>>();
        assert_eq!(mem::size_of::<Prison<u8>>(), 32+vec_size);
        Ok(())
    }

    //TODO: TEST: Prison::new()
    //TODO: TEST: Prison::with_capacity()
    //TODO: TEST: Prison::vec_len()
    //TODO: TEST: Prison::vec_cap()
    //TODO: TEST: Prison::num_free()
    //TODO: TEST: Prison::num_used()
    //TODO: TEST: Prison::density()

    //FN TEST: insert()
    #[test]
    fn insert() -> Result<(), AccessError> {
        let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 0);
        let key_0 = assert_cell_key!(prison.insert(MyNoCopy(0)), 0, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 1);
        let key_1 = assert_cell_key!(prison.insert(MyNoCopy(1)), 1, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 2);
        let key_2 = assert_cell_key!(prison.insert(MyNoCopy(2)), 2, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 3);
        prison.visit_ref(key_0, |val_0| {
            assert_access_err!(prison.insert(MyNoCopy(3)), AccessError::InsertAtMaxCapacityWhileAValueIsReferenced);
            Ok(())
        })?;
        prison.visit_mut(key_0, |val_0| {
            assert_access_err!(prison.insert(MyNoCopy(3)), AccessError::InsertAtMaxCapacityWhileAValueIsReferenced);
            Ok(())
        })?;
        let key_3 = assert_cell_key!(prison.insert(MyNoCopy(3)), 3, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 4);
        assert_cell_state!(prison, 0, 0, 0, MyNoCopy(0));
        assert_cell_state!(prison, 1, 0, 0, MyNoCopy(1));
        assert_cell_state!(prison, 2, 0, 0, MyNoCopy(2));
        assert_cell_state!(prison, 3, 0, 0, MyNoCopy(3));
        prison.remove(key_2)?; 
        assert_free_state!(prison, 2, IdxD::INVALID, IdxD::INVALID);
        assert_prison_state!(prison, 0, 1, 2, 1, 4);
        let key_2 = assert_cell_key!(prison.insert(MyNoCopy(22)), 2, 1);
        assert_cell_state!(prison, 2, 0, 1, MyNoCopy(22));
        assert_prison_state!(prison, 0, 1, IdxD::INVALID, 0, 4);
        Ok(())
    }

    //FN TEST: insert_at()
    #[test]
    fn insert_at() -> Result<(), AccessError> {
        let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 0);
        assert_access_err!(prison.insert_at(0, MyNoCopy(0)), AccessError::IndexOutOfRange(0));
        let key_0 = assert_cell_key!(prison.insert(MyNoCopy(0)), 0, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 1);
        let key_1 = assert_cell_key!(prison.insert(MyNoCopy(1)), 1, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 2);
        let key_2 = assert_cell_key!(prison.insert(MyNoCopy(2)), 2, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 3);
        prison.remove(key_0)?;
        assert_prison_state!(prison, 0, 1, 0, 1, 3);
        assert_free_state!(prison, 0, IdxD::INVALID, IdxD::INVALID);
        prison.remove(key_1)?;
        assert_prison_state!(prison, 0, 1, 1, 2, 3);
        assert_free_state!(prison, 1, IdxD::INVALID, 0);
        assert_free_state!(prison, 0, 1, IdxD::INVALID);
        prison.remove(key_2)?;
        assert_free_state!(prison, 2, IdxD::INVALID, 1);
        assert_free_state!(prison, 1, 2, 0);
        assert_free_state!(prison, 0, 1, IdxD::INVALID);
        assert_prison_state!(prison, 0, 1, 2, 3, 3);
        let key_0 = assert_cell_key!(prison.insert_at(0, MyNoCopy(10)), 0, 1);
        assert_prison_state!(prison, 0, 1, 2, 2, 3);
        assert_free_state!(prison, 2, IdxD::INVALID, 1);
        assert_free_state!(prison, 1, 2, IdxD::INVALID);
        let key_1 = assert_cell_key!(prison.insert_at(1, MyNoCopy(11)), 1, 1);
        assert_prison_state!(prison, 0, 1, 2, 1, 3);
        assert_free_state!(prison, 2, IdxD::INVALID, IdxD::INVALID);
        let key_2 = assert_cell_key!(prison.insert_at(2, MyNoCopy(12)), 2, 1);
        assert_prison_state!(prison, 0, 1, IdxD::INVALID, 0, 3);
        assert_access_err!(prison.insert_at(0, MyNoCopy(0)), AccessError::IndexIsNotFree(0));
        assert_cell_state!(prison, 0, 0, 1, MyNoCopy(10));
        assert_cell_state!(prison, 1, 0, 1, MyNoCopy(11));
        assert_cell_state!(prison, 2, 0, 1, MyNoCopy(12));
        prison.remove(key_0)?;
        prison.remove(key_1)?;
        prison.remove(key_2)?;
        let key_1 = assert_cell_key!(prison.insert_at(1, MyNoCopy(111)), 1, 2);
        assert_free_state!(prison, 2, IdxD::INVALID, 0);
        assert_cell_state!(prison, 1, 0, 2, MyNoCopy(111));
        assert_free_state!(prison, 0, 2, IdxD::INVALID);
        Ok(())
    }

    //FN TEST: overwrite()
    #[test]
    fn overwrite() -> Result<(), AccessError> {
        // test `overwrite()` behaves exactly like `insert_at()` when given a free index
        let prison: Prison<MyNoCopy> = Prison::with_capacity(3);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 0);
        assert_access_err!(prison.insert_at(0, MyNoCopy(0)), AccessError::IndexOutOfRange(0));
        let key_0 = assert_cell_key!(prison.insert(MyNoCopy(0)), 0, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 1);
        let key_1 = assert_cell_key!(prison.insert(MyNoCopy(1)), 1, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 2);
        let key_2 = assert_cell_key!(prison.insert(MyNoCopy(2)), 2, 0);
        assert_prison_state!(prison, 0, 0, IdxD::INVALID, 0, 3);
        prison.remove(key_0)?;
        assert_prison_state!(prison, 0, 1, 0, 1, 3);
        assert_free_state!(prison, 0, IdxD::INVALID, IdxD::INVALID);
        prison.remove(key_1)?;
        assert_prison_state!(prison, 0, 1, 1, 2, 3);
        assert_free_state!(prison, 1, IdxD::INVALID, 0);
        assert_free_state!(prison, 0, 1, IdxD::INVALID);
        prison.remove(key_2)?;
        assert_free_state!(prison, 2, IdxD::INVALID, 1);
        assert_free_state!(prison, 1, 2, 0);
        assert_free_state!(prison, 0, 1, IdxD::INVALID);
        assert_prison_state!(prison, 0, 1, 2, 3, 3);
        let key_0 = assert_cell_key!(prison.overwrite(0, MyNoCopy(10)), 0, 1);
        assert_prison_state!(prison, 0, 1, 2, 2, 3);
        assert_free_state!(prison, 2, IdxD::INVALID, 1);
        assert_free_state!(prison, 1, 2, IdxD::INVALID);
        let key_1 = assert_cell_key!(prison.overwrite(1, MyNoCopy(11)), 1, 1);
        assert_prison_state!(prison, 0, 1, 2, 1, 3);
        assert_free_state!(prison, 2, IdxD::INVALID, IdxD::INVALID);
        let key_2 = assert_cell_key!(prison.overwrite(2, MyNoCopy(12)), 2, 1);
        assert_prison_state!(prison, 0, 1, IdxD::INVALID, 0, 3);
        assert_cell_state!(prison, 0, 0, 1, MyNoCopy(10));
        assert_cell_state!(prison, 1, 0, 1, MyNoCopy(11));
        assert_cell_state!(prison, 2, 0, 1, MyNoCopy(12));
        prison.remove(key_0)?;
        prison.remove(key_1)?;
        prison.remove(key_2)?;
        let key_1 = assert_cell_key!(prison.overwrite(1, MyNoCopy(111)), 1, 2);
        assert_free_state!(prison, 2, IdxD::INVALID, 0);
        assert_cell_state!(prison, 1, 0, 2, MyNoCopy(111));
        assert_free_state!(prison, 0, 2, IdxD::INVALID);
        // Test overwriting filled cell
        let key_1 = assert_cell_key!(prison.overwrite(1, MyNoCopy(1111)), 1, 3);
        assert_cell_state!(prison, 1, 0, 3, MyNoCopy(1111));
        assert_prison_state!(prison, 0, 3, 2, 2, 3);
        Ok(())
    }

    //TODO: TEST: Prison::remove()
    //TODO: TEST: Prison::remove_idx()
    //TODO: TEST: Prison::visit_mut()
    //TODO: TEST: Prison::visit_ref()
    //TODO: TEST: Prison::visit_mut_idx()
    //TODO: TEST: Prison::visit_ref_idx()
    //TODO: TEST: Prison::visit_many_mut()
    //TODO: TEST: Prison::visit_many_ref()
    //TODO: TEST: Prison::visit_many_mut_idx()
    //TODO: TEST: Prison::visit_many_ref_idx()
    //TODO: TEST: Prison::visit_slice_mut()
    //TODO: TEST: Prison::visit_slice_ref()
    //TODO: TEST: Prison::guard_mut()
    //TODO: TEST: Prison::guard_ref()
    //TODO: TEST: Prison::guard_mut_idx()
    //TODO: TEST: Prison::guard_ref_idx()
    //TODO: TEST: Prison::guard_many_mut()
    //TODO: TEST: Prison::guard_many_ref()
    //TODO: TEST: Prison::guard_many_mut_idx()
    //TODO: TEST: Prison::guard_many_ref_idx()
    //TODO: TEST: Prison::guard_slice_mut()
    //TODO: TEST: Prison::guard_slice_ref()
    //TODO: TEST: Prison::clone_val()
    //TODO: TEST: Prison::clone_val_idx()
    //TODO: TEST: Prison::clone_many_vals()
    //TODO: TEST: Prison::clone_many_vals_idx()

    //REGION JailCell Tests
    //TODO: TEST: JailCell::new()
    //TODO: TEST: JailCell::visit_mut()
    //TODO: TEST: JailCell::visit_ref()
    //TODO: TEST: JailCell::guard_mut()
    //TODO: TEST: JailCell::guard_ref()
    //TODO: TEST: JailCell::clone_val()
}
