//! Loom-based concurrency tests
//!
//! Run with: `cargo test --features loom --test loom_tests`

#![cfg(feature = "loom")]

use loom::model::Builder;
use loom::thread;
use swmr_cell::SwmrCell;

/// Test: Multiple readers can safely read concurrently
#[test]
fn loom_concurrent_readers() {
    loom::model(|| {
        let cell = SwmrCell::new(42i32);

        let mut handles = vec![];

        // Spawn 2 local threads
        for _ in 0..2 {
            let local = cell.local();

            let handle = thread::spawn(move || {
                let guard = local.pin();
                assert_eq!(*guard, 42);
            });

            handles.push(handle);
        }

        for handle in handles {
            handle.join().unwrap();
        }
    });
}

/// Test: Single writer with concurrent readers (basic SWMR)
#[test]
fn loom_single_writer_multi_reader() {
    loom::model(|| {
        let mut cell = SwmrCell::new(1i32);

        // Spawn local thread
        let local = cell.local();
        let reader_handle = thread::spawn(move || {
            let guard = local.pin();
            let value = *guard;
            // Value should be either 1 or 2
            assert!(value == 1 || value == 2);
        });

        // Writer updates value
        cell.store(2i32);
        cell.collect();

        reader_handle.join().unwrap();
    });
}

/// Test: Nested loads (Reentrancy)
#[test]
fn loom_nested_loads() {
    loom::model(|| {
        let cell = SwmrCell::new(100i32);
        let local = cell.local();

        let handle = thread::spawn(move || {
            // Nested loading
            let guard1 = local.pin();
            assert_eq!(*guard1, 100);

            let guard2 = local.pin();
            assert_eq!(*guard2, 100);

            // Both guards should work
            assert_eq!(*guard1, 100);

            drop(guard2);

            // guard1 should still work
            assert_eq!(*guard1, 100);
        });

        handle.join().unwrap();
    });
}

/// Test: Garbage collection doesn't free data being read
#[test]
fn loom_gc_safety() {
    loom::model(|| {
        let mut cell = SwmrCell::new(1i32);

        let local = cell.local();

        let reader_handle = thread::spawn(move || {
            let guard = local.pin();
            let value = *guard;
            assert!(value >= 1 && value <= 3);

            // Simulate some work while holding the pin
            thread::yield_now();

            // Value should still be valid
            assert_eq!(*guard, value);
        });

        // Writer updates and collects garbage
        cell.store(2i32);
        cell.collect();

        cell.store(3i32);
        cell.collect();

        reader_handle.join().unwrap();
    });
}

/// Test: Multiple sequential stores and garbage collection
#[test]
fn loom_multiple_stores() {
    loom::model(|| {
        let mut cell = SwmrCell::new(1i32);
        let local = cell.local();

        // Multiple stores
        cell.store(2i32);
        cell.store(3i32);

        // Collection should safely reclaim old values
        cell.collect();

        let guard = local.pin();
        assert_eq!(*guard, 3);
    });
}

/// Test: Epoch advancement under concurrent access
#[test]
fn loom_epoch_advancement() {
    loom::model(|| {
        let mut cell = SwmrCell::new(0i32);

        let local = cell.local();

        let reader_handle = thread::spawn(move || {
            // Load and drop multiple times
            for _ in 0..2 {
                let guard = local.pin();
                assert!(*guard >= 0);
                drop(guard);
            }
        });

        // Writer performs multiple collections
        cell.collect();
        cell.collect();

        reader_handle.join().unwrap();
    });
}

/// Test: Store and load consistency
#[test]
fn loom_store_load_consistency() {
    loom::model(|| {
        let mut cell = SwmrCell::new(1i32);
        let local = cell.local();

        // Store a value
        cell.store(42i32);

        // Immediately load should see the new value
        let guard = local.pin();
        assert_eq!(*guard, 42);
    });
}

/// Test: Sequential writers (simulating ownership transfer of writer is not really possible in loom easily without move, but we can just do seq ops)
#[test]
fn loom_sequential_writer_ops() {
    loom::model(|| {
        let mut cell = SwmrCell::new(1i32);

        let local = cell.local();

        let reader_thread = thread::spawn(move || {
            let guard = local.pin();
            let val = *guard;
            assert!(val >= 1 && val <= 3);
        });

        // Sequential stores
        cell.store(2i32);
        cell.collect();
        cell.store(3i32);
        cell.collect();

        reader_thread.join().unwrap();
    });
}

/// Test: Multiple SwmrCell instances
#[test]
fn loom_multiple_swmr_cells() {
    loom::model(|| {
        let mut w1 = SwmrCell::new(10i32);
        let mut w2 = SwmrCell::new(20i32);

        let r1 = w1.local();
        let r2 = w2.local();

        let local = thread::spawn(move || {
            let g1 = r1.pin();
            let g2 = r2.pin();

            let v1 = *g1;
            let v2 = *g2;

            // Values should be from their respective stores
            assert!(v1 == 10 || v1 == 11);
            assert!(v2 == 20 || v2 == 21);
        });

        // Update both pointers
        w1.store(11i32);
        w2.store(21i32);
        w1.collect(); // w1 and w2 are independent, but we can collect both
        w2.collect();

        local.join().unwrap();
    });
}

