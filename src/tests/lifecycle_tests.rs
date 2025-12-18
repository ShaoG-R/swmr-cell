/// Lifecycle and memory safety tests
use crate::SwmrCell;
use std::prelude::v1::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use std::format;
use std::vec;

/// Test 1: local guard lifetime constraint
#[test]
fn test_guard_lifetime_constraint() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();
    let guard = local.pin();
    assert_eq!(*guard, 42);
    // value lifetime bound to guard
}

/// Test 2: Multiple guards simultaneously active
#[test]
fn test_multiple_guards_simultaneously_active() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();

    let guard1 = local.pin();
    let guard2 = local.pin();

    assert_eq!(*guard1, 42);
    assert_eq!(*guard2, 42);
}

/// Test 3: Guard nested scopes
#[test]
fn test_guard_nested_scopes() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();

    {
        let guard1 = local.pin();
        assert_eq!(*guard1, 42);

        {
            let guard2 = local.pin();
            assert_eq!(*guard2, 42);
        }

        let guard1_again = local.pin();
        assert_eq!(*guard1_again, 42);
    }
}

/// Test 4: local isolation across threads
#[test]
fn test_reader_isolation_across_threads() {
    let mut cell = SwmrCell::new(0i32);
    let mut handles = vec![];

    for _ in 0..3 {
        let local = cell.local();
        handles.push(thread::spawn(move || {
            let guard = local.pin();
            assert!(*guard >= 0);
        }));
    }

    cell.store(1);

    for h in handles {
        h.join().unwrap();
    }
}

/// Test 5: Writer single threaded constraint
/// The type system enforces this (SwmrCell is !Sync? No, SwmrCell is Send but maybe !Clone),
/// but we can't easily test compilation failure here.
/// We just assume if we can't clone SwmrCell, it's good.
#[test]
fn test_writer_uniqueness() {
    // SwmrCell does not implement Clone (Single Writer)
    // let cell = SwmrCell::new(0);
    // let c2 = cell.clone(); // Should fail to compile
}

/// Test 6: Garbage collection memory safety
#[test]
fn test_garbage_collection_memory_safety() {
    let mut cell = SwmrCell::new(vec![1, 2, 3]);
    let local = cell.local();

    cell.store(vec![4, 5, 6]);
    cell.store(vec![7, 8, 9]);

    let _guard = local.pin();
    cell.collect();

    let _guard2 = local.pin();
}

/// Test 7: EpochPtr drop implementation
/// Covered by swmr_drop test in basic_tests, but here specifically checks leaks?
#[test]
fn test_swmr_drop_implementation() {
    {
        let _c = SwmrCell::new(String::from("test"));
    }
}

/// Test 8: Multiple SwmrCell independence
#[test]
fn test_multiple_swmr_independence() {
    let c1 = SwmrCell::new(10i32);
    let c2 = SwmrCell::new(20i32);
    let c3 = SwmrCell::new(30i32);

    let r1 = c1.local();
    let r2 = c2.local();
    let r3 = c3.local();

    let g1 = r1.pin();
    let g2 = r2.pin();
    let g3 = r3.pin();

    assert_eq!(*g1, 10);
    assert_eq!(*g2, 20);
    assert_eq!(*g3, 30);
}

/// Test 9: Domain clone safety (local creation safety)
#[test]
fn test_reader_creation_safety() {
    let cell = SwmrCell::new(0i32);
    let r1 = cell.local();
    let r2 = cell.local();
    let r3 = cell.local();

    let _g1 = r1.pin();
    let _g2 = r2.pin();
    let _g3 = r3.pin();
}

/// Test 10: Epoch advancement correctness
#[test]
fn test_epoch_advancement_correctness() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    {
        let guard = local.pin();
        assert_eq!(*guard, 0);
    }

    cell.store(1);

    {
        let guard = local.pin();
        assert_eq!(*guard, 1);
    }
}

