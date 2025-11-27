//! # SWMR Version-Based Single Object
//!
//! This crate provides a single-writer, multi-reader (SWMR) cell that supports
//! concurrent wait-free reads and lock-free writes using version-based garbage collection.
//!
//! ## Core Concepts
//!
//! - **Single Object**: The `swmr_cell` library manages a single versioned object per `SwmrCell`.
//! - **Version**: The version counter represents the state of the object. Each write increments the version.
//! - **Pinning**: Readers pin the current version when they start reading, preventing the writer from reclaiming that version (and any older versions still visible to other readers) until they are done.
//!
//! ## Typical Usage
//!
//! ```rust
//! use swmr_cell::SwmrCell;
//!
//! // 1. Create a new SWMR cell with an initial value
//! let mut cell = SwmrCell::new(42i32);
//!
//! // 2. Create a local reader for this thread (or pass to another thread)
//! let local = cell.local();
//!
//! // 3. Pin and read the value by dereferencing the guard
//! let guard = local.pin();
//! assert_eq!(*guard, 42);
//! drop(guard);
//!
//! // 4. Writer updates the value
//! cell.store(100i32);
//!
//! // 5. Read the new value
//! let guard = local.pin();
//! assert_eq!(*guard, 100);
//! drop(guard);
//!
//! // 6. Manually collect garbage (optional, happens automatically too)
//! cell.collect();
//! ```
mod sync;

#[cfg(test)]
mod tests;

use crate::sync::*;
use std::{collections::VecDeque, marker::PhantomData, ops::Deref, vec::Vec};

/// Default threshold for automatic garbage reclamation (count of retired nodes).
/// 自动垃圾回收的默认阈值（已退休节点的数量）。
pub(crate) const AUTO_RECLAIM_THRESHOLD: usize = 64;

/// Default interval for cleaning up dead reader slots (in collection cycles).
/// 清理死读者槽的默认间隔（以回收周期为单位）。
pub(crate) const DEFAULT_CLEANUP_INTERVAL: usize = 16;

/// Represents a reader that is not currently pinned to any version.
/// 表示当前未被钉住到任何版本的读者。
pub(crate) const INACTIVE_VERSION: usize = usize::MAX;


/// A single-writer, multi-reader cell with version-based garbage collection.
///
/// `SwmrCell` provides safe concurrent access where one writer can update the value
/// and multiple readers can read it concurrently. Readers access the value by
/// creating a `LocalReader` and pinning it.
///
/// 单写多读单元，带有基于版本的垃圾回收。
///
/// `SwmrCell` 提供安全的并发访问，其中一个写入者可以更新值，
/// 多个读者可以并发读取它。读者通过创建 `LocalReader` 并 pin 来访问值。
pub struct SwmrCell<T: 'static> {
    shared: Arc<SharedState>,
    ptr: Arc<AtomicPtr<T>>,
    garbage: GarbageSet<T>,
    auto_reclaim_threshold: Option<usize>,
    collection_counter: usize,
    cleanup_interval: usize,
}

