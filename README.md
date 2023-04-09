# grit-data-prison
This crate provides the generic type `Prison<T>`, a data structure that uses an underlying `Vec<T>` to store values of the same type, but allows simultaneous interior mutability to each and every  value by providing `.visit()` methods that take closures that are passed mutable references to the values.

This documentation describes the usage of `Prison<T>`, how its `Vec` analogous methods differ from those found on a `Vec`, how to use its unusual `.visit()` methods, and how it achieves memory safety.
# Motivation
I wanted a data structure that met these criteria:
- Backed by a `Vec<T>` (or similar) for cache efficiency
- Allowed interior mutability to each of its elements
- Was fully memory safe (**needs verification**)
- Always returned a relevant error instead of panicking
- Was easier to reason about when and where it might error than reference counting
# Usage
This crate is [on crates.io](https://crates.io/crates/grit-data-prison)
First, add this crate as a dependency to your project:
```toml
[dependencies]
grit-data-prison = "0.1.2"
```
Then import [`AccessError`] from the crate root, along with the relevant version you wish to use in the file where it is needed (right now only one flavor is available, [`single_threaded`]):
```rust
use  grit_data_prison::{AccessError, single_threaded::Prison};
```
Create a prison and add your data to it (NOTE that it does not have to be declared `mut`)
```rust
let  prison: Prison<String> =  Prison::new();
prison.push(String::from("Hello, "));
prison.push(String::from("World!"));
```
You can then use one of the `.visit()` methods to access a mutable reference to your data from within a closure
```rust
prison.visit(1, |val_at_idx_1| {
	*val_at_idx_1  =  String::from("Rust!!");
});
```
Visiting multiple values at the same time can be done by nesting `.visit()` calls, or by using one of the batch `.visit()` methods
```rust
prison.visit(0, |val_0| {
	prison.visit(1, |val_1| {
		println!("{}{}", *val_0, *val_1); // Prints "Hello, Rust!!"
	});
});
prison.visit_many(&[0, 1], |vals| {
	println!("{}{}", vals[0], vals[1]); // Also prints "Hello, Rust!!"
});
```
Operations that affect the underlying Vector can also be done from *within* `.visit()` closures as long as none of the following rules are violated:
- The operation does not remove, read, or modify any element that is *currently* being visited
 - The operation does not cause a re-allocation of the entire Vector (or otherwise cause the entire Vector to relocate to another memory address)
```rust
let  prison: Prison<u64> =  Prison::with_capacity(10);
prison.push(0);
prison.push(10);
prison.push(20);
prison.push(30);
prison.push(42);
let  mut  accidental_val: u64  =  0;
prison.visit(3, |val| {
	accidental_val  =  prison.pop().unwrap();
	prison.push(40);
});
```
For more examples, see the [full documentation](https://docs.rs/grit-data-prison/0.1.2/grit_data_prison/)
# Why this strange syntax?
Closures provide a safe sandbox to access mutable references, as they cant be moved out of the closure, and because they use generics the rust compiler can choose to inline them in many/most cases.
# How is this safe?!
The short answer is: it *should* be *mostly* safe.

I welcome any feedback and analysis showing otherwise so I can fix it or revise my methodology.

[Prison] follows a few simple rules:
- One and ONLY one reference to any element can be in scope at any given time
- Because we are only allowing one reference, that one reference will always be a mutable reference
 - Any method that would or *could* read, modify, or delete any element cannot be performed while that element is currently being visited
- Any method that would or *could* cause the underlying Vector to relocate to a different spot in memory cannot be performed while even ONE visit is in progress

It achieves all of the above with a few lightweight sentinel values:
- A single `UnsafeCell` to hold *all* of the Prison internals and provide interior mutability
- A master `visit_count: usize` on Prison itself to track whether *any* visit is in progress
- A `locked: bool` on each element that prevents getting 2 mutable references to the same element

Compared to the number of sentinel values when using, for example, `Rc<RefCell<T>>`, this is quite a light-weight solution

Attempting to perform an action that would violate any of these rules will either be prevented from compiling or return an [AccessError] that describes why it was an error, and should never panic.
### Example: compile-time safety
```rust
let  prison: Prison<String> =  Prison::new();
prison.push(String::from("cannot be stolen"));
let  mut  steal_mut_ref: &mut  String  =  String::new();
let  mut  steal_prison: Prison<bool> =  Prison::new();
prison.visit(0, |mut_ref| {
	// will not compile: (error[E0521]: borrowed data escapes outside of closure)
	steal_mut_ref  =  mut_ref;
	// will not compile: (error[E0505]: cannot move out of `prison` because it is borrowed)
	steal_prison  =  prison;
});
```
### Example: run-time safety
```rust
struct  MyStruct(u32);
fn  main() {
	// Note this prison can only hold 2 elements
	let  prison: Prison<MyStruct> =  Prison::with_capacity(2); 
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
This crate is very much UNSTABLE, meaning that not every error condition may have a test, methods may return different errors/values as my understanding of how they should be properly implemented evolves, I may add/remove methods altogether, etc.

**In particular** I plan on moving from a simple `usize` index approach to a **Generational Arena** approach, where each element is indexed by both a `usize` and `u64` indicating which insert operation was responsible for them.

Since reallocating faces particular challenges in this data structure, this would allow me to recycle empty indices and relax restictions on adding and removing elements without running into the [ABA problem](https://en.wikipedia.org/wiki/ABA_problem).

For example, the implementation might look similar to the crate [generational-arena](https://crates.io/crates/generational-arena), however I will probably do it a little differently on the inside.

Other possible future additions may include:
[x] Single-thread safe `Prison<T>`
[ ] Multi-thread safe `AtomicPrison<T>`
[ ] ? Single standalone value version, `JailCell<T>`
[ ] ? Multi-thread safe standalone value version, `AtomicJailCell<T>`
[ ] ? Completely unchecked and unsafe version `UnPrison<T>`
[ ] ??? Multi-thread ~~safe~~ unsafe version `AtomicUnPrison<T>`
# How to Help/Contribute
This crate is [on crates.io](https://crates.io/crates/grit-data-prison)
The repo is [on github](https://github.com/gabe-lee/grit-data-prison)
Feel free to leave feedback!

If you can give me concrete examples that *definitely* violate memory-safety, meaning that the provided mutable references can be made to point to invalid/illegal memory (without the use of additional unsafe :P), or otherwise cause unsafe conditions (for example changing an expected enum variant to another where the compiler doesn't expect it to be possible), I'd love to fix, further restrict, or rethink the crate entirely.
