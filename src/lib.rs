//REGION MAIN DOCUMENTATION
/*!This crate provides the struct [Prison<T>](crate::single_threaded::Prison), a generational arena data structure
that allows simultaneous interior mutability to each and every element by providing `.visit()` methods
that take closures that are passed mutable references to the values, or by using the `.guard()` methods to
obtain a guarded mutable reference to the value.

This documentation describes the usage of [Prison<T>](crate::single_threaded::Prison), how its methods differ from
those found on a [Vec], how to access the data contained in it, and how it achieves memory safety.

[On: Crates.io](https://crates.io/crates/grit-data-prison)
[On: Github](https://github.com/gabe-lee/grit-data-prison)
[On: Docs.rs](https://docs.rs/grit-data-prison/0.2.3/grit_data_prison/)

### Quick Look
- Uses an underlying [Vec<T>] to store items of the same type
- Acts primarily as a Generational Arena, where each element is accessed using a [CellKey] that differentiates two values that may have been located at the same index but represent fundamentally separate data
- Can *also* be indexed with a plain [usize] for simple use cases
- Provides safe (***needs verification***) interior mutability by doing reference counting on each element to adhere to Rust's memory safety rules
- Uses [usize] refernce counters on each element and a master [usize] counter to track the number/location of active references and prevent mutable reference aliasing and disallow scenarios that could invalidate existing references
- [CellKey] uses a [usize] index and [usize] generation to match an index to the context in which it was created and prevent two unrelated values that both at some point lived at the same index from being mistaken as equal
- All methods return an [AccessError] where the scenario would cause a panic if not caught

### NOTE
This package is still UNSTABLE and may go through several iterations before I consider it good enough to set in stone
- Version 0.3.x is a breaking api change for 0.2.x and older
    - Version 0.2.x and older were discovered to have a soft memory leak when using `insert_at()` and `overwrite()`, see [changelog](#changelog)
- Version 0.2.x is a breaking api change for 0.1.x and older
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
grit-data-prison = "0.3"
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
From here there are 2 main ways to access the values contained in the [Prison](crate::single_threaded::Prison)
## Visiting the values in prison
You can use one of the `.visit()` methods to access a mutable reference
to your data from within a closure, either mutably or immutably
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
# fn main() -> Result<(), AccessError> {
# let prison: Prison<String> = Prison::new();
# let key_hello = prison.insert(String::from("Hello, "))?;
# prison.insert(String::from("World!"))?;
prison.visit_mut_idx(1, |val_at_idx_1| {
    *val_at_idx_1 = String::from("Rust!!");
    Ok(())
});
# Ok(())
# }
```
The rules for mutable or immutable references are the same as Rust's rules for normal variable referencing:
- ONLY one mutable reference
- OR any number of immutable references

Visiting multiple values at the same time can be done by nesting `.visit()` calls,
or by using one of the batch `.visit()` methods
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};
# fn main() -> Result<(), AccessError> {
# let prison: Prison<String> = Prison::new();
# let key_hello = prison.insert(String::from("Hello, "))?;
# prison.insert(String::from("World!"))?;
# prison.visit_mut_idx(1, |val_at_idx_1| {
#   *val_at_idx_1 = String::from("Rust!!");
#   Ok(())
# });
prison.visit_ref(key_hello, |val_0| {
    prison.visit_ref_idx(1, |val_1| {
        println!("{}{}", *val_0, *val_1); // Prints "Hello, Rust!!"
        Ok(())
    });
    Ok(())
});
prison.visit_many_ref_idx(&[0, 1], |vals| {
    println!("{}{}", vals[0], vals[1]); // Also prints "Hello, Rust!!"
    Ok(())
});
# Ok(())
# }
```
### Full Visit Example Code
```rust
use grit_data_prison::{AccessError, CellKey, single_threaded::Prison};

fn main() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::new();
    let key_hello = prison.insert(String::from("Hello, "))?;
    prison.insert(String::from("World!"))?;
    prison.visit_mut_idx(1, |val_at_idx_1| {
        *val_at_idx_1 = String::from("Rust!!");
        Ok(())
    });
    prison.visit_ref(key_hello, |val_0| {
        prison.visit_ref_idx(1, |val_1| {
            println!("{}{}", *val_0, *val_1); // Prints "Hello, Rust!!"
            Ok(())
        });
        Ok(())
    });
    prison.visit_many_ref_idx(&[0, 1], |vals| {
        println!("{}{}", vals[0], vals[1]); // Also prints "Hello, Rust!!"
        Ok(())
    });
    Ok(())
}
```
## Guarding values with wrapper structs
You can also use one of the `.guard()` methods to obtain a guarded wrapper around your data,
keeping the value marked as referenced as long as the wrapper remains in scope.

First you need to import one of [PrisonValueMut](crate::single_threaded::PrisonValueMut),
[PrisonValueRef](crate::single_threaded::PrisonValueRef), [PrisonSliceMut](crate::single_threaded::PrisonSliceMut),
or [PrisonSliceRef](crate::single_threaded::PrisonSliceRef) from the same module as
[Prison](crate::single_threaded::Prison)
```rust
use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef}};
```
Then obtain a guarded wrapper by using the corresponding `.guard()` method
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef, PrisonValueMut}};
# fn main() -> Result<(), AccessError> {
let prison: Prison<String> = Prison::new();
let key_hello = prison.insert(String::from("Hello, "))?;
prison.insert(String::from("World!"))?;
let grd_hello = prison.guard_ref(key_hello)?;
# Ok(())
# }
```
As long as the referencing rules aren't violated, you can guard (or visit) that value, even when other values from the same
prison are being visited or guarded. The guarded wrappers (for example [PrisonValueRef](crate::single_threaded::PrisonValueRef))
keep the element(s) marked with the appropriate form of referencing until they go out of scope.
This can be done by wrapping the area it is used in a code block, or by manually passing it to the
associated `::unguard()` function on the wrapper type to immediately drop it out of scope and update the
reference count.

The guarded wrapper types all implement [Deref], [AsRef], and [Borrow], while the mutable versions
also implement [DerefMut], [AsMut], and [BorrowMut] to provide transparent access to their inner values
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef, PrisonValueMut, PrisonSliceRef}};
# fn main() -> Result<(), AccessError> {
# let prison: Prison<String> = Prison::new();
# let key_hello = prison.insert(String::from("Hello, "))?;
# prison.insert(String::from("World!"))?;
{
    let grd_hello = prison.guard_ref(key_hello)?;
    let grd_world = prison.guard_ref_idx(1)?;
    println!("{}{}", *grd_hello, *grd_world); // Prints "Hello, World!"
}
// block ends, both guards go out of scope and their reference countes return to what they were before
let mut grd_world_to_rust = prison.guard_mut_idx(1)?;
*grd_world_to_rust = String::from("Rust!!");
PrisonValueMut::unguard(grd_world_to_rust); // index one is no longer marked mutably referenced
let grd_both = prison.guard_many_ref_idx(&[0, 1])?;
println!("{}{}", grd_both[0], grd_both[1]); // Prints "Hello, Rust!!"
# Ok(())
# }
```
### Full Guard Example Code
```rust
use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueRef, PrisonValueMut, PrisonSliceRef}};

fn main() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::new();
    let key_hello = prison.insert(String::from("Hello, "))?;
    prison.insert(String::from("World!"))?;
    {
        let grd_hello = prison.guard_ref(key_hello)?;
        let grd_world = prison.guard_ref_idx(1)?;
        println!("{}{}", *grd_hello, *grd_world); // Prints "Hello, World!"
    }
    // block ends, both guards go out of scope and their reference countes return to what they were before
    let mut grd_world_to_rust = prison.guard_mut_idx(1)?;
    *grd_world_to_rust = String::from("Rust!!");
    PrisonValueMut::unguard(grd_world_to_rust); // index one is no longer marked mutably referenced
    let grd_both = prison.guard_many_ref_idx(&[0, 1])?;
    println!("{}{}", grd_both[0], grd_both[1]); // Prints "Hello, Rust!!"
    Ok(())
}
```
Operations that affect the underlying [Vec] can also be done
from *within* `.visit()` closures or while values are `guard()`-ed as long as none of the following rules are violated:
- The operation does not remove, read, or modify any element that is *currently* being referenced
- The operation does not cause a re-allocation of the entire [Vec] (or otherwise cause the entire [Vec] to relocate to another memory address)
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
# fn main() -> Result<(), AccessError> {
let prison: Prison<u64> = Prison::with_capacity(5);
prison.insert(0)?;
prison.insert(10)?;
prison.insert(20)?;
prison.insert(30)?;
prison.insert(42)?;
let mut accidental_val: u64 = 0;
let mut grd_0 = prison.guard_mut_idx(0)?;
prison.visit_ref_idx(3, |val| {
    accidental_val = prison.remove_idx(4)?;
    prison.insert(40)?;
    Ok(())
});
*grd_0 = 80;
PrisonValueMut::unguard(grd_0);
// No values are actively referenced here so we can perform
// an action that would cause re-allocation safely
for i in 0..100u64 {
    prison.insert(i + 100)?;
}
# Ok(())
# }
```
Also provided is a quick shortcut to clone values out of the [Prison<T>](crate::single_threaded::Prison)
when type T implements [Clone]. Because cloning values does not alter the original or presume any
precondition regarding the content of the value, it is safe (in a single-threaded context) to
clone values that are currently being guarded or visited.
### Example
```rust
# use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison}};
# fn main() -> Result<(), AccessError> {
let prison: Prison<String> = Prison::new();
let key_0 = prison.insert(String::from("Foo"))?;
prison.insert(String::from("Bar"))?;
let cloned_foo = prison.clone_val(key_0)?;
let cloned_bar = prison.clone_val_idx(1)?;
# Ok(())
# }
```
For more examples, see the specific documentation for the relevant types/methods

## JailCell
Also included is the struct [JailCell<T>](crate::single_threaded::JailCell), which acts as a stand-alone
version of a [Prison<T>](crate::single_threaded::Prison), but with no generation counter.

[JailCell](crate::single_threaded::JailCell) includes the same basic interface as [Prison](crate::single_threaded::Prison)
and also employs reference counting, but with a much simpler set of safety checks
```rust
use grit_data_prison::{AccessError, CellKey, single_threaded::{JailCell, JailValueRef}};

fn main() -> Result<(), AccessError> {
    let string_jail: JailCell<String> = JailCell::new(String::from("'Bad-Guy' Bert"));
    string_jail.visit_mut(|criminal| {
        let bigger_bad = String::from("Dr. Lego-Step");
        println!("Breaking News: {} to be set free to make room for {}", *criminal, bigger_bad);
        *criminal = bigger_bad;
        Ok(())
    })?;
    let guarded_criminal = string_jail.guard_ref()?;
    println!("{} will now be paraded around town for public shaming", *guarded_criminal);
    assert_eq!(*guarded_criminal, String::from("Dr. Lego-Step"));
    JailValueRef::unguard(guarded_criminal);
    Ok(())
}
```
See the documentation on [JailCell](crate::single_threaded::JailCell) for more info
# Why this strange syntax?

For the `visit()` methodology, closures provide a safe sandbox to access mutable references, as they cant be moved out of the closure,
and because the `visit()` functions that take the closures handle all of the
safety and housekeeping needed before and after.

Since closures use generics the rust compiler can inline them in many/most/all? cases.

The `guard()` methodology requires the values not be able to leak, alias, or never reset their reference counts,
so they are wrapped in structs that provide limited access to the references and know how to
automatically reset the reference counter for the value when they go out of scope

# How is this safe?!

The short answer is: it *should* be *mostly* safe.
I welcome any feedback and analysis showing otherwise so I can fix it or revise my methodology.

[Prison](crate::single_threaded::Prison) follows a few simple rules:
- You can only get an immutable reference if the value has zero references or only immutable references
- You can only get a mutable reference is the value has zero references of any type
- Any method that would or *could* read, modify, or delete any element cannot be performed while that element is currently being referenced
- Any method that would or *could* cause the underlying [Vec] to relocate to a different spot in memory cannot be performed while even ONE reference to ANY element in the [Vec] is still in scope

In addition, it provides the functionality of a Generational Arena with these additional rules:
- The [Prison](crate::single_threaded::Prison) has a master generation counter to track the largest generation of any element inside it
- Every valid element has a generation attatched to it, and `insert()` operations return a [CellKey] that pairs the element index with the current largest generation value
- Any operation that removes *or* overwrites a valid element *with a genreation counter that is equal to the largest generation* causes the master generation counter to increase by one

It achieves all of the above with a few lightweight sentinel values:
- A single [UnsafeCell](std::cell::UnsafeCell) to hold *all* of the [Prison](crate::single_threaded::Prison) internals and provide interior mutability
- A master `access_count` [usize] on [Prison](crate::single_threaded::Prison) itself to track whether *any* reference is in active
- Each element is either a `Cell` or `Free` variant:
    - A `Free` simply contains the value of the *next* free index after this one is filled
    - A `ref_count` [usize] on each `Cell` that tracks both mutable and immutable references
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
prison.visit_mut_idx(0, |mut_ref| {
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
# use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, PrisonValueMut}};
struct MyStruct(u32);

fn main() -> Result<(), AccessError> {
    let prison: Prison<MyStruct> = Prison::with_capacity(2); // Note this prison can only hold 2 elements
    let key_0 = prison.insert(MyStruct(1))?;
    prison.insert(MyStruct(2))?;
    let grd_0 = prison.guard_mut(key_0)?;
    assert!(prison.guard_mut(key_0).is_err());
    assert!(prison.guard_ref_idx(0).is_err());
    PrisonValueMut::unguard(grd_0);
    prison.visit_mut(key_0, |val_0| {
        assert!(prison.visit_mut(key_0, |val_0_again| Ok(())).is_err());
        assert!(prison.visit_ref(key_0, |val_0_again| Ok(())).is_err());
        assert!(prison.visit_mut_idx(0, |val_0_again| Ok(())).is_err());
        assert!(prison.visit_ref_idx(3, |val_3_out_of_bounds| Ok(())).is_err());
        assert!(prison.guard_mut(key_0).is_err());
        assert!(prison.guard_ref(key_0).is_err());
        assert!(prison.guard_ref_idx(3).is_err());
        prison.visit_ref_idx(1, |val_1| {
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
[Prison<T>](crate::single_threaded::Prison) has 4 [usize] house-keeping values in addition to a [Vec<PrisonCell<T>>]

Each `PrisonCell<T>` consists of:
- A [usize] that tracks whether the cell is `used` or `free` in its most significant bit, and either the `generation` or `prev_free` respectively
- A [usize] that tracks (depending on `used` or `free`) either the `reference_count` or the `next_free`
- A value field of type [MaybeUninit<T>] (guaranteed same size as `T`)

Therefore the total _additional_ size compared to a [Vec<T>] on a 64-bit system is 32 bytes flat + 16 bytes per element,
and these values are validated in the test suite with a test that checks [mem::size_of](std::mem::size_of) for several
types of `T`

# How this crate may change in the future

This crate is very much UNSTABLE, meaning that not every error condition may be tested,
methods may return different errors/values as my understanding of how they should be properly implemented
evolves, I may add/remove methods altogether, etc.

Possible future additions may include:
- [x] Single-thread safe [Prison<T>](crate::single_threaded::Prison)
- [x] `Guard` api for a more Rust-idiomatic way to access values
- [x] Switch to reference counting with same memory footprint
- [ ] More public methods (as long as they make sense and don't bloat the API)
- [ ] Multi-thread safe `AtomicPrison<T>`
- [x] ? Single standalone value version, `JailCell<T>`
- [ ] ? Multi-thread safe standalone value version, `AtomicJailCell<T>`
- [ ] ?? Completely unchecked and unsafe version `UnPrison<T>`
- [ ] ??? Multi-thread ~~safe~~ unsafe version `AtomicUnPrison<T>`

# How to Help/Contribute

This crate is [on crates.io](https://crates.io/crates/grit-data-prison)
The repo is [on github](https://github.com/gabe-lee/grit-data-prison)

Feel free to leave feedback, or fork/branch the project and submit fixes/optimisations!

If you can give me concrete examples that *definitely* violate memory-safety, meaning
that the provided references can be made to point to invalid/illegal memory or violate aliasing rules
(without the use of additional unsafe :P), or otherwise cause unsafe conditions (for
example changing an expected enum variant to another where the compiler doesnt expect it
to be possible), I'd love to fix, further restrict, or rethink the crate entirely.
# Changelog
 - Version 0.3.0: MAJOR BREAKING change to API:
     - Switch to reference counting instead of [bool] locks: the memory footprint is the *exact* same and the safety logic is almost the same. Reference counting gives more flexibility and finer grained control with no real penalty compared to using a [bool]
     - `escort()` methods renamed to `guard()` methods
     - `visit()` and `guard()` methods split into `_ref()` and `_mut()` variants
     - [AccessError] variants renamed and changed to be more clear
     - Addition of 3 crate features: `major_malf_is_err`, `major_malf_is_panic`, `major_malf_is_undefined` that allow conditional compilation choices for behavior that is *certainly* a bug in the library
     - **Version 0.2.x and older discovered to have a soft memory leak and should be avoided:** when using `insert_at()` and `overwrite()` on indexes that werent the 'top' free in the stack, all other free indexes above them in the stack would be forgotten and never re-used. However, they should be freed when the entire Prison is freed. Sorry! 
 - Version 0.2.3: Non-Breaking feature: `clone_val()` methods to shortcut cloning a value when T implements [Clone]
 - Version 0.2.2: Non-Breaking update to [PrisonValue](crate::single_threaded::PrisonValue) and [PrisonSlice](crate::single_threaded::PrisonSlice) to reduce their memory footprint
 - Version 0.2.1: Non-breaking addition of `escort()` api function (why didnt I think of this earlier?)
 - Version 0.2.x: has a different API than version 0.1.x and is a move from a plain Vec to a Generational Arena
 - Version 0.1.x: first version, plain old [Vec] with [usize] indexing
*/