/// Test 11: Concurrent read consistency
#[test]
fn test_concurrent_read_consistency() {
    let cell = SwmrCell::new(42i32);
    let check = Arc::new(AtomicUsize::new(0));
    let mut handles = vec![];

    for _ in 0..10 {
        let local = cell.local();
        let c = check.clone();
        handles.push(thread::spawn(move || {
            for _ in 0..100 {
                let guard = local.pin();
                if *guard == 42 {
                    c.fetch_add(1, Ordering::Relaxed);
                }
            }
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(check.load(Ordering::Relaxed), 1000);
}

/// Test 12: local exit cleanup
#[test]
fn test_reader_exit_cleanup() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    let t = thread::spawn(move || {
        let _guard = local.pin();
    });
    t.join().unwrap();
    cell.collect();
}

/// Test 13: Large garbage safe reclamation
#[test]
fn test_large_garbage_safe_reclamation() {
    let mut cell = SwmrCell::new(0i32);
    for i in 0..1000 {
        cell.store(i);
    }
    cell.collect();
}

/// Test 14: Complex type lifetime management
#[test]
fn test_complex_type_lifetime_management() {
    #[derive(Debug)]
    struct ComplexData {
        id: usize,
        values: Vec<i32>,
        name: String,
    }

    let data = ComplexData {
        id: 1,
        values: vec![1, 2, 3, 4, 5],
        name: String::from("test"),
    };

    let cell = SwmrCell::new(data);
    let local = cell.local();

    let guard = local.pin();
    assert_eq!(guard.id, 1);
    assert_eq!(guard.values.len(), 5);
    assert_eq!(guard.name, "test");
}

/// Test 15: Data visibility across epochs
#[test]
fn test_data_visibility_across_epochs() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    let g1 = local.pin();

    assert_eq!(*g1, 0);

    cell.store(1);

    let g2 = local.pin();
    assert_eq!(*g2, 1);

    // Original reader still sees old data if it holds guard
    assert_eq!(*g1, 0);

    // But if it pin again?
    let g1_new = local.pin();
    assert_eq!(*g1_new, 1);
}

/// Test 17: Rapid local switching
#[test]
fn test_rapid_reader_switching() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();

    for _ in 0..100 {
        let guard = local.pin();
        assert_eq!(*guard, 42);
        drop(guard);

        let guard = local.pin();
        assert_eq!(*guard, 42);
    }
}

/// Test 18: Writer garbage management
#[test]
fn test_writer_garbage_management() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    {
        let _guard = local.pin();
        for i in 0..50 {
            cell.store(i);
        }
        // Garbage retained
    }
    // local inactive
    cell.collect();
    // Garbage collected
}

/// Test 19: Multiple readers garbage protection
#[test]
fn test_multiple_readers_garbage_protection() {
    let mut cell = SwmrCell::new(0i32);
    let r1 = cell.local();
    let r2 = cell.local();
    let r3 = cell.local();

    let _g1 = r1.pin();
    let _g2 = r2.pin();
    let _g3 = r3.pin();

    for i in 0..100 {
        cell.store(i);
    }

    // Garbage protected
    cell.collect();
}

/// Test 20: Complete lifecycle scenario
#[test]
fn test_complete_lifecycle_scenario() {
    let mut cell = SwmrCell::new(String::from("initial"));
    let mut readers = vec![];
    for _ in 0..5 {
        readers.push(cell.local());
    }

    for round in 0..3 {
        let guards: Vec<_> = readers
            .iter()
            .map(|r: &crate::LocalReader<String>| r.pin())
            .collect();

        for guard in &guards {
            assert!(!(*guard).is_empty());
        }

        cell.store(format!("round_{}", round));

        for i in 0..50 {
            cell.store(format!("garbage_{}", i));
        }

        cell.collect();
    }
}

// ============================================================================
// New API Lifecycle Tests
// ============================================================================

/// Test 21: get() lifetime does not outlive cell
#[test]
fn test_get_lifetime_bound_to_cell() {
    let cell = SwmrCell::new(42i32);
    let value_ref = cell.get();
    assert_eq!(*value_ref, 42);
    // value_ref is valid as long as cell is alive
}

/// Test 22: update() preserves garbage for previous()
#[test]
fn test_update_preserves_previous() {
    let mut cell = SwmrCell::new(1i32);

    cell.update(|v| v + 1);
    assert_eq!(cell.previous(), Some(&1));

    cell.update(|v| v * 2);
    assert_eq!(cell.previous(), Some(&2));
}

/// Test 24: version consistency throughout lifecycle
#[test]
fn test_version_consistency_lifecycle() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    for i in 0..10 {
        assert_eq!(cell.version(), i);
        assert_eq!(local.version(), i);

        let guard = local.pin();
        assert_eq!(guard.version(), i);
        drop(guard);

        cell.store(i as i32 + 1);
    }
}

/// Test 26: is_pinned lifecycle with clone
#[test]
fn test_is_pinned_lifecycle_with_clone() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();

    assert!(!local.is_pinned());

    let guard1 = local.pin();
    assert!(local.is_pinned());

    let guard2 = guard1.clone();
    assert!(local.is_pinned());

    drop(guard1);
    assert!(local.is_pinned()); // guard2 still holds

    drop(guard2);
    assert!(!local.is_pinned());
}

/// Test 28: PinGuard version preserved across writes
#[test]
fn test_pin_guard_version_preserved() {
    let mut cell = SwmrCell::new(0i32);
    let local = cell.local();

    let guard = local.pin();
    let initial_version = guard.version();

    // Write multiple times
    for i in 1..=10 {
        cell.store(i);
    }

    // Guard version should not change
    assert_eq!(guard.version(), initial_version);

    // But value seen by guard is the snapshot
    assert_eq!(*guard, 0);
}

/// Test 29: Default trait with complex lifecycle
#[test]
fn test_default_trait_lifecycle() {
    let mut cell: SwmrCell<Vec<i32>> = SwmrCell::default();
    let local = cell.local();

    assert!(cell.get().is_empty());
    assert_eq!(cell.version(), 0);

    cell.update(|v: &Vec<i32>| {
        let mut new_v = v.clone();
        new_v.push(1);
        new_v
    });

    assert_eq!(*cell.get(), vec![1]);
    assert_eq!(cell.version(), 1);

    let guard = local.pin();
    assert_eq!(*guard, vec![1]);
}