impl<T: 'static> SwmrCell<T> {
    /// Create a new SWMR cell with default settings and the given initial value.
    ///
    /// 使用默认设置和给定的初始值创建一个新的 SWMR 单元。
    #[inline]
    pub fn new(data: T) -> Self
    where
        T: Send,
    {
        Self::builder().build(data)
    }

    /// Returns a builder for configuring the SWMR cell.
    ///
    /// 返回用于配置 SWMR 单元的构建器。
    #[inline]
    pub fn builder() -> SwmrCellBuilder<T> {
        SwmrCellBuilder {
            auto_reclaim_threshold: Some(AUTO_RECLAIM_THRESHOLD),
            cleanup_interval: DEFAULT_CLEANUP_INTERVAL,
            marker: PhantomData::default()
        }
    }

    /// Create a new `LocalReader` for reading.
    ///
    /// Each thread should create its own `LocalReader` and reuse it.
    /// `LocalReader` is `!Sync` and should not be shared between threads.
    ///
    /// 创建一个新的 `LocalReader` 用于读取。
    /// 每个线程应该创建自己的 `LocalReader` 并重复使用。
    /// `LocalReader` 是 `!Sync` 的，不应在线程之间共享。
    #[inline]
    pub fn local(&self) -> LocalReader<T> {
        LocalReader::new(self.shared.clone(), self.ptr.clone())
    }

    /// Store a new value, making it visible to readers.
    /// The old value is retired and will be garbage collected.
    ///
    /// This operation increments the global version.
    ///
    /// 存储新值，使其对读者可见。
    /// 旧值已退休，将被垃圾回收。
    /// 此操作会增加全局版本。
    pub fn store(&mut self, data: T) {
        let new_ptr = Box::into_raw(Box::new(data));
        let old_ptr = self.ptr.swap(new_ptr, Ordering::Release);

        // Increment global version.
        // The old value belongs to the previous version (the one before this increment).
        // 增加全局版本。
        // 旧值属于前一个版本（此次增加之前的那个）。
        let old_version = self.shared.global_version.fetch_add(1, Ordering::AcqRel);

        if !old_ptr.is_null() {
            // Safe because we just swapped it out and we own the writer
            unsafe {
                self.garbage.add(Box::from_raw(old_ptr), old_version);
            }
        }

        // Auto-reclaim
        if let Some(threshold) = self.auto_reclaim_threshold {
            if self.garbage.len() > threshold {
                self.collect();
            }
        }
    }

    /// Manually trigger garbage collection.
    /// 手动触发垃圾回收。
    pub fn collect(&mut self) {
        // In this design, we don't necessarily advance the version just for collection.
        // But we need to find min_active_version.
        
        // Ensure that any global version updates (from store) are visible before we scan readers.
        // We need a StoreLoad barrier to ensure the Writer sees the Reader's active_version store
        // if the Reader didn't see the Writer's global_version store.
        swmr_barrier::heavy_barrier();

        let current_version = self.shared.global_version.load(Ordering::Acquire);
        let mut min_active = current_version;
        
        self.collection_counter += 1;
        let should_cleanup = self.cleanup_interval > 0 && self.collection_counter % self.cleanup_interval == 0;

        let mut shared_readers = self.shared.readers.lock();
        let mut dead_count = 0;

        for arc_slot in shared_readers.iter() {
            let version = arc_slot.active_version.load(Ordering::Acquire);
            if version != INACTIVE_VERSION {
                min_active = min_active.min(version);
            } else if should_cleanup && Arc::strong_count(arc_slot) == 1 {
                dead_count += 1;
            }
        }

        if should_cleanup && dead_count > 0 {
            shared_readers.retain(|arc_slot| Arc::strong_count(arc_slot) > 1);
        }
        
        drop(shared_readers);

        self.shared.min_active_version.store(min_active, Ordering::Release);
        self.garbage.collect(min_active, current_version);
    }
}


/// A builder for configuring and creating a SWMR cell.
///
/// 用于配置和创建 SWMR 单元的构建器。
pub struct SwmrCellBuilder<T> {
    auto_reclaim_threshold: Option<usize>,
    cleanup_interval: usize,
    marker: PhantomData<T>
}

impl<T: Send + 'static> SwmrCellBuilder<T> {
    /// Sets the threshold for automatic garbage reclamation.
    ///
    /// When the number of retired objects exceeds this threshold,
    /// garbage collection is triggered automatically during `store`.
    ///
    /// Set to `None` to disable automatic reclamation.
    /// Default is `Some(64)`.
    ///
    /// 设置自动垃圾回收的阈值。
    /// 当已退休对象的数量超过此阈值时，将在 `store` 期间自动触发垃圾回收。
    /// 设置为 `None` 以禁用自动回收。
    /// 默认为 `Some(64)`。
    #[inline]
    pub fn auto_reclaim_threshold(mut self, threshold: Option<usize>) -> Self {
        self.auto_reclaim_threshold = threshold;
        self
    }

    /// Sets the interval for cleaning up dead reader slots.
    ///
    /// The cleanup happens every `interval` collection cycles.
    /// Default is `16`.
    ///
    /// 设置清理死读者槽的间隔。
    /// 每隔 `interval` 个回收周期进行一次清理。
    /// 默认为 `16`。
    #[inline]
    pub fn cleanup_interval(mut self, interval: usize) -> Self {
        self.cleanup_interval = interval;
        self
    }

    /// Creates a new SWMR cell with the configured settings and initial value.
    ///
    /// 使用配置的设置和初始值创建一个新的 SWMR 单元。
    pub fn build(self, data: T) -> SwmrCell<T> {
        let shared = Arc::new(SharedState {
            global_version: AtomicUsize::new(0),
            min_active_version: AtomicUsize::new(0),
            readers: Mutex::new(Vec::new()),
        });

        let ptr = Arc::new(AtomicPtr::new(Box::into_raw(Box::new(data))));

        SwmrCell {
            shared,
            ptr,
            garbage: GarbageSet::new(),
            auto_reclaim_threshold: self.auto_reclaim_threshold,
            collection_counter: 0,
            cleanup_interval: self.cleanup_interval,
        }
    }
}



