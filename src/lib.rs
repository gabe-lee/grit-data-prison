/*! This crate provides the generic type [Prison<T>](crate::single_threaded::Prison), a data structure that uses an underlying [`Vec<T>`]
 to store values of the same type, but allows simultaneous interior mutability to each and every
 value by providing `.visit()` methods that take closures that are passed mutable references to the values.
 
 This documentation describes the usage of [`Prison<T>`](crate::single_threaded::Prison), how its [Vec] analogous methods differ from
 those found on a [Vec], how to use its unusual `.visit()` methods, and how it achieves memory safety.
 
 # Motivation
 
 I wanted a data structure that met these criteria:
 - Backed by a [Vec<T>] (or similar) for cache efficiency
 - Allowed interior mutability to each of its elements
 - Was fully memory safe (**needs verification**)
 - Always returned a relevant error instead of panicing
 - Was easier to reason about when and where it might error than reference counting
 
 # Usage
 
 This crate is [on crates.io](https://crates.io/crates/grit-data-prison)
 
 First, add this crate as a dependency to your project:
 ```toml
 [dependencies]
 grit-data-prison = "0.1.2"
 ```
 Then import [`AccessError`] from the crate root, along with the relevant version you wish to use in
 the file where it is needed (right now only one flavor is available, [`single_threaded`]):
 ```rust
 use grit_data_prison::{AccessError, single_threaded::Prison};
 ```
 Create a prison and add your data to it (NOTE that it does not have to be declared `mut`)
 ```rust
 # use grit_data_prison::{AccessError, single_threaded::Prison};
 # fn main() {
 let prison: Prison<String> = Prison::new();
 prison.push(String::from("Hello, "));
 prison.push(String::from("World!"));
 # }
 ```
 You can then use one of the `.visit()` methods to access a mutable reference
 to your data from within a closure
 ```rust
 # use grit_data_prison::{AccessError, single_threaded::Prison};
 # fn main() {
 # let prison: Prison<String> = Prison::new();
 # prison.push(String::from("Hello, "));
 # prison.push(String::from("World!"));
 prison.visit(1, |val_at_idx_1| {
     *val_at_idx_1 = String::from("Rust!!");
 });
 # }
 ```
 Visiting multiple values at the same time can be done by nesting `.visit()` calls,
 or by using one of the batch `.visit()` methods
 ```rust
 # use grit_data_prison::{AccessError, single_threaded::Prison};
 # fn main() {
 # let prison: Prison<String> = Prison::new();
 # prison.push(String::from("Hello, "));
 # prison.push(String::from("World!"));
 # prison.visit(1, |val_at_idx_1| {
 #    *val_at_idx_1 = String::from("Rust!!");
 # });
 prison.visit(0, |val_0| {
     prison.visit(1, |val_1| {
         println!("{}{}", *val_0, *val_1); // Prints "Hello, Rust!!"
     });
 });
 prison.visit_many(&[0, 1], |vals| {
     println!("{}{}", vals[0], vals[1]); // Also prints "Hello, Rust!!"
 });
 # }
 ```
 Operations that affect the underlying Vector can also be done
 from *within* `.visit()` closures as long as none of the following rules are violated:
 - The operation does not remove, read, or modify any element that is *currently* being visited
 - The operation does not cause a re-allocation of the entire Vector (or otherwise cause the entire Vector to relocate to another memory address)
 ```rust
 # use grit_data_prison::{AccessError, single_threaded::Prison};
 # fn main() {
 let prison: Prison<u64> = Prison::with_capacity(10);
 prison.push(0);
 prison.push(10);
 prison.push(20);
 prison.push(30);
 prison.push(42);
 let mut accidental_val: u64 = 0;
 prison.visit(3, |val| {
     accidental_val = prison.pop().unwrap();
     prison.push(40);
 });
 # }
 ```
 
 For more examples, see the specific documentation for the relevant type/method
 
 # Why this strange syntax?
 
 Closures provide a safe sandbox to access mutable references, 
 as they cant be moved out of the closure, and because they use generics the rust compiler can
 choose to inline them in many/most cases.
 
 # How is this safe?!
 
 The short answer is: it *should* be *mostly* safe.
 I welcome any feedback and analysis showing otherwise so I can fix it or revise my methodology.
 
 [Prison](crate::single_threaded::Prison) follows a few simple rules:
 - One and ONLY one reference to any element can be in scope at any given time
 - Because we are only allowing one reference, that one reference will always be a mutable reference
 - Any method that would or *could* read, modify, or delete any element cannot be performed while that element is currently being visited
 - Any method that would or *could* cause the underlying Vector to relocate to a different spot in memory cannot be performed while even ONE visit is in progress
 
 It achieves all of the above with a few lightweight sentinel values:
 - A single [UnsafeCell](std::cell::UnsafeCell) to hold *all* of the [Prison](crate::single_threaded::Prison) internals and provide interior mutability
 - A master `visit_count` [usize] on Prison itself to track whether *any* visit is in progress
 - A `locked` [bool] on each element that prevents getting 2 mutable references to the same element
 
 Attempting to perform an action that would violate any of these rules will either be prevented from compiling
 or return an [AccessError] that describes why it was an error, and should never panic.
 ### Example: compile-time safety
 ```compile_fail
 # use grit_data_prison::{AccessError, single_threaded::Prison};
 # fn main() {
 let prison: Prison<String> = Prison::new();
 prison.push(String::from("cannot be stolen"));
 let mut steal_mut_ref: &mut String = String::new();
 let mut steal_prison: Prison<bool> = Prison::new();
 prison.visit(0, |mut_ref| {
     // will not compile: (error[E0521]: borrowed data escapes outside of closure)
     steal_mut_ref = mut_ref;
     // will not compile: (error[E0505]: cannot move out of `prison` because it is borrowed)
     steal_prison = prison;
 });
 # }
 ```
 ### Example: run-time safety
 ```rust
 # use grit_data_prison::{AccessError, single_threaded::Prison};
 struct MyStruct(u32);
 
 fn main() {
     let prison: Prison<MyStruct> = Prison::with_capacity(2); // Note this prison can only hold 2 elements
     prison.push(MyStruct(1));
     prison.push(MyStruct(2));
     prison.visit(0, |val_0| {
         assert!(prison.visit(0, |val_0_again| {}).is_err());
         assert!(prison.visit(3, |val_3_out_of_bounds| {}).is_err());
         prison.visit(1, |val_1| {
             assert!(prison.pop().is_err()); // would delete memory referenced by val_1
             assert!(prison.push(MyStruct(3)).is_err()); // would cause reallocation and invalidate any current references
         });
     });
 }
 ```
 
 # How this crate may change in the future
 
 This crate is very much UNSTABLE, meaning that not every error condition may have a test,
 methods may return different errors/values as my understanding of how they should be properly implemented
 evolves, I may add/remove methods altogether, etc.
 
 **In particular** I plan on moving from a simple [usize] index approach to a **Generational Arena** approach,
 where each element is indexed by both a [usize] and [u64] indicating which insert operation was responsible for them.
 Since reallocating faces particular challenges in this data structure, this would allow me to recycle empty indices
 and relax restictions on adding and removing elements without running into the
 [ABA problem](https://en.wikipedia.org/wiki/ABA_problem).
 
 For example, the implementation might look similar to the crate [generational-arena](https://crates.io/crates/generational-arena),
 however I will probably do it a little differently on the inside.
 
 Other possible future additions may include:
 [x] Single-thread safe [Prison<T>](crate::single_threaded::Prison)
 [ ] Multi-thread safe `AtomicPrison<T>`
 [ ] ? Single standalone value version, `JailCell<T>`
 [ ] ? Multi-thread safe standalone value version, `AtomicJailCell<T>`
 [ ] ? Completely unchecked and unsafe version `UnPrison<T>`
 [ ] ??? Multi-thread ~~safe~~ unsafe version `AtomicUnPrison<T>`
 
 # How to Help/Contribute
 
 This crate is [on crates.io](https://crates.io/crates/grit-data-prison)
 The repo is [on github](https://github.com/gabe-lee/grit-data-prison)
 
 Feel free to leave feedback!
 If you can give me concrete examples that *definitely* violate memory-safety, meaning
 that the provided mutable references can be made to point to invalid/illegal memory
 (without the use of additional unsafe :P), or otherwise cause unsafe conditions (for
 example changing an expected enum variant to another where the compiler doesnt expect it
 to be possible), I'd love to fix, further restrict, or rethink the crate entirely.
*/