/// Test: Fast load/drop cycles during garbage collection
#[test]
fn loom_fast_load_drop_cycles() {
    loom::model(|| {
        let mut cell = SwmrCell::new(0i32);

        let local = cell.local();

        let reader_thread = thread::spawn(move || {
            // Rapid load/drop cycles
            for _ in 0..2 {
                let guard = local.pin();
                let _val = *guard;
                drop(guard);
                thread::yield_now();
            }
        });

        // Writer stores and collects during local's cycles
        cell.store(1i32);
        cell.collect();

        reader_thread.join().unwrap();
    });
}

/// Test: No active readers - GC should reclaim all garbage
#[test]
fn loom_gc_with_no_active_readers() {
    loom::model(|| {
        let mut cell = SwmrCell::new(1i32);
        let local = cell.local();

        // Store multiple values without any active readers (local exists but no guards)
        cell.store(2i32);
        cell.store(3i32);
        cell.store(4i32);

        // Collect - should reclaim all since no guards are active
        cell.collect();

        // Now load and verify latest value
        let guard = local.pin();
        assert_eq!(*guard, 4);
    });
}

/// Test: local drop behavior
#[test]
fn loom_reader_drop() {
    loom::model(|| {
        let mut cell = SwmrCell::new(1i32);

        let local = cell.local();

        let reader_thread = thread::spawn(move || {
            {
                let guard = local.pin();
                let val = *guard;
                assert!(val == 1 || val == 2);
                // guard dropped
            }
            // local dropped
        });

        thread::yield_now();

        // Store after local might have dropped
        cell.store(2i32);
        cell.collect();

        reader_thread.join().unwrap();
    });
}

/// Test: local pinned across multiple epoch advancements
#[test]
fn loom_reader_across_epochs() {
    let mut builder = Builder::new();
    builder.preemption_bound = Some(3);
    builder.check(|| {
        let mut cell = SwmrCell::new(1i32);

        let local = cell.local();

        let reader_thread = thread::spawn(move || {
            let guard = local.pin();
            let initial = *guard;

            thread::yield_now();
            // Hold pin across multiple yields
            let val1 = *guard;
            thread::yield_now();
            let val2 = *guard;
            thread::yield_now();

            // Should see a consistent view
            assert_eq!(val1, initial); // Because we pinned at start, we see initial value or whatever was current when we pinned.
            // Actually, if we pinned 1, we see 1. Even if writer updates to 2 and 3.
            assert_eq!(val2, initial);
        });

        // Advance epoch multiple times
        cell.collect();
        cell.store(2i32);
        cell.collect();
        cell.store(3i32);
        cell.collect();

        reader_thread.join().unwrap();
    });
}

/// Test: Three concurrent readers with writer
#[test]
fn loom_three_readers_one_writer() {
    // Use preemption bound to limit state space exploration
    let mut builder = Builder::new();
    builder.preemption_bound = Some(2);
    builder.check(|| {
        let mut cell = SwmrCell::new(0i32);
        let mut readers = vec![];

        // Spawn 3 local threads
        for _ in 0..3 {
            let r = cell.local();
            let handle = thread::spawn(move || {
                let guard = r.pin();
                assert!(*guard <= 5);
            });
            readers.push(handle);
        }

        // Writer updates
        cell.store(5i32);
        cell.collect();

        for handle in readers {
            handle.join().unwrap();
        }
    });
}

/// Test: Interleaved load/drop from multiple readers
#[test]
fn loom_interleaved_load_drop() {
    // Multiple cycles create large state space
    let mut builder = Builder::new();
    builder.preemption_bound = Some(4);
    builder.check(|| {
        let mut cell = SwmrCell::new(100i32);
        let mut handles = vec![];

        for _ in 0..2 {
            let r = cell.local();
            let handle = thread::spawn(move || {
                // First load
                {
                    let guard = r.pin();
                    let _val = *guard;
                }

                thread::yield_now();

                // Second load after drop
                {
                    let guard = r.pin();
                    let _val = *guard;
                }
            });
            handles.push(handle);
        }

        // Collect during interleaved access
        cell.collect();

        for h in handles {
            h.join().unwrap();
        }
    });
}

/// Test: Store with immediate collection and concurrent read
#[test]
fn loom_store_collect_read_race() {
    loom::model(|| {
        let mut cell = SwmrCell::new(1i32);

        let local = cell.local();
        let reader_thread = thread::spawn(move || {
            let guard = local.pin();
            let val = *guard;
            assert!(val == 1 || val == 2);
        });

        // Store and immediately collect - races with local
        cell.store(2i32);
        cell.collect();

        reader_thread.join().unwrap();
    });
}

