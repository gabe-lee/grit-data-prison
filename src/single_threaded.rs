#[cfg(not(feature = "no_std"))]
use std::ops::RangeBounds;

#[cfg(feature = "no_std")]
use core::ops::RangeBounds;

use crate::{AccessError, LockValPair, CountVecPair, extract_true_start_end};

/// The single-threaded implementation of [`Prison<T>`]
/// 
/// This struct uses an underlying `Vec<T>` to store data, but provides full interior mutability
/// for all of its methods. It does this by using [`std::cell::UnsafeCell`], simple `bool` locks,
/// and a master `usize` counter that are used to determine what cells (indexes) are cureently
/// being accessed to prevent violating Rust's memory management rules (to the best of it's ability).
/// 
/// See the crate-level documentation for more info
#[derive(Debug)]
pub struct Prison<T> {
    count_vec: CountVecPair<T>,
}

impl<T> Prison<T> {
    /// Create a new [`Prison<T>`] with the default capacity ([`Vec::new()`])
    /// 
    /// Because re-allocating the internal `Vec` comes with many restrictions,
    /// it is recommended to use [`Prison::with_capacity()`] with a suitable 
    /// best-guess starting value rather than [`Prison::new()`]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let my_prison: Prison<u32> = Prison::new();
    /// assert!(my_prison.cap() < 100)
    /// # }
    /// ```
    pub fn new() -> Self {
        return Self { count_vec: CountVecPair::new() };
    }
    /// Create a new [`Prison<T>`] with the a specific starting capacity (`Vec::with_capacity()`)
    /// 
    /// Because re-allocating the internal `Vec` comes with many restrictions,
    /// it is recommended to use [`Prison::with_capacity()`] with a suitable 
    /// best-guess starting value rather than [`Prison::new()`]
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let my_prison: Prison<u32> = Prison::with_capacity(1000);
    /// assert!(my_prison.cap() == 1000)
    /// # }
    /// ```
    pub fn with_capacity(size: usize) -> Self {
        return Self { count_vec: CountVecPair::with_capacity(size) };
    }
    /// Return the length of the underlying `Vec`
    /// 
    /// Length refers to the number of *filled* indexes in an Vec,
    /// not necessarily the number of reserved spaces in memory allocated to it.
    pub fn len(&self) -> usize {
        let (_, vec) = self.count_vec.open();
        return vec.len();
    }
    /// Return the capacity of the underlying `Vec`
    /// 
    /// Capacity refers to the number of total spaces in memory reserved for the Vec
    /// to *possibly* use, not the number it currently *has* used
    pub fn cap(&self) -> usize {
        let (_, vec) = self.count_vec.open();
        return vec.capacity();
    }
    /// Add a value onto the end of the underlying `Vec`
    /// 
    /// As long as there is sufficient capacity to do so, you may `push()`
    /// to the `Prison` while in the middle of any `visit()`
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let string_prison: Prison<String> = Prison::with_capacity(10);
    /// string_prison.push(String::from("Hello, "));
    /// string_prison.visit(0, |first_string| {
    ///     string_prison.push(String::from("World!"));
    ///     string_prison.visit(1, |second_string| {
    ///         let hello_world = format!("{}{}", first_string, second_string);
    ///         assert_eq!(hello_world, "Hello, World!");
    ///     });
    /// });
    /// # }
    /// ```
    /// 
    /// However, if the `Prison` is at maxumum capacity, attempting to `push()`
    /// during a visit will cause the operation to fail and a [`AccessError::PushAtMaxCapacityWhileVisiting`]
    /// to be returned
    /// ### Example
    /// ```rust
    /// # use grit_data_prison::single_threaded::Prison;
    /// # fn main() {
    /// let string_prison: Prison<String> = Prison::with_capacity(1);
    /// string_prison.push(String::from("Hello, "));
    /// string_prison.visit(0, |first_string| {
    ///     assert!(string_prison.push(String::from("World!")).is_err());
    /// });
    /// # }
    /// ```
    pub fn push(&self, value: T) -> Result<usize, AccessError> {
        let (visit_count, vec) = self.count_vec.open();
        let new_idx = vec.len();
        if new_idx >= vec.capacity() && *visit_count > 0 {
            return Err(AccessError::PushAtMaxCapacityWhileVisiting);
        }
        vec.push(LockValPair::new(value));
        return Ok(new_idx);
    }