/// Manages retired objects and their reclamation.
///
/// This struct encapsulates the logic for:
/// - Storing retired objects in version-ordered queue.
/// - Reclaiming objects when they are safe to delete.
///
/// 管理已退休对象及其回收。
///
/// 此结构体封装了以下逻辑：
/// - 将已退休对象存储在按版本排序的队列中。
/// - 当对象可以安全删除时进行回收。
struct GarbageSet<T> {
    /// Queue of garbage items, ordered by version.
    /// Each element is (version, node).
    queue: VecDeque<(usize, Box<T>)>,
}

impl<T> GarbageSet<T> {
    /// Create a new empty garbage set.
    /// 创建一个新的空垃圾集合。
    fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }

    /// Get the total number of retired objects.
    /// 获取已退休对象的总数。
    #[inline]
    fn len(&self) -> usize {
        self.queue.len()
    }

    /// Add a retired node to the set for the current version.
    ///
    /// 将已退休节点添加到当前版本的集合中。
    #[inline]
    fn add(&mut self, node: Box<T>, current_version: usize) {
        self.queue.push_back((current_version, node));
    }

    /// Reclaim garbage that is safe to delete.
    ///
    /// Garbage from versions older than `min_active_version` is dropped.
    ///
    /// 回收可以安全删除的垃圾。
    ///
    /// 来自比 `min_active_version` 更旧的版本的垃圾将被 drop。
    #[inline]
    fn collect(&mut self, min_active_version: usize, _current_version: usize) {
        // We reclaim everything that is strictly older than min_active_version.
        // If min_active_version == current_version, then everything (all < current_version) is reclaimed.
        while let Some((version, _)) = self.queue.front() {
            if *version >= min_active_version {
                break;
            }
            self.queue.pop_front(); // Box<T> is dropped here
        }
    }
}



/// A slot allocated for a reader thread to record its active version.
///
/// Cache-aligned to prevent false sharing between readers.
///
/// 为读者线程分配的槽，用于记录其活跃版本。
/// 缓存对齐以防止读者之间的伪共享。
#[derive(Debug)]
#[repr(align(64))]
pub(crate) struct ReaderSlot {
    /// The version currently being accessed by the reader, or INACTIVE_VERSION.
    /// 读者当前访问的版本，或 INACTIVE_VERSION。
    pub(crate) active_version: AtomicUsize,
}

/// Global shared state for the version GC domain.
///
/// Contains the global version, the minimum active version, and the list of reader slots.
///
/// version GC 域的全局共享状态。
/// 包含全局版本、最小活跃版本和读者槽列表。
#[derive(Debug)]
#[repr(align(64))]
pub(crate) struct SharedState {
    /// The global monotonic version counter.
    /// 全局单调版本计数器。
    pub(crate) global_version: AtomicUsize,
    /// The minimum version among all active readers (cached for performance).
    /// 所有活跃读者中的最小版本（为性能而缓存）。
    pub(crate) min_active_version: AtomicUsize,
    /// List of all registered reader slots. Protected by a Mutex.
    /// 所有注册读者槽的列表。由 Mutex 保护。
    pub(crate) readers: Mutex<Vec<Arc<ReaderSlot>>>,
}

/// A reader thread's local version state.
///
/// Each reader thread should create exactly one `LocalReader` via `SwmrCell::local()`.
/// It is `!Sync` (due to `Cell`) and must be stored per-thread.
///
/// The `LocalReader` is used to:
/// - Pin the thread to the current version via `pin()`.
/// - Obtain a `PinGuard` that protects access to values and can be dereferenced.
///
/// **Thread Safety**: `LocalReader` is not `Sync` and must be used by only one thread.
///
/// 读者线程的本地版本状态。
/// 每个读者线程应该通过 `SwmrCell::local()` 创建恰好一个 `LocalReader`。
/// 它是 `!Sync` 的（因为 `Cell`），必须在每个线程中存储。
/// `LocalReader` 用于：
/// - 通过 `pin()` 将线程钉住到当前版本。
/// - 获取保护对值访问的 `PinGuard`，可以解引用来读取值。
/// **线程安全性**：`LocalReader` 不是 `Sync` 的，必须仅由一个线程使用。
pub struct LocalReader<T: 'static> {
    slot: Arc<ReaderSlot>,
    shared: Arc<SharedState>,
    ptr: Arc<AtomicPtr<T>>,
    pin_count: Cell<usize>,
}

