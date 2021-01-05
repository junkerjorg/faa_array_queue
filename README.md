[![Latest version](https://img.shields.io/crates/v/faa_array_queue.svg)](https://crates.io/crates/faa_array_queue)
[![Documentation](https://docs.rs/faa_array_queue/badge.svg)](https://docs.rs/faa_array_queue)
![Lines of code](https://tokei.rs/b1/github/junkerjorg/faa_array_queue)
![MIT](https://img.shields.io/badge/license-MIT-blue.svg)

# faa_array_queue
Fetch-And-Add Array Queue (a lock free mpmc queue) implementation for Rust.

## Usage

Add these lines to your `Cargo.toml`:

```toml
[dependencies]
faa_array_queue = "0.1"
```

and use the queue like this:

```rust
use faa_array_queue::FaaArrayQueue;

let queue = FaaArrayQueue::<usize>::default();
queue.enqueue(1337);
assert!(queue.dequeue().unwrap() == 1337);
```

## License

Licensed under [MIT license](http://opensource.org/licenses/MIT)