    pub fn pop(&self) -> Result<T, AccessError> {
        let (_, vec) = self.count_vec.open();
        if vec.len() == 0 {
            return Err(AccessError::PopOnEmptyPrison)
        }
        let (last_locked, _) = vec[vec.len()-1].open();
        if *last_locked {
            return Err(AccessError::PopWhileLastElementIsVisited(vec.len()-1));
        }
        return Ok(vec.pop().unwrap().0.into_inner().1);
    }

    pub fn visit<F: FnMut(&mut T)>(&self, cell_index: usize, mut operation: F) -> Result<(), AccessError> {
        let (current_visits, cells) = self.count_vec.open();
        if cell_index >= cells.len() {
            return Err(AccessError::IndexOutOfRange(cell_index));
        }
        let (locked, val) = (&cells[cell_index]).open();
        if *locked {
            return Err(AccessError::CellAlreadyBeingVisited(cell_index));
        }
        *current_visits += 1;
        *locked = true;
        operation(val);
        *locked = false;
        *current_visits -= 1;
        return Ok(());
    }

    pub fn visit_many<F: FnMut(&mut[&mut T])>(&self, cell_indices: &[usize], mut operation: F) -> Result<(), AccessError> {
        let (current_visits, cells) = self.count_vec.open();
        let mut vals = Vec::new();
        let mut locks = Vec::new();
        let mut ret_value = Ok(());
        for idx in cell_indices {
            if *idx >= cells.len() {
                ret_value = Err(AccessError::IndexOutOfRange(*idx));
                break;
            }
            let (locked, val) = (&cells[*idx]).open();
            if *locked {
                ret_value = Err(AccessError::CellAlreadyBeingVisited(*idx));
                break;
            }
            *locked = true;
            locks.push(locked);
            vals.push(val);
        }
        if ret_value.is_ok() {
            *current_visits += 1;
            operation(vals.as_mut_slice());
            *current_visits -= 1;
        }
        for lock in locks {
            *lock = false
        }
        return ret_value;
    }

    pub fn visit_slice<IB, F>(&self, range: IB, operation: F) -> Result<(), AccessError>
    where
    IB: Iterator<Item = usize> + RangeBounds<usize>,
    F:  FnMut(&mut[&mut T]) {
        let (start, end) = extract_true_start_end(range, self.len());
        let indices: Vec<usize> = (start..end).collect();
        println!("{:?}", indices); //DEBUG
        return self.visit_many(&indices, operation)
    }

    fn visit_with_index<F: FnMut(usize, &mut T)>(&self, cell_index: usize, mut operation: F) -> Result<(), AccessError> {
        let (current_visits, cells) = self.count_vec.open();
        if cell_index >= cells.len() {
            return Err(AccessError::IndexOutOfRange(cell_index));
        }
        let (locked, val) = (&cells[cell_index]).open();
        if *locked {
            return Err(AccessError::CellAlreadyBeingVisited(cell_index));
        }
        *current_visits += 1;
        *locked = true;
        operation(cell_index, val);
        *locked = false;
        *current_visits -= 1;
        return Ok(());
    }

    pub fn visit_each_in_range<IB, F>(&self, range: IB, mut operation: F) -> Result<(), AccessError>
    where
    IB: Iterator<Item = usize> + RangeBounds<usize>,
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

    pub fn visit_each<F: FnMut(usize, &mut T)>(&self, mut operation: F) -> Result<(), AccessError> {
        for i in 0..self.len() {
            if let Err(err) = self.visit_with_index(i, &mut operation) {
                return Err(err);
            }
        }
        return Ok(())
    }
}