impl<T: 'static> LocalReader<T> {
    fn new(shared: Arc<SharedState>, ptr: Arc<AtomicPtr<T>>) -> Self {
        let slot = Arc::new(ReaderSlot {
            active_version: AtomicUsize::new(INACTIVE_VERSION),
        });

        // Register the reader immediately in the shared readers list
        shared.readers.lock().push(Arc::clone(&slot));

        LocalReader {
            slot,
            shared,
            ptr,
            pin_count: Cell::new(0),
        }
    }
    
    /// Pin this thread to the current version.
    ///
    /// Returns a `PinGuard` that keeps the thread pinned for its lifetime.
    /// The guard can be dereferenced to access the current value.
    ///
    /// **Reentrancy**: This method is reentrant. Multiple calls can be nested, and the thread
    /// remains pinned until all returned guards are dropped. You can also clone a guard to create
    /// additional references: `let guard2 = guard1.clone();`
    ///
    /// **Example**:
    /// ```ignore
    /// let local = cell.local();
    /// let guard1 = local.pin();
    /// let value = *guard1;  // Dereference to read
    /// let guard2 = local.pin();  // Reentrant call
    /// let guard3 = guard1.clone();     // Clone for nested scope
    /// // Thread remains pinned until all three guards are dropped
    /// ```
    ///
    /// While pinned, the thread is considered "active" at a particular version,
    /// and the garbage collector will not reclaim data from that version.
    ///
    /// 将此线程钉住到当前版本。
    ///
    /// 返回一个 `PinGuard`，在其生命周期内保持线程被钉住。
    /// 可以解引用该守卫来访问当前值。
    ///
    /// **可重入性**：此方法是可重入的。多个调用可以嵌套，线程在所有返回的守卫被 drop 之前保持被钉住。
    /// 你也可以克隆一个守卫来创建额外的引用：`let guard2 = guard1.clone();`
    ///
    /// **示例**：
    /// ```ignore
    /// let local = cell.local();
    /// let guard1 = local.pin();
    /// let value = *guard1;  // 解引用来读取
    /// let guard2 = local.pin();  // 可重入调用
    /// let guard3 = guard1.clone();     // 克隆用于嵌套作用域
    /// // 线程保持被钉住直到所有三个守卫被 drop
    /// ```
    ///
    /// 当被钉住时，线程被认为在特定版本"活跃"，垃圾回收器不会回收该版本的数据。
    #[inline]
    pub fn pin(&self) -> PinGuard<'_, T> {
        let pin_count = self.pin_count.get();

        // Reentrant pin: the version is already protected by the outer pin.
        // Just increment count and reuse the existing pinned pointer.
        // 可重入 pin：版本已经被外层 pin 保护。
        // 只需增加计数并复用现有的 pinned 指针。
        if pin_count > 0 {
            self.pin_count.set(pin_count + 1);
            
            // Load the pointer that corresponds to our already-pinned version.
            // Since we're reentrant, we should see the same or newer pointer.
            // 加载与我们已 pin 版本对应的指针。
            // 由于是可重入的，我们应该看到相同或更新的指针。
            let ptr = self.ptr.load(Ordering::Acquire);
            
            return PinGuard { local: self, ptr };
        }

        // First pin: need to acquire a version and validate it.
        // 首次 pin：需要获取版本并验证。
        loop {
            let current_version = self.shared.global_version.load(Ordering::Relaxed);
            
            self.slot
                .active_version
                .store(current_version, Ordering::Relaxed);

            swmr_barrier::light_barrier();

            // Check if our version is still valid (not yet reclaimed).
            // 检查我们的版本是否仍然有效（尚未被回收）。
            let min_active = self.shared.min_active_version.load(Ordering::Acquire);
            
            if current_version >= min_active {
                break;
            }
            
            // Version was reclaimed between our read and store.
            // Retry with a fresh version.
            // 版本在我们读取和存储之间被回收了。
            // 用新版本重试。
            std::hint::spin_loop();
        }

        self.pin_count.set(1);

        // Capture the pointer at pin time for snapshot semantics.
        // 在 pin 时捕获指针以实现快照语义。
        let ptr = self.ptr.load(Ordering::Acquire);

        PinGuard { local: self, ptr }
    }
}

