#[cfg(not(feature = "no_std"))]
use std::ops::RangeBounds;

#[cfg(feature = "no_std")]
use core::ops::RangeBounds;

use crate::{AccessError, LockValPair, CountVecPair, extract_true_start_end};

#[derive(Debug)]
pub struct Prison<T> {
    count_vec: CountVecPair<T>,
}

impl<T> Prison<T> {
    pub fn new() -> Self {
        return Self { count_vec: CountVecPair::new() };
    }

    pub fn with_capacity(size: usize) -> Self {
        return Self { count_vec: CountVecPair::with_capacity(size) };
    }

    pub fn len(&self) -> usize {
        let (_, vec) = self.count_vec.open();
        return vec.len();
    }

    pub fn cap(&self) -> usize {
        let (_, vec) = self.count_vec.open();
        return vec.capacity();
    }

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