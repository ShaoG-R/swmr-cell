# SWMR-Cell: Version-Based Single-Writer Multi-Reader Cell

[![Crates.io](https://img.shields.io/crates/v/swmr-cell)](https://crates.io/crates/swmr-cell)
[![Documentation](https://docs.rs/swmr-cell/badge.svg)](https://docs.rs/swmr-cell)
[![License](https://img.shields.io/crates/l/swmr-cell)](LICENSE-MIT)

[中文文档](./README_CN.md)

`swmr-cell` provides a thread-safe `SwmrCell` that supports **concurrent wait-free reads** and **lock-free writes** (amortized) using version-based garbage collection. It is designed for high-performance scenarios where a single writer updates data frequently while multiple readers access it concurrently.

## Features

- **Single-Writer Multi-Reader (SWMR)**: Optimized for one writer thread and multiple concurrent reader threads.
- **Wait-Free Reads**: Readers obtain data snapshots without locking, blocking, or interfering with the writer.
- **Version-Based Garbage Collection**: Old data is automatically reclaimed when no readers are observing it, using a mechanism similar to RCU (Read-Copy-Update) or Epoch-based GC.
- **Snapshot Semantics**: A reader holds a consistent view of the data as long as the `PinGuard` is alive.
- **Minimal Synchronization**: 
  - Readers use atomic operations to register their active version.
  - The writer uses a mutex only for managing reader registration and scanning reader states during collection.
- **Configurable GC**: Supports automatic or manual garbage collection with tunable thresholds.
- **no_std Support**: Compatible with `no_std` environments using `alloc` and `spin`.
- **Type-Safe**: Full Rust ownership and lifetime safety.

## Usage

Add `swmr-cell` to your `Cargo.toml`:

```toml
[dependencies]
swmr-cell = "0.3"
```

### Read-Preferred Mode

By default, `swmr-cell` is optimized for **write-heavy** or balanced workloads. You can enable **read-preferred** mode via the builder, which uses specialized heavy/light memory barriers (via `swmr-barrier`). This makes readers wait-free in instruction cycles by shifting synchronization cost to the writer.

```rust
use swmr_cell::SwmrCell;

let mut cell = SwmrCell::builder()
    .read_preferred() // Optimize for read performance
    .build(42);
```

### no_std Support

To use `swmr-cell` in a `no_std` environment, disable the default features and enable the `spin` feature. Note that this crate relies on `alloc`, so a global allocator must be available.

```toml
[dependencies]
swmr-cell = { version = "0.3", default-features = false, features = ["spin"] }
```

### Basic Example

```rust
use swmr_cell::SwmrCell;

// 1. Create a new SWMR cell with an initial value
let mut cell = SwmrCell::new(42i32);

// 2. Create a local reader for this thread (each thread needs its own local reader)
let local = cell.local_reader();

// 3. Pin and read the value
// The guard provides a snapshot of the value at the moment of pinning
let guard = local.pin();
assert_eq!(*guard, 42);
drop(guard); // Reader is no longer accessing the data

// 4. Writer updates the value
// This creates a new version; old version is retired
cell.store(100i32);

// 5. Read the new value
let guard = local.pin();
assert_eq!(*guard, 100);
```

### Threaded Example

```rust
use swmr_cell::SwmrCell;
use std::thread;

fn main() {
    let mut cell = SwmrCell::new(0);
    
    let mut reader_handles = vec![];
    
    for i in 0..3 {
        // Create a LocalReader for each thread
        let local = cell.local_reader();
        
        let handle = thread::spawn(move || {
            loop {
                let guard = local.pin();
                let val = *guard;
                println!("Reader {} saw: {}", i, val);
                if val == 10 { break; }
                drop(guard); // unpin before sleeping or doing other work
                thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        reader_handles.push(handle);
    }
    
    // Writer updates values
    for i in 1..=10 {
        cell.store(i);
        thread::sleep(std::time::Duration::from_millis(20));
    }
    
    for h in reader_handles {
        h.join().unwrap();
    }
}
```

### Using `SwmrReaderFactory` for Shared Reader Creation

If you need to distribute the ability to create readers to multiple threads (e.g., in a thread pool where threads are dynamic), you can use `SwmrReaderFactory`. Unlike `LocalReader`, `SwmrReaderFactory` is `Sync` and `Clone`.

```rust
use swmr_cell::SwmrCell;
use std::thread;

let mut cell = SwmrCell::new(0);

// Create a SwmrReaderFactory that can be shared
let reader_factory = cell.reader_factory();

for i in 0..3 {
    // Clone the factory for each thread
    let factory = reader_factory.clone();
    
    thread::spawn(move || {
        // Create a LocalReader on the thread using the factory
        let local = factory.local_reader();
        
        // ... use local reader ...
    });
}
```

You can also obtain a `SwmrReaderFactory` from an existing `LocalReader` if you need to pass the capability to another thread:

```rust
// 1. Share: Create a SwmrReaderFactory from a LocalReader
let local_reader = cell.local_reader();
let factory = local_reader.reader_factory(); 
thread::spawn(move || {
    let local = factory.local_reader();
    // ...
});

// 2. Convert: Consume LocalReader to get SwmrReaderFactory
let local_reader = cell.local_reader();
let factory = local_reader.into_swmr(); // Consumes local_reader
thread::spawn(move || {
    let local = factory.local_reader();
    // ...
});
```

### Writer Utilities

`SwmrCell` provides several utilities for the writer:

```rust
let mut cell = SwmrCell::new(10);

// Update value using a closure
cell.update(|v| v + 5);
assert_eq!(*cell.get(), 15);

// Access the previous (retired) value
// Useful for rollback or comparison
assert_eq!(cell.previous(), Some(&10));
```

## Configuration

You can customize the garbage collection behavior using `SwmrCell::builder()`:

```rust
use swmr_cell::SwmrCell;

let mut cell = SwmrCell::builder()
    .auto_reclaim_threshold(Some(128)) // Trigger GC automatically after 128 stores (default: 16)
    // .auto_reclaim_threshold(None)   // Disable automatic GC
    .build(0);
```

- **auto_reclaim_threshold**: Controls how many retired objects are buffered before scanning readers for reclamation. Larger values reduce GC overhead but increase memory usage.

## How It Works

1.  **Versions**: The cell maintains a global monotonic version counter. Each `store` increments the version.
2.  **Readers**: Each `LocalReader` has a slot where it publishes the version it is currently accessing (`active_version`).
3.  **Reclamation**: When the writer updates the value, the old value is added to a garbage queue with the version it belonged to.
4.  **Collection**: The writer scans all active reader slots. It calculates the minimum version currently visible to any reader. Any garbage from versions strictly older than this minimum is safe to reclaim.

## Safety

- `LocalReader` is `!Sync` and must remain on the thread that uses it.
- `PinGuard` is bound to the lifetime of the `LocalReader` and prevents the underlying data from being freed while held.
- Accessing the data via `PinGuard` is safe because the writer guarantees that no version visible to an active reader will be deallocated.

## License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