impl<T: 'static> Clone for LocalReader<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self::new(self.shared.clone(), self.ptr.clone())
    }
}

/// A guard that keeps the current thread pinned to a version.
///
/// `PinGuard` is obtained by calling `LocalReader::pin()`.
/// It implements `Deref<Target = T>` to allow reading the current value.
/// It is `!Send` and `!Sync` because it references a `!Sync` `LocalReader`.
/// Its lifetime is bound to the `LocalReader` it came from.
///
/// While a `PinGuard` is held, the thread is considered "active" at a particular version,
/// and the garbage collector will not reclaim data from that version.
///
/// `PinGuard` supports internal cloning via reference counting (increments the pin count),
/// allowing nested pinning. The thread remains pinned until all cloned guards are dropped.
///
/// **Safety**: The `PinGuard` is the mechanism that ensures safe concurrent access to
/// shared values. Readers must always hold a valid `PinGuard` when accessing
/// shared data.
///
/// 一个保持当前线程被钉住到一个版本的守卫。
/// `PinGuard` 通过调用 `LocalReader::pin()` 获得。
/// 它实现了 `Deref<Target = T>`，允许读取当前值。
/// 它是 `!Send` 和 `!Sync` 的，因为它引用了一个 `!Sync` 的 `LocalReader`。
/// 它的生命周期被绑定到它来自的 `LocalReader`。
/// 当 `PinGuard` 被持有时，线程被认为在特定版本"活跃"，
/// 垃圾回收器不会回收该版本的数据。
/// `PinGuard` 支持通过引用计数的内部克隆（增加 pin 计数），允许嵌套 pinning。
/// 线程保持被钉住直到所有克隆的守卫被 drop。
/// **安全性**：`PinGuard` 是确保对值安全并发访问的机制。
/// 读者在访问共享数据时必须始终持有有效的 `PinGuard`。
#[must_use]
pub struct PinGuard<'a, T: 'static> {
    local: &'a LocalReader<T>,
    /// The pointer captured at pin time for snapshot semantics.
    /// 在 pin 时捕获的指针，用于快照语义。
    ptr: *const T,
}

impl<'a, T> Deref for PinGuard<'a, T> {
    type Target = T;

    /// Dereference to access the pinned value.
    ///
    /// Returns a reference to the value that was current when this guard was created.
    /// This provides snapshot semantics - the value won't change during the guard's lifetime.
    ///
    /// 解引用以访问被 pin 的值。
    ///
    /// 返回对创建此守卫时当前值的引用。
    /// 这提供了快照语义 - 在守卫的生命周期内值不会改变。
    #[inline]
    fn deref(&self) -> &T {
        // Safety: pin() guarantees pinned_version >= min_active,
        // and the pointer was captured at pin time.
        // The value is valid as long as guard is held.
        // 安全性：pin() 保证 pinned_version >= min_active，
        // 并且指针在 pin 时被捕获。
        // 只要 guard 被持有，值就是有效的。
        unsafe { &*self.ptr }
    }
}

impl<'a, T> Clone for PinGuard<'a, T> {
    /// Clone this guard to create a nested pin.
    ///
    /// Cloning increments the pin count, and the thread remains pinned until all cloned guards
    /// are dropped. This allows multiple scopes to hold pins simultaneously.
    ///
    /// 克隆此守卫以创建嵌套 pin。
    ///
    /// 克隆会增加 pin 计数，线程保持被钉住直到所有克隆的守卫被 drop。
    /// 这允许多个作用域同时持有 pin。
    #[inline]
    fn clone(&self) -> Self {
        let pin_count = self.local.pin_count.get();

        assert!(
            pin_count > 0,
            "BUG: Cloning a PinGuard in an unpinned state (pin_count = 0). \
             This indicates incorrect API usage or a library bug."
        );

        self.local.pin_count.set(pin_count + 1);

        PinGuard {
            local: self.local,
            ptr: self.ptr,
        }
    }
}

impl<'a, T> Drop for PinGuard<'a, T> {
    #[inline]
    fn drop(&mut self) {
        let pin_count = self.local.pin_count.get();

        assert!(
            pin_count > 0,
            "BUG: Dropping a PinGuard in an unpinned state (pin_count = 0). \
             This indicates incorrect API usage or a library bug."
        );

        if pin_count == 1 {
            self.local
                .slot
                .active_version
                .store(INACTIVE_VERSION, Ordering::Release);
        }

        self.local.pin_count.set(pin_count - 1);
    }
}
