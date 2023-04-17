This crate provides the struct [Prison<T>](crate::single_threaded::Prison), a generational arena data structure 
that allows simultaneous interior mutability to each and every element by providing `.visit()` methods
that take closures that are passed mutable references to the values, or by using the `.escort()` methods to
obtain a guarded mutable reference to the value.

This documentation describes the usage of [Prison<T>](crate::single_threaded::Prison), how its methods differ from
those found on a [Vec], how to use its unusual `.visit()` methods, and how it achieves memory safety.

[On: Crates.io](https://crates.io/crates/grit-data-prison)  
[On: Github](https://github.com/gabe-lee/grit-data-prison)  
[On: Docs.rs](https://docs.rs/grit-data-prison/0.2.3/grit_data_prison/)  

### Quick Look
- Uses an underlying [Vec<T>] to store items of the same type
- Acts primarily as a Generational Arena, where each element is accessed using a [CellKey] that differentiates two values that may have been located at the same index but represent fundamentally separate data
- Can *also* be indexed with a plain [usize] for simple use cases
- Provides safe (***needs verification***) interior mutability by only providing mutable references to values using closures to define strict scopes where they are valid and hide the setup/teardown for safety checks
- Uses [bool] locks on each element and a master [usize] counter to track the number/location of active references and prevent mutable reference aliasing and disallow scenarios that could invalidate existing references
- [CellKey] uses a [usize] index and [usize] generation to match an index to the context in which it was created and prevent two unrelated values that both at some point lived at the same index from being mistaken as equal
- All methods return an [AccessError] where the scenario would cause a panic if not caught

### NOTE
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
let prison: Prison<String> = Prison::new();
let key_hello = prison.insert(String::from("Hello, "))?;
prison.insert(String::from("World!"))?;
```
From here there are 2 main ways to access the values contained in the [Prison](crate::single_threaded::Prison)
## Visiting the values in prison
You can use one of the `.visit()` methods to access a mutable reference
to your data from within a closure
```rust
prison.visit_idx(1, |val_at_idx_1| {
    *val_at_idx_1 = String::from("Rust!!");
    Ok(())
});
```
Visiting multiple values at the same time can be done by nesting `.visit()` calls,
or by using one of the batch `.visit()` methods
```rust
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
```
### Full Visit Example Code
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
## Escorting the values out of the prison temporarily
You can also use one of the `.escort()` methods to obtain a guarded wrapper around your data as well,
perventing any other access to that element while the value is in scope. 

First you need to import [EscortedValue](crate::single_threaded::EscortedValue) or
[EscortedSlice](crate::single_threaded::EscortedSlice) from the same module as
[Prison](crate::single_threaded::Prison)
```rust
use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue, EscortedSlice}};
```
Then obtain an [EscortedValue](crate::single_threaded::EscortedValue) by using `.escort()`
```rust
let prison: Prison<String> = Prison::new();
let key_hello = prison.insert(String::from("Hello, "))?;
prison.insert(String::from("World!"))?;
let esc_hello = prison.escort(key_hello)?;
```
As long as the value isnt being visited or escorted, you can escort (or visit) that value, even when other values from the same
prison are being visited or escorted. [EscortedValue](crate::single_threaded::EscortedValue) keeps the element locked
until it goes out of scope. This can be done by wrapping the area it is used in a code block, or by manually
calling `.unescort()` on it to cause it to go out of scope an unlock immediately.

To access the data inside an [EscortedValue](crate::single_threaded::EscortedValue) you dereference it,
and to access the values in an [EscortedSlice](crate::single_threaded::EscortedSlice) you index into it
```rust
{
    let esc_hello = prison.escort(key_hello)?;
    let esc_world = prison.escort_idx(1)?;
    println!("{}{}", *esc_hello, *esc_world); // Prints "Hello, World!"
}
// block ends, both escorts go out of scope and their values unlock
let mut esc_world_to_rust = prison.escort_idx(1)?;
*esc_world_to_rust = String::from("Rust!!");
esc_world_to_rust.unescort(); // index one is returned and unlocked manually
let esc_both = prison.escort_many_idx(&[0, 1])?;
println!("{}{}", esc_both[0], esc_both[1]); // Prints "Hello, Rust!!"
```
### Full Escort Example Code
```rust
use grit_data_prison::{AccessError, CellKey, single_threaded::{Prison, EscortedValue, EscortedSlice}};

fn main() -> Result<(), AccessError> {
    let prison: Prison<String> = Prison::new();
    let key_hello = prison.insert(String::from("Hello, "))?;
    prison.insert(String::from("World!"))?;
    {
        let esc_hello = prison.escort(key_hello)?;
        let esc_world = prison.escort_idx(1)?;
        println!("{}{}", *esc_hello, *esc_world); // Prints "Hello, World!"
    }
    // block ends, both escorts go out of scope and their values unlock
    let mut esc_world_to_rust = prison.escort_idx(1)?;
    *esc_world_to_rust = String::from("Rust!!");
    esc_world_to_rust.unescort(); // index one is returned and unlocked manually
    let esc_both = prison.escort_many_idx(&[0, 1])?;
    println!("{}{}", esc_both[0], esc_both[1]); // Prints "Hello, Rust!!"
    Ok(())
}
```
Operations that affect the underlying [Vec] can also be done
from *within* `.visit()` closures or while values are `escort()`-ed as long as none of the following rules are violated:
- The operation does not remove, read, or modify any element that is *currently* being visited or escorted
- The operation does not cause a re-allocation of the entire [Vec] (or otherwise cause the entire [Vec] to relocate to another memory address)
```rust
let prison: Prison<u64> = Prison::with_capacity(5);
prison.insert(0)?;
prison.insert(10)?;
prison.insert(20)?;
prison.insert(30)?;
prison.insert(42)?;
let mut accidental_val: u64 = 0;
let mut esc_0 = prison.escort_idx(0)?;
prison.visit_idx(3, |val| {
    accidental_val = prison.remove_idx(4)?;
    prison.insert_at(4, 40);
    Ok(())
});
*esc_0 = 80;
esc_0.unescort();
// No values are visited or escorted here so we can perform
// an action that would cause re-allocation safely
for i in 0..100u64 {
    prison.insert(i + 100)?;
}
```
For more examples, see the specific documentation for the relevant type/method
 
# Why this strange syntax?
 
For the `visit()` methodology, closures provide a safe sandbox to access mutable references, as they cant be moved out of the closure,
and because the `visit()` functions that take the closures handle all of the
safety and housekeeping needed before and after.
 
Since closures use generics the rust compiler can inline them in many/most/all? cases.

The `escort()` methodology requires the values not be able to leak, alias, or never unlock, 
so they are wrapped in structs that provide limited access to the mutable references and know how to
automatically unlock the value when they go out of scope
 
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
```rust
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
```
### Example: run-time safety
```rust
struct MyStruct(u32);

fn main() -> Result<(), AccessError> {
    let prison: Prison<MyStruct> = Prison::with_capacity(2); // Note this prison can only hold 2 elements
    let key_0 = prison.insert(MyStruct(1))?;
    prison.insert(MyStruct(2))?;
    let esc_0 = prison.escort(key_0)?;
    assert!(prison.escort(key_0).is_err());
    assert!(prison.escort_idx(0).is_err());
    esc_0.unescort();
    prison.visit(key_0, |val_0| {
        assert!(prison.visit(key_0, |val_0_again| Ok(())).is_err());
        assert!(prison.visit_idx(0, |val_0_again| Ok(())).is_err());
        assert!(prison.visit_idx(3, |val_3_out_of_bounds| Ok(())).is_err());
        assert!(prison.escort(key_0).is_err());
        assert!(prison.escort_idx(3).is_err());
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
- [x] `Escort` api for a more Rust-idiomatic way to access values
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
 - Version 0.2.2: Non-Breaking update to [EscortedValue] and [EscortedSlice] to reduce their memory footprint
 - Version 0.2.1: Non-breaking addition of `escort()` api function (why didnt I think of this earlier?)
 - Version 0.2.x: has a different API than version 0.1.x and is a move from a plain Vec to a Generational Arena
 - Version 0.1.x: first version, plain old [Vec] with [usize] indexing