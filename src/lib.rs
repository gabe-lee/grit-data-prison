/*!This crate provides the struct [Prison<T>](crate::single_threaded::Prison), an arena data structure 
that allows simultaneous interior mutability to each and every element by providing `.visit()` methods
that take closures that are passed mutable references to the values.

This documentation describes the usage of [Prison<T>](crate::single_threaded::Prison), how its methods differ from
those found on a [Vec], how to use its unusual `.visit()` methods, and how it achieves memory safety.

## Quick Look
- Uses an underlying [Vec<T>] to store items of the same type
- Acts primarily as a Generational Arena, where each element is accessed using a [CellKey] that differentiates two values that may have been located at the same index but represent fundamentally separate data
- Can *also* be indexed with a plain [usize] for simple use cases
- Provides safe (***needs verification***) interior mutability by only providing mutable references to values using closures to define strict scopes where they are valid and hide the setup/teardown for safety checks
- Uses [bool] locks on each element and a master [usize] counter to track the number/location of active references and prevent mutable reference aliasing and disallow scenarios that could invalidate existing references
- [CellKey] uses a [usize] index and [usize] generation to match an index to the context in which it was created and prevent two unrelated values that both at some point lived at the same index from being mistaken as equal
- All methods return an [AccessError] where the scenario would cause a panic if not caught
 
## NOTE
This package is still UNSTABLE and may go through several iterations before I consider it good enough to set in stone
See [changelog](#changelog)

# Motivation
 
I wanted a data structure that met these criteria:
- Backed by a [Vec<T>] (or similar) for cache efficiency
- Allowed interior mutability to each of its elements
- Was fully memory safe (***needs verification***)
- Always returned a relevant error instead of panicking
- Was easier to reason about when and where it might error than reference counting
 
# Usage
 
This crate is [on crates.io](https://crates.io/crates/grit-data-prison)
 
First, add this crate as a dependency to your project:
```toml
[dependencies]
grit-data-prison = "0.2"
```
Then import [AccessError] and [CellKey] from the crate root, along with the relevant version you wish to use in
the file where it is needed (right now only one flavor is available, [single_threaded]):
```rust
use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
```
Create a [Prison<T>](crate::single_threaded::Prison) and add your data to it using one of the `insert()` type methods
 
Note the following quirks:
- A [Prison](crate::single_threaded::Prison) does not need to be declared `mut` to mutate it
- `insert()` and its variants return a [Result]<[CellKey], [AccessError]> that you need to handle
- You can ignore the [CellKey] and simply look up the value by index if you wish (shown later)
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
# fn main() -> Result<(), AccessError> {
let prison: Prison<String> = Prison::new();
let key_hello = prison.insert(String::from("Hello, "))?;
prison.insert(String::from("World!"))?;
# Ok(())
# }
```
You can then use one of the `.visit()` methods to access a mutable reference
to your data from within a closure
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
# fn main() -> Result<(), AccessError> {
# let prison: Prison<String> = Prison::new();
# let key_hello = prison.insert(String::from("Hello, "))?;
# prison.insert(String::from("World!"))?;
prison.visit_idx(1, |val_at_idx_1| {
    *val_at_idx_1 = String::from("Rust!!");
    Ok(())
});
# Ok(())
# }
```
Visiting multiple values at the same time can be done by nesting `.visit()` calls,
or by using one of the batch `.visit()` methods
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
# fn main() -> Result<(), AccessError> {
# let prison: Prison<String> = Prison::new();
# let key_hello = prison.insert(String::from("Hello, "))?;
# prison.insert(String::from("World!"))?;
# prison.visit_idx(1, |val_at_idx_1| {
#   *val_at_idx_1 = String::from("Rust!!");
#   Ok(())
# });
prison.visit(key_hello, |val_0| {
    prison.visit_idx(1, |val_1| {
        println!("{}{}", *val_0, *val_1); // Prints "Hello, Rust!!"
        Ok(())
    });
    Ok(())
});
prison.visit_many_idx(&[0, 1], |vals| {
    println!("{}{}", vals[0], vals[1]); // Also prints "Hello, Rust!!"
    Ok(())
});
# Ok(())
# }
```
### Full Example Code
```rust
use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};

fn main() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::new();
    let key_hello = prison.insert(String::from("Hello, "))?;
    prison.insert(String::from("World!"))?;
    prison.visit_idx(1, |val_at_idx_1| {
        *val_at_idx_1 = String::from("Rust!!");
        Ok(())
    });
    prison.visit(key_hello, |val_0| {
        prison.visit_idx(1, |val_1| {
            println!("{}{}", *val_0, *val_1); // Prints "Hello, Rust!!"
            Ok(())
        });
        Ok(())
    });
    prison.visit_many_idx(&[0, 1], |vals| {
        println!("{}{}", vals[0], vals[1]); // Also prints "Hello, Rust!!"
        Ok(())
    });
    Ok(())
}
```
Operations that affect the underlying [Vec] can also be done
from *within* `.visit()` closures as long as none of the following rules are violated:
- The operation does not remove, read, or modify any element that is *currently* being visited
- The operation does not cause a re-allocation of the entire [Vec] (or otherwise cause the entire [Vec] to relocate to another memory address)
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
# fn main() -> Result<(), AccessError> {
let prison: Prison<u64> = Prison::with_capacity(10);
prison.insert(0)?;
prison.insert(10)?;
prison.insert(20)?;
prison.insert(30)?;
prison.insert(42)?;
let mut accidental_val: u64 = 0;
prison.visit_idx(3, |val| {
    accidental_val = prison.remove_idx(4)?;
    prison.insert_at(4, 40);
    Ok(())
});
# Ok(())
# }
```
For more examples, see the specific documentation for the relevant type/method
 
# Why this strange syntax?
 
Closures provide a safe sandbox to access mutable references, as they cant be moved out of the closure,
and because the `visit()` functions that take the closures handle all of the
safety and housekeeping needed before and after.
 
Since closures use generics the rust compiler can inline them in many/most/all? cases.
 
# How is this safe?!
 
The short answer is: it *should* be *mostly* safe.
I welcome any feedback and analysis showing otherwise so I can fix it or revise my methodology.
 
[Prison](crate::single_threaded::Prison) follows a few simple rules:
- One and ONLY one reference to any element can be in scope at any given time
- Because we are only allowing one reference, that one reference will always be a mutable reference
- Any method that would or *could* read, modify, or delete any element cannot be performed while that element is currently being visited
- Any method that would or *could* cause the underlying [Vec] to relocate to a different spot in memory cannot be performed while even ONE visit is in progress

In addition, it provides the functionality of a Generational Arena with these additional rules:
- The [Prison](crate::single_threaded::Prison) has a master generation counter to track the largest generation of any element inside it
- Every valid element has a generation attatched to it, and `insert()` operations return a [CellKey] that pairs the element index with the current largest generation value
- Any operation that removes *or* overwrites a valid element *with a genreation counter that is equal to the largest generation* causes the master generation counter to increase by one
 
It achieves all of the above with a few lightweight sentinel values:
- A single [UnsafeCell](std::cell::UnsafeCell) to hold *all* of the [Prison](crate::single_threaded::Prison) internals and provide interior mutability
- A master `visit_count` [usize] on [Prison](crate::single_threaded::Prison) itself to track whether *any* visit is in progress
- A master `generation` [usize] on [Prison](crate::single_threaded::Prison) itself to track largest generation
- Each element is either a `Cell` or `Free` variant: 
    - A `Free` Simply contains the value of the *next* free index after this one is filled
    - A `locked` [bool] on each `Cell` that prevents getting 2 mutable references to the same element
    - A `generation` [usize] on each `Cell` to use when matching to the [CellKey] used to access the index

(see [performance](#performance) for more info on specifics)

Attempting to perform an action that would violate any of these rules will either be prevented from compiling
or return an [AccessError] that describes why it was an error, and should never panic.
### Example: compile-time safety
```compile_fail
# use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
# fn main() -> Result<(), AccessError> {
let prison: Prison<String> = Prison::new();
prison.insert(String::from("cannot be stolen"));
let mut steal_mut_ref: &mut String;
let mut steal_prison: Prison<String>;
prison.visit_idx(0, |mut_ref| {
    // will not compile: (error[E0521]: borrowed data escapes outside of closure)
    steal_mut_ref = mut_ref;
    // will not compile: (error[E0505]: cannot move out of `prison` because it is borrowed)
    steal_prison = prison;
    Ok(())
});
# Ok(())
# }
```
### Example: run-time safety
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
struct MyStruct(u32);

fn main() -> Result<(), AccessError> {
    let prison: Prison<MyStruct> = Prison::with_capacity(2); // Note this prison can only hold 2 elements
    let key_0 = prison.insert(MyStruct(1))?;
    prison.insert(MyStruct(2))?;
    prison.visit(key_0, |val_0| {
        assert!(prison.visit(key_0, |val_0_again| Ok(())).is_err());
        assert!(prison.visit_idx(0, |val_0_again| Ok(())).is_err());
        assert!(prison.visit_idx(3, |val_3_out_of_bounds| Ok(())).is_err());
        prison.visit_idx(1, |val_1| {
            assert!(prison.remove_idx(1).is_err()); // would delete memory referenced by val_1
            assert!(prison.remove(key_0).is_err()); // would delete memory referenced by val_0
            assert!(prison.insert(MyStruct(3)).is_err()); // would cause reallocation and invalidate any current references
            Ok(())
        });
        Ok(())
    });
    Ok(())
}
```
# Performance

### Speed
(Benchmarks are Coming Soon™)

### Size
[Prison<T>](crate::single_threaded::Prison) has 4 [usize] house-keeping values in addition to a [Vec<CellOrFree<T>>]

Each element in [Vec<CellOrFree<T>>] is Either a `Cell` variant or `Free` variant, so the marker is only a [u8]
- `Free` variant only contains a single [usize], so it is not the limiting variant
- `Cell` variant contains a [usize] generation counter, [bool] access lock, and a value of type `T`

Therefore the total _additional_ size compared to a [Vec<T>] on a 64-bit system is
(at worst due to alignment):

32 bytes flat + 16 bytes per element

# How this crate may change in the future
 
This crate is very much UNSTABLE, meaning that not every error condition may have a test,
methods may return different errors/values as my understanding of how they should be properly implemented
evolves, I may add/remove methods altogether, etc.
 
Possible future additions may include:
- [x] Single-thread safe [Prison<T>](crate::single_threaded::Prison)
- [ ] More public methods (as long as they make sense and don't bloat the API)
- [ ] Multi-thread safe `AtomicPrison<T>`
- [ ] ? Single standalone value version, `JailCell<T>`
- [ ] ? Multi-thread safe standalone value version, `AtomicJailCell<T>`
- [ ] ?? Completely unchecked and unsafe version `UnPrison<T>`
- [ ] ??? Multi-thread ~~safe~~ unsafe version `AtomicUnPrison<T>`
 
# How to Help/Contribute
 
This crate is [on crates.io](https://crates.io/crates/grit-data-prison)
The repo is [on github](https://github.com/gabe-lee/grit-data-prison)
 
Feel free to leave feedback, or fork/branch the project and submit fixes/optimisations!

If you can give me concrete examples that *definitely* violate memory-safety, meaning
that the provided mutable references can be made to point to invalid/illegal memory
(without the use of additional unsafe :P), or otherwise cause unsafe conditions (for
example changing an expected enum variant to another where the compiler doesnt expect it
to be possible), I'd love to fix, further restrict, or rethink the crate entirely.
# Changelog
 - Version 0.2.x has a different API than version 0.1.x and is a move from a plain Vec to a Generational Arena
 - Version 0.1.x: first version, plain old [Vec] with [usize] indexing
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
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

/// Module defining the version(s) of [Prison<T>] suitable for use only from within a single-thread
pub mod single_threaded;

/// Error type that provides helpful information about why an operation on any [Prison<T>](crate::single_threaded::Prison) failed
/// 
/// Every error returned from functions or methods defined in this crate will be one of these variants,
/// and nearly all versions of [Prison<T>](crate::single_threaded::Prison) are designed to never panic and always return errors.
/// 
/// Additional variants may be added in the future, therefore it is recommended you add a catch-all branch
/// to any match statements on this enum to future-proof your code:
/// ```rust
/// # use grit_data_prison::AccessError;
/// # fn main() {
/// # let acc_err = AccessError::IndexOutOfRange(100);
/// match acc_err {
///     AccessError::IndexOutOfRange(bad_idx) => {},
///     AccessError::IndexAlreadyBeingVisited(duplicate_idx) => {},
///     // other variants
///     _ => {}
/// }
/// # }
/// ```
/// 
/// [AccessError] has a custom implementation for both [std::fmt::Display] and 
/// [std::fmt::Debug] traits, with the `Display` version giving a short description of the problem,
/// and the `Debug` version giving a more in-depth explaination of exactly why an error had to be
/// returned
#[derive(PartialEq, Eq)]
pub enum AccessError {
    /// Indicates that an operation attempted to access an index beyond the range of the [Prison<T>](crate::single_threaded::Prison),
    /// along with the offending index
    IndexOutOfRange(usize),
    /// Indicates that an operation attempted to access an index already being accessed by another operation,
    /// along with the index in question
    IndexAlreadyBeingVisited(usize),
    /// Indicates that an insert would require re-allocation of the internal [Vec<T>], thereby invalidating
    /// any current visits
    InsertAtMaxCapacityWhileVisiting,
    /// Indicates that the last element in the [Prison<T>](crate::single_threaded::Prison) is being accessed, and `remove()`-ing the value
    /// from the underlying [Vec<T>] would invalidate the reference
    RemoveWhileIndexBeingVisited(usize),
    /// Indicates that the value requested was deleted and a new value with an updated generation took its place
    /// 
    /// Contains the index and generation from the invalid [CellKey], in that order
    ValueDeleted(usize, usize),
    /// Indicates that a very large number of removes and inserts caused the generation counter to reach its max value
    MaxValueForGenerationReached,
    /// Indicates that an attempted insert to a specific index would overwrite and invalidate a value still in use
    IndexIsNotFree(usize),
    /// Indicates that the underlying [Vec] reached the maximum capacity set by Rust ([isize::MAX])
    MaximumCapacityReached
}

impl Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::IndexOutOfRange(idx) => write!(f, "Index [{}] is out of range", idx),
            Self::IndexAlreadyBeingVisited(idx) => write!(f, "Cell at index [{}] is already being visited", idx),
            Self::InsertAtMaxCapacityWhileVisiting => write!(f, "Prison is at max capacity, cannot push() new value while visiting"),
            Self::ValueDeleted(idx, gen) => write!(f, "Value requested at index {} gen {} was already deleted", idx, gen),
            Self::MaxValueForGenerationReached => write!(f, "Maximum value for generation counter reached"),
            Self::RemoveWhileIndexBeingVisited(idx) => write!(f, "Index [{}] is currently being visited, cannot remove", idx),
            Self::IndexIsNotFree(idx) => write!(f, "Index [{}] is not free and may be still in use, cannot overwrite", idx),
            Self::MaximumCapacityReached => write!(f, "Prison has reached the maximum capacity allowed by Rust"),
        }
    }
}

impl Debug for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::IndexOutOfRange(idx) => write!(f, "Index [{}] is out of range", idx),
            Self::IndexAlreadyBeingVisited(idx) => write!(f, "Cell at index [{}] is already being visited\n---------\nVisiting the same cell twice would give two mutable references to the same memory. You could potentially alter some expected pre-condition the compiler expects of the value, such as changing an Enum's Variant or deleting all the items from a Vector expected to have a non-zero length.", idx),
            Self::InsertAtMaxCapacityWhileVisiting => write!(f, "Prison is at max capacity\n---------\nPushing to a Vec at max capacity while a visit is in progress may cause re-allocation that will invalidate value references"),
            Self::ValueDeleted(idx, gen) => write!(f, "Value requested at index {} gen {} was already deleted\n---------\nWhen deleting a value, it is recomended you take steps to invalidate any held keys refering to it", idx, gen),
            Self::MaxValueForGenerationReached => write!(f, "Maximum value for generation counter reached\n---------\nA large number of removals and inserts has caused the generation counter to reach its max value. Manually perform a Prison::purge() and re-issue the keys to continue using this Prison"),
            Self::RemoveWhileIndexBeingVisited(idx) => write!(f, "Index [{}] is currently being visited, cannot remove\n---------\nRemoving a value with an active mutable reference in scope will overwrite the memory at that location and invalidate the reference", idx),
            Self::IndexIsNotFree(idx) => write!(f, "Index [{}] is not free and may be still in use, cannot overwrite\n---------\nWriting a new value to this index will cause any keys referencing the old value to return errors. If this is truly the behavior you want, use Prison::overwrite() instead of Prison::insert()", idx),
            Self::MaximumCapacityReached => write!(f, "Prison has reached the maximum capacity allowed by Rust\n---------\nRust does not allow a [Vec] to have a capacity longer than [isize::MAX] becuase most operating systems only allow half of the total memory space to be addressed by programs"),
        }
    }
}

impl Error for AccessError {}

/// Struct that defines a packaged index into a [Prison](crate::single_threaded::Prison)
/// 
/// This struct is designed to be passed to some other struct or function that needs to be able to
/// reference the data stored at the cell number.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct CellKey {
    idx: usize,
    gen: usize,
}

impl CellKey {
    /// Create a new index from an index and generation
    /// 
    /// Not recomended in most cases, as there is no way to guarantee an item with that
    /// exact index and generation exists in your [Prison](crate::single_threaded::Prison)
    pub fn from_raw_parts(idx: usize, gen: usize) -> CellKey {
        return CellKey { idx, gen };
    }

    /// Return the internal index and generation from the cell key
    /// 
    /// Not recomended in most cases. If you need just the index by itself,
    /// use [CellKey::idx()] instead
    pub fn into_raw_parts(&self) -> (usize, usize) {
        return (self.idx, self.gen);
    }

    /// Return only the index of the [CellKey]
    /// 
    /// Useful if you want to only get the value at the specified index in the [Prison](crate::single_threaded::Prison)
    /// without checking that the generations match
    pub fn idx(&self) -> usize {
        return self.idx
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