#![deny(rustdoc::broken_intra_doc_links)]
#![deny(rustdoc::private_intra_doc_links)]
#![deny(missing_docs)]
#![allow(clippy::needless_return)]
#![allow(clippy::needless_lifetimes)]

//REGION Crate Imports
#[cfg(not(feature = "no_std"))]
pub(crate) use std::{
    error::Error,
    borrow::{Borrow, BorrowMut},
    hint::unreachable_unchecked,
    cell::UnsafeCell,
    fmt::{Debug, Display},
    mem::{MaybeUninit, replace as mem_replace},
    ops::{Deref, DerefMut, RangeBounds},
};

#[cfg(feature = "no_std")]
pub(crate) use core::{
    borrow::{Borrow, BorrowMut},
    hint::unreachable_unchecked,
    cell::UnsafeCell,
    fmt::{Debug, Display},
    mem::{MaybeUninit, replace as mem_replace},
    ops::{Deref, DerefMut, RangeBounds},
};

#[cfg(feature = "no_std")]
pub(crate) trait Error: Debug + Display {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        None
    }
}

/// Module defining the version(s) of [Prison<T>] and [JailCell<T>] suitable for use only from within a single-thread
pub mod single_threaded;

//ENUM AccessError
/// Error type that provides helpful information about why an operation on any
/// [Prison](crate::single_threaded::Prison) or [JailCell](crate::single_threaded::JailCell) failed
///
/// Every error returned from functions or methods defined in this crate will be one of these variants,
/// and all safe versions of [Prison](crate::single_threaded::Prison) and [JailCell](crate::single_threaded::JailCell) are designed to never panic and always return errors.
///
/// Additional variants may be added in the future, therefore it is recommended you add a catch-all branch
/// to any match statements on this enum to future-proof your code:
/// ```rust
/// # use grit_data_prison::AccessError;
/// # fn main() {
/// # let acc_err = AccessError::IndexOutOfRange(100);
/// match acc_err {
///     AccessError::IndexOutOfRange(bad_idx) => {},
///     AccessError::ValueAlreadyMutablyReferenced(duplicate_idx) => {},
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
    /// Indicates that an operation attempted to reference a value (mutably or immutably) already being mutably referenced by another operation,
    /// along with the index in question
    ValueAlreadyMutablyReferenced(usize),
    /// Indicates that an operation attempted to mutably reference a value already being immutably referenced by another operation,
    /// along with the index in question
    ValueStillImmutablyReferenced(usize),
    /// Indicates that an overwriteing insert would invalidate currently active references to a value
    OverwriteWhileValueReferenced(usize),
    /// Indicates that an insert would require re-allocation of the internal [Vec<T>], thereby invalidating
    /// any currently active references
    InsertAtMaxCapacityWhileAValueIsReferenced,
    /// Indicates that the last element in the [Prison<T>](crate::single_threaded::Prison) is being accessed, and `remove()`-ing the value
    /// from the underlying [Vec<T>] would invalidate the reference
    RemoveWhileValueReferenced(usize),
    /// Indicates that the value requested was deleted and a new value with an updated generation took its place
    ///
    /// Contains the index and generation from the invalid [CellKey], in that order
    ValueDeleted(usize, usize),
    /// Indicates that a very large number of removes and inserts caused the generation counter to reach its max value
    MaxValueForGenerationReached,
    /// Indicates that an attempted insert to a specific index would overwrite and invalidate a value still in use
    IndexIsNotFree(usize),
    /// Indicates that the underlying [Vec] reached the maximum capacity set by Rust ([isize::MAX])
    MaximumCapacityReached,
    /// Indicates that you (somehow) reached the limit for reference counting immutable references
    MaximumImmutableReferencesReached(usize),
    /// Indicates that the operation created an invalid and unexpected state. This may have resulted in memory leaking, mutable aliasing, undefined behavior, etc.
    /// 
    /// This error should be considered a BUG inside the library crate `grit-data-prison` and reported to the author of the crate
    #[allow(non_camel_case_types)]
    MAJOR_MALFUNCTION(String)
}