#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]
#![deny(missing_docs)]

#[cfg(not(feature = "no_std"))]
use std::{ops::RangeBounds, error::Error, fmt::{Display, Debug}};

#[cfg(feature = "no_std")]
use core::{ops::RangeBounds, fmt::{Display, Debug}};

#[cfg(feature = "no_std")]
trait Error: Debug + Display {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        None
    }
}

/// Module defining the version(s) of [`Prison<T>`] suitable for use only from within a single-thread
pub mod single_threaded;

/// Error type that provides helpful information about why an operation on any [`Prison<T>`] failed
/// 
/// Every error returned from functions or methods defined in this crate will be one of these variants,
/// and nearly all versions of [`Prison<T>`] are designed to never panic and always return errors.
/// 
/// Additional variants may be added in the future, therefore it is recommended you add a catch-all branch
/// to any match statements on this enum to future-proof your code:
/// ```rust
/// # use grit_data_prison::AccessError;
/// # fn main() {
/// # let acc_err = AccessError::IndexOutOfRange(100);
/// match acc_err {
///     AccessError::IndexOutOfRange(bad_idx) => {},
///     AccessError::CellAlreadyBeingVisited(double_idx) => {},
///     // other variants
///     _ => {}
/// }
/// # }
/// ```
/// 
/// [`AccessError`] has a custom implementation for both [`std::fmt::Display`] and 
/// [`std::fmt::Debug`] traits, with the `Display` version giving a short description of the problem,
/// and the `Debug` version giving a more in-depth explaination of exactly why an error had to be
/// returned
pub enum AccessError {
    /// Indicates that an operation attempted to access an index beyond the range of the [`Prison<T>`],
    /// along with the offending index
    IndexOutOfRange(usize),
    /// Indicates that an operation attempted to access an index already being accessed by another operation,
    /// along with the index in question
    CellAlreadyBeingVisited(usize),
    /// Indicates that a push would require re-allocation of the internal `Vec<T>`, thereby invalidating
    /// any current visits
    PushAtMaxCapacityWhileVisiting,
    /// Indicates that the last element in the [`Prison<T>`] is being accessed, and `pop()`-ing the value out
    /// of the underlying `Vec<T>` would invalidate the reference
    PopWhileLastElementIsVisited(usize),
    /// Indicates that the underlying `Vec<T>` is empty, and there is nothing to `pop()` out
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