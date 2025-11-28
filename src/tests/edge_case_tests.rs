/// Edge case and stress tests
use crate::SwmrCell;
use std::thread;

/// Test 1: Empty garbage collection
#[test]
fn test_empty_garbage_collection() {
    let mut cell = SwmrCell::new(0i32);
    cell.collect();
}

/// Test 2: Single data retire and reclaim
#[test]
fn test_single_data_retire_and_reclaim() {
    let mut cell = SwmrCell::new(42i32);
    cell.store(100);
    cell.collect();
}

/// Test 3: Exactly reach reclaim threshold
#[test]
fn test_exactly_reach_reclaim_threshold() {
    let mut cell = SwmrCell::builder()
        .auto_reclaim_threshold(Some(64))
        .build(0i32);

    for i in 0..64 {
        cell.store(i);
    }
    // One more to trigger?
    cell.store(100);
}

/// Test 4: Exceed reclaim threshold
#[test]
fn test_exceed_reclaim_threshold() {
    let mut cell = SwmrCell::builder()
        .auto_reclaim_threshold(Some(64))
        .build(0i32);

    for i in 0..100 {
        cell.store(i);
    }
}

/// Test 5: Zero sized type
#[test]
fn test_zero_sized_type() {
    #[derive(Debug, PartialEq)]
    struct ZeroSized;

    let mut cell = SwmrCell::new(ZeroSized);
    let local = cell.local();

    {
        let _guard = local.pin();
    }

    cell.store(ZeroSized);
}

/// Test 6: Large data structure
#[test]
fn test_large_data_structure() {
    #[derive(Debug, PartialEq)]
    struct LargeData {
        data: [u64; 1000],
    }

    let large = LargeData { data: [42; 1000] };
    let cell = SwmrCell::new(large);
    let local = cell.local();

    {
        let guard = local.pin();
        assert_eq!(guard.data[0], 42);
        assert_eq!(guard.data[999], 42);
    }
}

/// Test 7: Nested structures
#[test]
fn test_nested_structures() {
    #[derive(Debug, PartialEq)]
    struct Inner { value: i32 }
    #[derive(Debug, PartialEq)]
    struct Outer { inner: Inner, name: String }

    let outer = Outer {
        inner: Inner { value: 42 },
        name: String::from("test"),
    };

    let cell = SwmrCell::new(outer);
    let local = cell.local();

    {
        let guard = local.pin();
        assert_eq!(guard.inner.value, 42);
        assert_eq!(guard.name, "test");
    }
}

/// Test 8: Vector type
#[test]
fn test_vector_type() {
    let cell = SwmrCell::new(vec![1, 2, 3, 4, 5]);
    let local = cell.local();

    {
        let guard = local.pin();
        assert_eq!(guard.len(), 5);
        assert_eq!(guard[0], 1);
    }
}

/// Test 9: Multiple store operations
#[test]
fn test_multiple_store_operations() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    for i in 1..=10 {
        cell.store(i);
        assert_eq!(*local.pin(), i);
    }
}

/// Test 10: Rapid pin/unpin
#[test]
fn test_rapid_pin_unpin() {
    let cell = SwmrCell::new(0i32);
    let local = cell.local();

    for _ in 0..1000 {
        let _guard = local.pin();
    }
}

/// Test 11: Rapid local creation destruction
#[test]
fn test_rapid_reader_creation_destruction() {
    let cell = SwmrCell::new(0i32);

    for _ in 0..100 {
        let local = cell.local();
        let _guard = local.pin();
    }
}

/// Test 12: Readers in different threads
#[test]
fn test_readers_in_different_threads() {
    let mut cell = SwmrCell::new(0i32);
    let mut handles = vec![];

    for _ in 0..3 {
        let local = cell.local();
        handles.push(thread::spawn(move || {
            let guard = local.pin();
            assert!(*guard >= 0);
        }));
    }

    thread::sleep(std::time::Duration::from_millis(10));
    cell.store(1);

    for h in handles {
        h.join().unwrap();
    }
}

/// Test 13: Writer cleanup on drop
#[test]
fn test_writer_cleanup_on_drop() {
    {
        let mut cell = SwmrCell::new(0i32);
        for i in 0..50 {
            cell.store(i);
        }
    }
    // Should drop cleanly
}

/// Test 14: local handle cleanup on drop
#[test]
fn test_reader_handle_cleanup_on_drop() {
    let cell = SwmrCell::new(0i32);
    {
        let local = cell.local();
        let _guard = local.pin();
    }
    // guard dropped
    // local dropped at end of scope
}

