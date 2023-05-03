![Image](https://img.shields.io/badge/coverage-90.67%25-green) ![Image](https://img.shields.io/badge/unit%20tests-34%20pass%20%7C%200%20fail%20%7C%201%20ignore-brightgreen) ![Image](https://img.shields.io/badge/doc%20tests-100%20pass%20%7C%200%20fail-brightgreen) ![Image](https://img.shields.io/badge/dependencies-none-brightgreen) ![Image](https://img.shields.io/badge/license-BSD--3--Clause-informational)  
This crate provides the struct [Prison<T>](crate::single_threaded::Prison), a generational arena data structure
that allows simultaneous interior mutability to each and every element by providing `.visit()` methods
that take closures that are passed mutable references to the values, or by using the `.guard()` methods to
obtain a guarded mutable reference to the value.

This documentation describes the usage of [Prison<T>](crate::single_threaded::Prison), how its methods differ from
those found on a [Vec], how to access the data contained in it, and how it achieves memory safety.

### Project Links
grit-data-prison on [Crates.io](https://crates.io/crates/grit-data-prison)  
grit-data-prison on [Lib.rs](https://lib.rs/crates/grit-data-prison)  
grit-data-prison on [Github](https://github.com/gabe-lee/grit-data-prison)  
grit-data-prison on [Docs.rs](https://docs.rs/grit-data-prison/0.3.0/grit_data_prison/)  

### Quick Look
- Uses an underlying [Vec<T>] to store items of the same type
- Acts primarily as a Generational Arena, where each element is accessed using a [CellKey] that differentiates two values that may have been located at the same index but represent fundamentally separate data
- Can *also* be indexed with a plain [usize] for simple use cases
- Provides safe (***needs verification***) interior mutability by doing reference counting on each element to adhere to Rust's memory safety rules
- Uses [usize] refernce counters on each element and a master [usize] counter to track the number/location of active references and prevent mutable reference aliasing and disallow scenarios that could invalidate existing references
- [CellKey] uses a [usize] index and [usize] generation to match an index to the context in which it was created and prevent two unrelated values that both at some point lived at the same index from being mistaken as equal
- All methods return an [AccessError] where the scenario would cause a panic if not caught

### NOTE
This package is still UNSTABLE and may go through several iterations before I consider it good enough to set in stone, see [changelog](#changelog)
- Version 0.3.x is a breaking api change for 0.2.x and older
- ALSO: Version 0.2.x and older were discovered to have a soft memory leak when using `insert_at()` and `overwrite()`, see [changelog](#changelog) for details
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
let prison: Prison<String> = Prison::new();
let key_hello = prison.insert(String::from("Hello, "))?;
prison.insert(String::from("World!"))?;
```
From here there are 2 main ways to access the values contained in the [Prison](crate::single_threaded::Prison)
## Visiting the values in prison
You can use one of the `.visit()` methods to access a mutable reference
to your data from within a closure, either mutably or immutably
```rust
prison.visit_mut_idx(1, |val_at_idx_1| {
    *val_at_idx_1 = String::from("Rust!!");
    Ok(())
});
```
The rules for mutable or immutable references are the same as Rust's rules for normal variable referencing:
- ONLY one mutable reference
- OR any number of immutable references

Visiting multiple values at the same time can be done by nesting `.visit()` calls,
or by using one of the batch `.visit()` methods
```rust
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
let prison: Prison<String> = Prison::new();
let key_hello = prison.insert(String::from("Hello, "))?;
prison.insert(String::from("World!"))?;
let grd_hello = prison.guard_ref(key_hello)?;
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
{
    let grd_hello = prison.guard_ref(key_hello)?;
    let grd_world = prison.guard_ref_idx(1)?;
    println!("{}{}", *grd_hello, *grd_world); // Prints "Hello, World!"
}
// block ends, both guards go out of scope and their reference counts return to what they were before
let mut grd_world_to_rust = prison.guard_mut_idx(1)?;
*grd_world_to_rust = String::from("Rust!!");
PrisonValueMut::unguard(grd_world_to_rust); // index one is no longer marked mutably referenced
let grd_both = prison.guard_many_ref_idx(&[0, 1])?;
println!("{}{}", grd_both[0], grd_both[1]); // Prints "Hello, Rust!!"
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
    // block ends, both guards go out of scope and their reference counts return to what they were before
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
```
Also provided is a quick shortcut to clone values out of the [Prison<T>](crate::single_threaded::Prison)
when type T implements [Clone]. Because cloning values does not alter the original or presume any
precondition regarding the content of the value, it is safe (in a single-threaded context) to
clone values that are currently being guarded or visited.
### Example
```rust
let prison: Prison<String> = Prison::new();
let key_0 = prison.insert(String::from("Foo"))?;
prison.insert(String::from("Bar"))?;
let cloned_foo = prison.clone_val(key_0)?;
let cloned_bar = prison.clone_val_idx(1)?;
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
- Each element is (basically) a `Cell` or `Free` variant:
    - `Free` elements act as nodes in a doubly linked list that tracks free indexes
        - One [usize] that points to the previous free index before this one was made free
        - One [usize] that points to the next free index after this one is filled
    - `Cell`
        - A `ref_count` [usize] that tracks both mutable and immutable references
        - A `generation` [usize] to use when matching to the [CellKey] used to access the index
        - A value of type `T`

(see [performance](#performance) for more info on the *actual* specifics)

Attempting to perform an action that would violate any of these rules will either be prevented from compiling
or return an [AccessError] that describes why it was an error, and should never panic.
### Example: compile-time safety
```rust
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
```
### Example: run-time safety
```rust
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
# Crate Features
`no_std`: This crate can be used with the `no_std` feature to use only imports from the `core` library instead of the `std` library

Major Malfunctions:  
this crate can be passed one of three (optional) features that define how the library handles behavior that is DEFINITELY un-intended and should be considered a bug in the library itself. It defaults to `major_malf_is_err` if none are specified:
- `major_malf_is_err`: major malfunctions will be returned as an [AccessError::MAJOR_MALFUNCTION(msg)], this is the default even if not specified
- `major_malf_is_panic`: major malfunctions will result in a call to `panic(msg)` describing the unexpected behavior
- `major_malf_is_undefined`: branches where a major malfunction would nomally be are replaced with [unreachable_unchecked()], possibly allowing them to be removed from compilation entirely
# Performance

### Speed
(Benchmarks are Coming Soon™)

### Size
[Prison<T>](crate::single_threaded::Prison) has 4 [usize] house-keeping values in addition to a [Vec<PrisonCell<T>>]

Although the abstract of each `PrisonCell<T>` is as described as found in [How is This Safe?!](#how-is-this-safe),
the truth of the matter it that Rust was not optimising the memory footprint where it could have done so using Enums, so I had to roll my own
type of enum:
- Each element is a struct with a custom-enforced a `Cell` or `Free` variant, with the variant tracked in the top bit of one of its fields:
    - field `refs_or_next` holds a [usize] that holds either the reference count in `Cell` variant or the next free in `Free` variant
    - field `d_gen_or_prev` holds a [usize] that holds either the generation count in `Cell` variant or the prev free in `Free` variant
        - In addition, the most significant bit of `d_gen_or_prev` is reserved for marking the variant of the `PrisonCell` (the `d` is for `discriminant`). This means the *ACTUAL* maximum generation count is [isize::MAX](std::isize::MAX), but the prev index is unafected because a [Vec] cannot have more than [isize::MAX](std::isize::MAX) elements anyway...
    - field `val` is a [`MaybeUninit<T>`] that is always assumed uninitialized when the element is in `Free` state, and always assumed initialized when it is in `Cell` state.

Therefore the total _additional_ size compared to a [Vec<T>] on a 64-bit system is 32 bytes flat + 16 bytes per element,
and these values are validated in the test suite with an optional test that checks [mem::size_of](std::mem::size_of) for several
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
- [x] ? Single standalone value version, [JailCell<T>](crate::single_threaded::JailCell)
- [ ] ? Multi-thread safe standalone value version, `AtomicJailCell<T>`
- [ ] ?? Completely unchecked and unsafe version `UnPrison<T>`
- [ ] ??? Multi-thread ~~safe~~ unsafe version `AtomicUnPrison<T>`

# How to Help/Contribute

This crate is [on crates.io](https://crates.io/crates/grit-data-prison)
The repo is [on github](https://github.com/gabe-lee/grit-data-prison)

Feel free to leave feedback, or fork/branch the project and submit fixes/optimisations!

If you can give me concrete examples that *definitely* violate memory-safety, meaning
that the provided references can be made to point to invalid/illegal memory or violate aliasing rules
(without the use of additional unsafe :P), leak memory, or otherwise cause unsafe conditions (for
example changing an expected enum variant to another where the compiler doesnt expect it
to be possible), I'd love to fix, further restrict, or rethink the crate entirely.

The best way to do this would be to follow these steps:
- make a `bug/something` or `issue/something` branch off of the `dev` branch
- create a new test that demonstrates the current failing of the library
- then do one of the following:
    - solve the problem in your branch and create a pull request into the `dev` branch with a message explaining everything
    - create a pull request with only the test proving the failure point with a message describing why it is a failure and that *this pull request does not solve the problem*
# Changelog
 - Version 0.3.0: MAJOR BREAKING change to API:
     - Switch to reference counting instead of [bool] locks: the memory footprint is the same (in most cases) and the safety logic is almost the same. Reference counting gives more flexibility and finer grained control with no real penalty compared to using a [bool]
     - `escort()` methods renamed to `guard()` methods
     - `visit()` and `guard()` methods split into `_ref()` and `_mut()` variants
     - [AccessError] variants renamed and changed to be more clear
     - Addition of 3 crate features: `major_malf_is_err`, `major_malf_is_panic`, `major_malf_is_undefined` that allow conditional compilation choices for behavior that is *certainly* a bug in the library
     - **Version 0.2.x and older discovered to have a soft memory leak and should be avoided:** when using `insert_at()` and `overwrite()` on indexes that werent the 'top' free in the stack, all other free indexes above them in the stack would be forgotten and never re-used. However, they should be freed when the entire Prison is freed. Sorry! 
 - Version 0.2.3: Non-Breaking feature: `clone_val()` methods to shortcut cloning a value when T implements [Clone]
 - Version 0.2.2: Non-Breaking update to `PrisonValue` and `PrisonSlice` to reduce their memory footprint
 - Version 0.2.1: Non-breaking addition of `escort()` api function (why didnt I think of this earlier?)
 - Version 0.2.x: has a different API than version 0.1.x and is a move from a plain Vec to a Generational Arena
 - Version 0.1.x: first version, plain old [Vec] with [usize] indexing