/// Basic tests module
/// Tests core functionality correctness
use crate::SwmrCell;
use std::thread;

/// Test 1: Create SwmrCell and basic usage
#[test]
fn test_create_swmr_cell_and_basic_usage() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();
    
    // Verify local can pin
    let guard = local.pin();
    assert_eq!(*guard, 42);
}

/// Test 2: local pin/drop cycle
#[test]
fn test_reader_pin_drop_cycle() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();

    // First pin
    {
        let _guard = local.pin();
        // guard is active here
    }
    // guard dropped, unpinned

    // Second pin
    {
        let _guard = local.pin();
        // guard active again
    }
}

/// Test 3: Writer store new value
#[test]
fn test_writer_store() {
    let mut cell = SwmrCell::new(10i32);
    let local = cell.local();

    // Initial value
    {
        let guard = local.pin();
        assert_eq!(*guard, 10);
    }

    // Writer stores new value
    cell.store(20);

    // Read new value
    {
        let guard = local.pin();
        assert_eq!(*guard, 20);
    }
}

/// Test 4: Writer manual collect
#[test]
fn test_writer_collect() {
    // Use builder to set a high threshold to avoid auto-collect
    let mut cell = SwmrCell::builder()
        .auto_reclaim_threshold(Some(1000))
        .build(0i32);

    // Retire some data
    cell.store(100);
    cell.store(200);

    // We can't check garbage count directly as it's private.
    // But we can call collect.
    cell.collect();
    
    // If it doesn't panic, it's good.
}

/// Test 5: Nested pins (Reentrancy)
#[test]
fn test_nested_pins() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();

    // Verify we can pin multiple times (reentrant pinning)
    let guard1 = local.pin();
    let guard2 = local.pin();
    let guard3 = local.pin(); // Reentrant

    assert_eq!(*guard1, 42);
    assert_eq!(*guard2, 42);
    assert_eq!(*guard3, 42);

    // All guards should work
    drop(guard3);
    drop(guard2);
    drop(guard1);
}

/// Test 6: Multiple Locals
#[test]
fn test_multiple_locals() {
    let cell = SwmrCell::new(42i32);

    let reader1 = cell.local();
    let reader2 = cell.local();

    // Both readers should work
    let guard1 = reader1.pin();
    let guard2 = reader2.pin();
    
    assert_eq!(*guard1, 42);
    assert_eq!(*guard2, 42);
}

/// Test 7: String type
#[test]
fn test_swmr_with_string() {
    let cell = SwmrCell::new(String::from("hello"));
    let local = cell.local();

    {
        let guard = local.pin();
        assert_eq!(*guard, "hello");
    }
}

/// Test 8: Struct type
#[test]
fn test_swmr_with_struct() {
    #[derive(Debug, PartialEq)]
    struct Point {
        x: i32,
        y: i32,
    }

    let cell = SwmrCell::new(Point { x: 10, y: 20 });
    let local = cell.local();

    {
        let guard = local.pin();
        assert_eq!(guard.x, 10);
        assert_eq!(guard.y, 20);
    }
}

/// Test 9: SwmrCell Drop
#[test]
fn test_swmr_drop() {
    let cell = SwmrCell::new(42i32);
    let local = cell.local();
    drop(local);
    drop(cell);
    // Memory should be freed. We rely on Miri or ASAN to catch leaks.
}

/// Test 10: Multiple SwmrCell instances
#[test]
fn test_multiple_swmr_instances() {
    let c1 = SwmrCell::new(10i32);
    let c2 = SwmrCell::new(20i32);
    let c3 = SwmrCell::new(30i32);
    
    let r1 = c1.local();
    let r2 = c2.local();
    let r3 = c3.local();

    {
        let g1 = r1.pin();
        let g2 = r2.pin();
        let g3 = r3.pin();

        assert_eq!(*g1, 10);
        assert_eq!(*g2, 20);
        assert_eq!(*g3, 30);
    }
}

/// Test 11: Thread safety
#[test]
fn test_thread_safety() {
    let cell = SwmrCell::new(0i32);

    let mut handles = vec![];

    // Start 5 local threads
    for _ in 0..5 {
        let local = cell.local();

        handles.push(thread::spawn(move || {
            let guard = local.pin();
            *guard // Return value
        }));
    }

    let results: Vec<i32> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    assert_eq!(results.len(), 5);
    for &result in &results {
        assert_eq!(result, 0);
    }
}

/// Test 12: previous() returns None initially
#[test]
fn test_previous_none_initially() {
    let cell = SwmrCell::new(42i32);
    assert!(cell.previous().is_none());
}

/// Test 13: previous() returns the old value after store
#[test]
fn test_previous_after_store() {
    let mut cell = SwmrCell::new(1i32);
    assert!(cell.previous().is_none());

    cell.store(2);
    assert_eq!(cell.previous(), Some(&1));

    cell.store(3);
    assert_eq!(cell.previous(), Some(&2));

    cell.store(4);
    assert_eq!(cell.previous(), Some(&3));
}

/// Test 14: previous() survives garbage collection
#[test]
fn test_previous_survives_gc() {
    let mut cell = SwmrCell::builder()
        .auto_reclaim_threshold(None) // Disable auto-reclaim
        .build(0i32);

    // Store multiple values to create garbage
    for i in 1..=10 {
        cell.store(i);
    }

    // Manual collect
    cell.collect();

    // previous() should still return the last retired value (9)
    // because safety_limit = current_version - 2 preserves it
    assert_eq!(cell.previous(), Some(&9));
}

/// Test 15: previous() with complex type
#[test]
fn test_previous_with_struct() {
    #[derive(Debug, PartialEq)]
    struct Data {
        value: i32,
        name: String,
    }

    let mut cell = SwmrCell::new(Data {
        value: 1,
        name: "first".to_string(),
    });
    assert!(cell.previous().is_none());

    cell.store(Data {
        value: 2,
        name: "second".to_string(),
    });

    let prev = cell.previous().unwrap();
    assert_eq!(prev.value, 1);
    assert_eq!(prev.name, "first");
}