/// Test 15: Alternating epoch advancement
#[test]
fn test_alternating_epoch_advancement() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    for cycle in 0..10 {
        for i in 0..100 {
            cell.store(cycle * 100 + i);
        }
        cell.collect();
        let _guard = local.pin();
    }
}

/// Test 16: Many readers epoch management
#[test]
fn test_many_readers_epoch_management() {
    let mut cell = SwmrCell::new(0i32);

    let reader1 = cell.local();
    let reader2 = cell.local();
    let reader3 = cell.local();

    cell.collect();

    let _g1 = reader1.pin();
    let _g2 = reader2.pin();
    let _g3 = reader3.pin();

    cell.collect();

    let _g4 = reader1.pin();
    let _g5 = reader2.pin();
    let _g6 = reader3.pin();
}

/// Test 17: Garbage protection across epochs
#[test]
fn test_garbage_protection_across_epochs() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    {
        let _guard = local.pin();
        for i in 0..50 {
            cell.store(i);
        }
        // Garbage protected
    }

    cell.collect();
    // Garbage collected
}

/// Test 18: Dynamic local registration
#[test]
fn test_dynamic_reader_registration() {
    let mut cell = SwmrCell::new(0i32);

    let reader1 = cell.local();
    let reader2 = cell.local();
    
    cell.collect();

    let _g1 = reader1.pin();
    let _g2 = reader2.pin();

    cell.collect();

    let reader3 = cell.local();
    let _g3 = reader3.pin();
}

/// Test 19: Stress high frequency operations
#[test]
fn test_stress_high_frequency_operations() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    for i in 0..1000 {
        cell.store(i % 100);
        {
            let guard = local.pin();
            assert!(*guard < 100);
        }
        if i % 100 == 0 {
            cell.collect();
        }
    }
}

// ============================================================================
// New API Edge Case Tests
// ============================================================================

/// Test 20: get() with zero-sized type
#[test]
fn test_get_with_zst() {
    #[derive(Debug, PartialEq)]
    struct ZeroSized;

    let cell = SwmrCell::new(ZeroSized);
    let _value = cell.get();
}

/// Test 21: update() with complex transformation
#[test]
fn test_update_with_complex_transformation() {
    let mut cell = SwmrCell::new(vec![1, 2, 3]);
    
    cell.update(|v| {
        let mut new_v = v.clone();
        new_v.push(4);
        new_v
    });
    
    assert_eq!(*cell.get(), vec![1, 2, 3, 4]);
}

/// Test 24: garbage_count after collect
#[test]
fn test_garbage_count_after_collect() {
    let mut cell = SwmrCell::builder()
        .auto_reclaim_threshold(None)
        .build(0i32);

    for i in 1..=10 {
        cell.store(i);
    }
    assert_eq!(cell.garbage_count(), 10);

    cell.collect();
    // After collect, some garbage should be reclaimed
    // (but previous is always kept)
    assert!(cell.garbage_count() < 10);
}

/// Test 25: is_pinned with nested pins
#[test]
fn test_is_pinned_with_nested_pins() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();

    assert!(!local.is_pinned());

    let guard1 = local.pin();
    assert!(local.is_pinned());

    let guard2 = local.pin();
    assert!(local.is_pinned());

    drop(guard1);
    assert!(local.is_pinned()); // Still pinned because guard2 exists

    drop(guard2);
    assert!(!local.is_pinned());
}

/// Test 26: PinGuard version consistency
#[test]
fn test_pin_guard_version_consistency() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    let guard1 = local.pin();
    let v1 = guard1.version();

    cell.store(1);

    // Nested pin still returns original pinned version
    let guard2 = local.pin();
    assert_eq!(guard2.version(), v1);

    drop(guard1);
    drop(guard2);

    // New pin gets new version
    let guard3 = local.pin();
    assert_eq!(guard3.version(), 1);
}

/// Test 27: update() triggers version increment
#[test]
fn test_update_triggers_version_increment() {
    let mut cell = SwmrCell::new(0i32);
    assert_eq!(cell.version(), 0);

    cell.update(|v| v + 1);
    assert_eq!(cell.version(), 1);

    cell.update(|v| v + 1);
    assert_eq!(cell.version(), 2);
}

/// Test 28: get() consistency with store
#[test]
fn test_get_consistency_with_store() {
    let mut cell = SwmrCell::new(0i32);
    
    for i in 0..100 {
        cell.store(i);
        assert_eq!(*cell.get(), i);
    }
}
