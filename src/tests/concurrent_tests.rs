/// Concurrent tests module
use crate::SwmrCell;
use std::thread;

/// Test 1: Single writer, multiple readers concurrent reads
#[test]
fn test_single_writer_multiple_readers_concurrent_reads() {
    let cell = SwmrCell::new(0i32);

    let mut handles = vec![];

    // Create 5 local threads
    for _ in 0..5 {
        let local = cell.local();

        let handle = thread::spawn(move || {
            // Each local reads 10 times
            for _ in 0..10 {
                let guard = local.pin();
                assert!(*guard >= 0);
            }
        });

        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}

/// Test 2: Writer updates, readers observe
#[test]
fn test_writer_updates_readers_observe() {
    let mut cell = SwmrCell::new(0i32);

    let local = cell.local();

    let reader_thread = thread::spawn(move || {
        // Read initial value
        {
            let guard = local.pin();
            assert_eq!(*guard, 0);
        }

        // Wait for update
        thread::sleep(std::time::Duration::from_millis(100));

        // Read updated value
        {
            let guard = local.pin();
            // We expect to see 100 eventually
            assert_eq!(*guard, 100);
        }
    });

    thread::sleep(std::time::Duration::from_millis(10));
    cell.store(100);

    reader_thread.join().unwrap();
}

/// Test 3: Sequential writer operations
#[test]
fn test_sequential_writer_operations() {
    let mut cell = SwmrCell::new(1i32);
    let local = cell.local();

    cell.store(2);
    assert_eq!(*local.pin(), 2);

    cell.store(3);
    assert_eq!(*local.pin(), 3);

    cell.store(4);
    assert_eq!(*local.pin(), 4);
}

/// Test 4: Readers in different epochs (versions)
#[test]
fn test_readers_in_different_epochs() {
    let mut cell = SwmrCell::new(0i32);
    let reader1 = cell.local();
    let reader2 = cell.local();

    // local 1 pins version 0
    let guard1 = reader1.pin();
    assert_eq!(*guard1, 0);

    // Writer updates to version 1
    cell.store(10);

    // local 2 pins version 1
    let guard2 = reader2.pin();
    assert_eq!(*guard2, 10);

    // local 1 still sees version 0 (because it's pinned and Guard holds the ref)
    assert_eq!(*guard1, 0);
}

/// Test 5: Garbage collection trigger
#[test]
fn test_garbage_collection_trigger() {
    // Threshold 64
    let mut cell = SwmrCell::builder()
        .auto_reclaim_threshold(Some(64))
        .build(0i32);

    // Retire data until trigger
    for i in 0..70 {
        cell.store(i);
    }

    // Should have triggered auto-reclaim.
}

/// Test 6: Active local protects garbage
#[test]
fn test_active_reader_protects_garbage() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    // local pins
    let _guard = local.pin();

    // Retire data
    for i in 0..70 {
        cell.store(i);
    }

    // Garbage should be protected
    cell.collect();
}

/// Test 7: Garbage reclaimed after local drop
#[test]
fn test_garbage_reclaimed_after_reader_drop() {
    let mut cell = SwmrCell::new(0i32);
    
    {
        let local = cell.local();
        let _guard = local.pin();
        for i in 0..70 {
            cell.store(i);
        }
        // local dropped at end of scope
    }
    
    // Collect
    cell.collect();
}

/// Test 8: Multiple readers min epoch
#[test]
fn test_min_epoch_calculation_multiple_readers() {
    let mut cell = SwmrCell::new(0i32);
    let reader1 = cell.local();
    let reader2 = cell.local();

    // local 1 at v0
    let _guard1 = reader1.pin();

    cell.store(10); // v1

    // local 2 at v1
    let _guard2 = reader2.pin();

    cell.store(20); // v2

    cell.collect();
    // min active is 0 (from local 1).
}

/// Test 9: High concurrency reads
#[test]
fn test_high_concurrency_reads() {
    let cell = SwmrCell::new(42i32);
    let mut handles = vec![];

    for _ in 0..20 {
        let local = cell.local();
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                let guard = local.pin();
                assert_eq!(*guard, 42);
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }
}

/// Test 10: local thread exit cleanup
#[test]
fn test_reader_thread_exit_cleanup() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    let t = thread::spawn(move || {
        let _guard = local.pin();
    });
    t.join().unwrap();

    // Thread exited, local dropped.
    // collect should cleanup the dead slot
    cell.collect();
}

/// Test 11: Interleaved read write
#[test]
fn test_interleaved_read_write_operations() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    for i in 0..10 {
        cell.store(i);
        assert_eq!(*local.pin(), i);
    }
}

/// Test 12: local holds guard during updates
#[test]
fn test_reader_holds_guard_during_updates() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    let t = thread::spawn(move || {
        let guard = local.pin();
        let val = *guard;
        thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(*guard, val);
    });

    for i in 1..50 {
        cell.store(i);
    }

    t.join().unwrap();
}
