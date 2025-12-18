# SWMR-Cell: 基于版本的单写多读单元

[![Crates.io](https://img.shields.io/crates/v/swmr-cell)](https://crates.io/crates/swmr-cell)
[![Documentation](https://docs.rs/swmr-cell/badge.svg)](https://docs.rs/swmr-cell)
[![License](https://img.shields.io/crates/l/swmr-cell)](LICENSE-MIT)

[English Documentation](./README.md)

`swmr-cell` 提供了一个线程安全的 `SwmrCell`，支持**并发无等待（Wait-Free）读取**和**无锁（Lock-Free）写入**（分摊成本），并使用基于版本的垃圾回收机制。它专为单写入者频繁更新数据、同时多个读取者并发访问的高性能场景而设计。

## 特性

- **单写多读 (SWMR)**：针对单写入线程和多并发读取线程进行了优化。
- **无等待读取**：读取者获取数据快照时无需加锁、阻塞，也不会干扰写入者。
- **基于版本的垃圾回收**：使用类似于 RCU (Read-Copy-Update) 或基于纪元（Epoch）的机制，当没有读取者观测时自动回收旧数据版本。
- **快照语义**：只要 `PinGuard` 存活，读取者就持有数据的一致视图。
- **最小化同步**：
  - 读取者使用原子操作注册其活跃版本。
  - 写入者仅在管理读取者注册和在回收期间扫描读取者状态时使用互斥锁。
- **可配置的 GC**：支持带有可调阈值的自动或手动垃圾回收。
- **no_std 支持**：兼容 `no_std` 环境（依赖 `alloc`），需启用 `spin` 特性。
- **类型安全**：完整的 Rust 所有权和生命周期安全保证。

## 使用方法

在 `Cargo.toml` 中添加 `swmr-cell`：

```toml
[dependencies]
swmr-cell = "0.1"
```

### no_std 支持

要在 `no_std` 环境中使用 `swmr-cell`，请禁用默认特性并启用 `spin` 特性。注意：本库依赖 `alloc`，因此需要提供全局分配器。

```toml
[dependencies]
swmr-cell = { version = "0.1", default-features = false, features = ["spin"] }
```

### 基本示例

```rust
use swmr_cell::SwmrCell;

// 1. 使用初始值创建一个新的 SWMR 单元
let mut cell = SwmrCell::new(42i32);

// 2. 为当前线程创建一个本地读取者（每个线程都需要自己的 local reader）
let local = cell.local();

// 3. Pin 住并读取值
// guard 提供了 pin 时刻的值的快照
let guard = local.pin();
assert_eq!(*guard, 42);
drop(guard); // 读取者不再访问数据

// 4. 写入者更新值
// 这会创建一个新版本；旧版本被标记为退休
cell.store(100i32);

// 5. 读取新值
let guard = local.pin();
assert_eq!(*guard, 100);
```

### 多线程示例

```rust
use swmr_cell::SwmrCell;
use std::sync::Arc;
use std::thread;

fn main() {
    // 如果需要传递 cell，可以将其包装在智能指针中。
    // 不过通常写入者持有 cell，读取者持有它们自己的 LocalReader。
    // 这里我们将 cell 保留在主线程（写入者）中。
    let mut cell = SwmrCell::new(0);
    
    // 为后台线程创建 LocalReader
    // LocalReader 是 !Sync 的，所以我们在这里创建并发送它，
    // 或者如果我们可以共享访问 cell，也可以在线程内部创建。
    let mut reader_handles = vec![];
    
    for i in 0..3 {
        // SwmrCell::local() 创建一个连接到该 cell 的读取者
        let local = cell.local();
        
        let handle = thread::spawn(move || {
            loop {
                let guard = local.pin();
                let val = *guard;
                println!("Reader {} saw: {}", i, val);
                if val == 10 { break; }
                drop(guard); // 在休眠或执行其他工作前 unpin
                thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        reader_handles.push(handle);
    }
    
    // 写入者更新值
    for i in 1..=10 {
        cell.store(i);
        thread::sleep(std::time::Duration::from_millis(20));
    }
    
    for h in reader_handles {
        h.join().unwrap();
    }
}
```

### 使用 `SwmrReader` 共享读取者创建能力

如果需要将创建读取者的能力分发给多个线程（例如，在线程动态变化的线程池中），可以使用 `SwmrReader`。与 `LocalReader` 不同，`SwmrReader` 是 `Sync` 和 `Clone` 的。

```rust
use swmr_cell::SwmrCell;
use std::thread;

let mut cell = SwmrCell::new(0);

// 创建一个可以共享的 SwmrReader 工厂
let reader_factory = cell.reader();

for i in 0..3 {
    // 为每个线程克隆工厂
    let factory = reader_factory.clone();
    
    thread::spawn(move || {
        // 使用工厂在线程上创建 LocalReader
        let local = factory.local();
        
        // ... 使用 local reader ...
    });
}
```

你也可以从现有的 `LocalReader` 获取 `SwmrReader`，以便将创建能力传递给另一个线程：

```rust
// 1. Share: 从 LocalReader 创建 SwmrReader
let local_reader = cell.local();
let swmr_reader = local_reader.share(); // 返回 SwmrReader
thread::spawn(move || {
    let local = swmr_reader.local();
    // ...
});

// 2. Convert: 消耗 LocalReader 以获取 SwmrReader
let local_reader = cell.local();
let swmr_reader = local_reader.into_swmr(); // 消耗 local_reader
thread::spawn(move || {
    let local = swmr_reader.local();
    // ...
});
```

## 配置

你可以使用 `SwmrCell::builder()` 自定义垃圾回收行为：

```rust
use swmr_cell::SwmrCell;

let mut cell = SwmrCell::builder()
    .auto_reclaim_threshold(Some(128)) // 在 128 次存储后自动触发 GC（默认：16）
    .build(0);
```

- **auto_reclaim_threshold**：控制在扫描读取者进行回收之前缓冲多少个已退休对象。较大的值会减少 GC 开销，但会增加内存使用。

## 工作原理

1.  **版本**：Cell 维护一个全局单调版本计数器。每次 `store` 都会增加版本。
2.  **读取者**：每个 `LocalReader` 都有一个槽，用于发布它当前正在访问的版本（`active_version`）。
3.  **回收**：当写入者更新值时，旧值连同其所属的版本号一起被加入垃圾队列。
4.  **收集**：写入者扫描所有活跃的读取者槽。它计算当前对任何读取者可见的最小版本。任何严格早于此最小版本的垃圾都可以安全回收。

## 安全性

- `LocalReader` 是 `!Sync` 的，必须保留在使用它的线程上。
- `PinGuard` 的生命周期绑定到 `LocalReader`，并防止底层数据在持有时被释放。
- 通过 `PinGuard` 访问数据是安全的，因为写入者保证不会释放任何对活跃读取者可见的版本。

## 许可证

本项目采用以下任一许可证授权：

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

由你选择。