impl AccessError {
    /// Returns a string that shows the [AccessError] variant and value, if any
    pub fn kind(&self) -> String {
        match &*self {
            Self::IndexOutOfRange(idx) => format!("AccessError::IndexOutOfRange({})", idx),
            Self::ValueAlreadyMutablyReferenced(idx) => format!("AccessError::ValueAlreadyMutablyReferenced({})", idx),
            Self::ValueStillImmutablyReferenced(idx) => format!("AccessError::ValueStillImmutablyReferenced({})", idx),
            Self::InsertAtMaxCapacityWhileAValueIsReferenced => format!("AccessError::InsertAtMaxCapacityWhileAValueIsReferenced"),
            Self::ValueDeleted(idx, gen) => format!("AccessError::ValueDeleted({}, {})", idx, gen),
            Self::MaxValueForGenerationReached => format!("AccessError::MaxValueForGenerationReached"),
            Self::RemoveWhileValueReferenced(idx) => format!("AccessError::RemoveWhileValueReferenced({})", idx),
            Self::IndexIsNotFree(idx) => format!("AccessError::IndexIsNotFree({})", idx),
            Self::MaximumCapacityReached => format!("AccessError::MaximumCapacityReached"),
            Self::MaximumImmutableReferencesReached(idx) => format!("AccessError::MaximumImmutableReferencesReached({})", idx),
            Self::OverwriteWhileValueReferenced(idx)=> format!("AccessError::OverwriteWhileValueReferenced({})", idx),
            Self::MAJOR_MALFUNCTION(msg) => format!("AccessError::MAJOR_MALFUNCTION({})", msg),
        }
    }
}