/// Test: Builder configuration with custom threshold
#[test]
fn loom_builder_custom_threshold() {
    loom::model(|| {
        let mut cell = SwmrCell::builder().auto_reclaim_threshold(Some(2)).build(1i32);
        let local = cell.local();

        // Store should not trigger auto-collection at threshold 2 (if start is 0 garbage)
        // Initial garbage count = 0.
        // store(2) -> replaces 1. Garbage count = 1.
        // 1 < 2, no collect.
        cell.store(2i32);

        // Verify value is updated
        let guard = local.pin();
        assert_eq!(*guard, 2);
    });
}

/// Test: Builder with disabled auto-reclamation
#[test]
fn loom_builder_no_auto_reclaim() {
    loom::model(|| {
        let mut cell = SwmrCell::builder()
            .auto_reclaim_threshold(None)
            .build(1i32);
        let local = cell.local();

        // Multiple stores without auto-collection
        cell.store(2i32);
        cell.store(3i32);

        // Manual collection
        cell.collect();

        let guard = local.pin();
        assert_eq!(*guard, 3);
    });
}

/// Test: Concurrent readers with different pin lifetimes
#[test]
fn loom_different_pin_lifetimes() {
    // Limit preemption bound for faster completion
    let mut builder = Builder::new();
    builder.preemption_bound = Some(4);
    builder.check(|| {
        let mut cell = SwmrCell::new(1i32);

        // local 1: short-lived pin
        let r1 = cell.local();
        let t1 = thread::spawn(move || {
            {
                let _guard = r1.pin();
            } // guard dropped early
        });

        // local 2: long-lived pin
        let r2 = cell.local();
        let t2 = thread::spawn(move || {
            let guard = r2.pin();
            let val = *guard;
            thread::yield_now();
            // Should see consistent value
            assert_eq!(val, *guard);
        });

        // Writer stores during mixed local lifetimes
        cell.store(2i32);
        cell.collect();

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

/// Test: Multiple collections without stores
#[test]
fn loom_multiple_collections_no_stores() {
    let mut builder = Builder::new();
    builder.preemption_bound = Some(3);
    builder.check(|| {
        let mut cell = SwmrCell::new(42i32);

        let r = cell.local();
        let t = thread::spawn(move || {
            let guard = r.pin();
            assert_eq!(*guard, 42);
        });

        // Multiple collections without any stores
        cell.collect();
        cell.collect();
        cell.collect();

        t.join().unwrap();
    });
}

/// Test: Alternating store and collect operations
#[test]
fn loom_alternating_store_collect() {
    loom::model(|| {
        let mut cell = SwmrCell::new(0i32);

        let r = cell.local();
        let t = thread::spawn(move || {
            let guard = r.pin();
            let val = *guard;
            assert!(val <= 2);
        });

        // Alternating pattern
        cell.store(1i32);
        cell.collect();
        cell.store(2i32);
        cell.collect();

        t.join().unwrap();
    });
}

/// Test: Multiple guards from same local (simulated by cloning local or just calling load twice)
/// Note: Same local instance can create multiple guards.
#[test]
fn loom_multiple_guards_same_reader() {
    loom::model(|| {
        let cell = SwmrCell::new(77i32);
        let local = cell.local();

        let handle = thread::spawn(move || {
            // Create multiple guards simultaneously
            let guard1 = local.pin();
            let guard2 = local.pin();
            
            assert_eq!(*guard1, 77);
            assert_eq!(*guard2, 77);

            // Drop in different order
            drop(guard2);
            assert_eq!(*guard1, 77);
        });

        handle.join().unwrap();
    });
}

/// Test: local observes values from valid range across pin/unpin cycles
#[test]
fn loom_reader_monotonic_observation() {
    let mut builder = Builder::new();
    builder.preemption_bound = Some(3);
    builder.check(|| {
        let mut cell = SwmrCell::new(1i32);

        let r = cell.local();
        let t = thread::spawn(move || {
            let guard1 = r.pin();
            let val1 = *guard1;
            drop(guard1);

            thread::yield_now();

            let guard2 = r.pin();
            let val2 = *guard2;
            drop(guard2);

            assert!(val1 >= 1 && val1 <= 3);
            assert!(val2 >= 1 && val2 <= 3);
        });

        // Monotonically increasing stores
        cell.store(2i32);
        cell.collect();
        cell.store(3i32);
        cell.collect();

        t.join().unwrap();
    });
}

/// Test: local holds guard while writer performs multiple updates
#[test]
fn loom_reader_holds_guard_during_updates() {
    loom::model(|| {
        let mut cell = SwmrCell::new(0i32);

        let r = cell.local();
        let t = thread::spawn(move || {
            let guard = r.pin();
            let initial_value = *guard;

            thread::yield_now();
            thread::yield_now();

            // The same reference should still be valid and consistent
            assert_eq!(*guard, initial_value);
            assert!(initial_value >= 0 && initial_value <= 3);
        });

        // Writer performs multiple updates
        cell.store(1i32);
        cell.store(2i32);
        cell.store(3i32);

        t.join().unwrap();
    });
}