impl Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &*self {
            Self::IndexOutOfRange(idx) => write!(f, "Index [{}] is out of range", idx),
            Self::ValueAlreadyMutablyReferenced(idx) => write!(f, "Value at index [{}] is already being mutably referenced by another operation", idx),
            Self::ValueStillImmutablyReferenced(idx) => write!(f, "Value at index [{}] is still being immutably referenced by another operation, cannot mutably reference", idx),
            Self::InsertAtMaxCapacityWhileAValueIsReferenced => write!(f, "Prison is at max capacity, cannot insert new value while any values are still referenced"),
            Self::ValueDeleted(idx, gen) => write!(f, "Value requested at index {} gen {} was already deleted", idx, gen),
            Self::MaxValueForGenerationReached => write!(f, "Maximum value for generation counter reached"),
            Self::RemoveWhileValueReferenced(idx) => write!(f, "Index [{}] is currently being referenced, cannot remove", idx),
            Self::IndexIsNotFree(idx) => write!(f, "Index [{}] is not free and may be still in use, cannot overwrite with unrelated value", idx),
            Self::MaximumCapacityReached => write!(f, "Prison has reached the maximum capacity allowed by Rust"),
            Self::MaximumImmutableReferencesReached(idx) => write!(f, "Value at index [{}] has reached the maximum number of immutable references: {}", idx, usize::MAX - 2),
            Self::OverwriteWhileValueReferenced(idx) => write!(f, "Value at index [{}] still has active references, cannot overwrite", idx),
            Self::MAJOR_MALFUNCTION(msg) => write!(f, "{}\n-------\nIndicates that the operation created an invalid and unexpected state. This may have resulted in memory leaking, mutable aliasing, undefined behavior, etc.", msg),
        }
    }
}

impl Debug for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &*self {
            Self::IndexOutOfRange(idx) => write!(f, "Index [{}] is out of range", idx),
            Self::ValueAlreadyMutablyReferenced(idx) => write!(f, "Value at index [{}] is already being mutably referenced by another operation\n---------\nMutably referencing the same cell twice or immutably referencing a value being mutably referenced violates Rust's memory saftey rules", idx),
            Self::ValueStillImmutablyReferenced(idx) => write!(f, "Value at index [{}] is still being immutably referenced by another operation, cannot mutably reference\n---------\nMutably referencing a cell while an immutable reference to it is still in scope violates Rust's memory saftey rules", idx),
            Self::InsertAtMaxCapacityWhileAValueIsReferenced => write!(f, "Prison is at max capacity, cannot insert new value while any values are still referenced\n---------\nInserting a value in a Vec at max capacity while a value reference is still in scope may cause re-allocation that will invalidate it"),
            Self::ValueDeleted(idx, gen) => write!(f, "Value requested at index {} gen {} was already deleted\n---------\nWhen deleting a value, it is recomended you take steps to invalidate any held keys refering to it", idx, gen),
            Self::MaxValueForGenerationReached => write!(f, "Maximum value for generation counter reached\n---------\nA large number of removals and inserts has caused the generation counter to reach its max value. Manually perform a Prison::purge() and re-issue the keys to continue using this Prison"),
            Self::RemoveWhileValueReferenced(idx) => write!(f, "Index [{}] is currently being referenced, cannot remove\n---------\nRemoving a value with an active reference in scope will could overwrite the memory at that location and cause undefined behavior", idx),
            Self::IndexIsNotFree(idx) => write!(f, "Index [{}] is not free and may be still in use, cannot overwrite with unrelated value\n---------\nWriting a new value to this index will cause any keys referencing the old value to return errors. If this is truly the behavior you want, use Prison::overwrite() instead of Prison::insert()", idx),
            Self::MaximumCapacityReached => write!(f, "Prison has reached the maximum capacity allowed by Rust\n---------\nRust does not allow a [Vec] to have a capacity longer than [isize::MAX] becuase most operating systems only allow half of the total memory space to be addressed by programs"),
            Self::MaximumImmutableReferencesReached(idx) => write!(f, "Value at index [{}] has reached the maximum number of immutable references: {}\n---------\nThis highly unlikely scenario means you somehow created {} immutable references to the value already", idx, usize::MAX - 2, usize::MAX - 2),
            Self::OverwriteWhileValueReferenced(idx)=> write!(f, "Value at index [{}] still has active references, cannot overwrite\n---------\nOverwriting a value with active references is the same as mutating a variable being immutably referenced, violating Rust's memory safety rules", idx),
            Self::MAJOR_MALFUNCTION(msg) => write!(f, "{}\n-------\nIndicates that the operation created an invalid and unexpected state. This may have resulted in memory leaking, mutable aliasing, undefined behavior, etc.\n---------\nThis error should be considered a BUG inside the library crate `grit-data-prison` and reported to the author of the crate", msg),
        }
    }
}

impl Error for AccessError {}

//STRUCT CellKey
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

    /// Return the internal index and generation from the cell key, in that order
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
        return self.idx;
    }
}

//REGION Crate Utilities
#[doc(hidden)]
fn extract_true_start_end<B>(range: B, max_len: usize) -> (usize, usize)
where
    B: RangeBounds<usize>,
{
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

macro_rules! internal {
    ($p:tt) => {
        unsafe { &mut *$p.internal.get() }
    };
}
pub(crate) use internal;

macro_rules! major_malfunction {
    ($MSG:expr) => {
        if cfg!(feature = "major_malf_is_err") {
            return Err(AccessError::MAJOR_MALFUNCTION($MSG));
        } else if cfg!(feature = "major_malf_is_panic") {
            panic!("{}", $MSG)
        } else if cfg!(feature = "major_malf_is_undefined") {
            unsafe { unreachable_unchecked() }
        } else {
            return Err(AccessError::MAJOR_MALFUNCTION($MSG));
        }
    };
}
pub(crate) use major_malfunction;
