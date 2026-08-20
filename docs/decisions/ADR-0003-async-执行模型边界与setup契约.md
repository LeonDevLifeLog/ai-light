# ADR-0003: Async 执行模型边界与 `setup` 契约

| 项目 | 内容 |
|---|---|
| 状态 | **Accepted（已确定）** |
| 日期 | 2026-08-20 |
| 决策人 | Leon |
| 关联 | `docs/specs/architecture.md` KAD-08、U-06（已落地）、ADR-0001/0002 |

---

## 背景

应用启动期在 macOS 上 abort：

```
panicked at crates/ailight-core/src/transport.rs:69:
there is no reactor running, must be called from the context of a Tokio 1.x runtime
→ panic in a function that cannot unwind
→ tao::platform_impl::platform::app_delegate::did_finish_launching
→ thread caused non-unwinding panic. aborting.
```

调用链：

| 层 | 文件:行 | 关键代码 |
|---|---|---|
| ① Tauri setup 回调 | `src-tauri/src/lib.rs:76` | `Engine::new(shared.clone(), device_io.clone())` |
| ② Engine 构造 | `crates/ailight-core/src/engine.rs:116` | `Transport::new(io, None)` |
| ③ panic 点 | `crates/ailight-core/src/transport.rs:69` | `tokio::spawn(writer_task(...))` |

`tokio::spawn` 依赖 **thread-local 的 runtime handle**，要求调用处身处 Tokio runtime 上下文内。Tauri 的 `.setup()` 回调运行在 macOS 主线程的 AppKit `did_finish_launching` 里，**不在**任何 Tokio runtime 上下文中——于是直接 panic。

由于该 panic 跨 `extern "C"` FFI 边界（不可 unwind），Rust 二次 panic 转 `abort`，所以错误信息同时夹着 `no reactor running`（Tokio 方言）和 `panic in a function that cannot unwind`（AppKit 方言）。

### 为什么 64 个测试没拦住

`transport.rs` / `engine.rs` 的测试全部用 `#[tokio::test]`，该宏自动构造并进入 runtime 上下文——**恰好系统性地伪造了一个生产环境不成立的前提**。被测单元（`Transport::new` / `Engine::new`）对环境存在隐式契约（"必须在 Tokio runtime 中调用"），而测试框架无偿提供了该环境。这是"被测框架掩盖被测契约"的典型盲区。

### U-06 已经预言了这件事

`docs/specs/architecture.md:163` 的 U-06：

> U-06 | Tauri async command 与 btleplug 事件循环线程模型整合 | KAD-01/03 | spike 先行

只是当时只盯了 BLE 线程（`ble.rs:196` 的 `tokio::spawn` 位于 `BleIo::connect()` 这个 `async fn` 内部，运行时必定已在 runtime 里——所以一直没事）。本次炸的是 setup 回调这一侧——**同一条不确定性的另一面**。

---

## 决策（4 项，全部确认）

### D-01 `core` 不引入 Tauri 依赖

**确定：`ailight-core` 仍只依赖 `tokio`，不引入 `tauri` 作为依赖。**

理由：core 是被多个上下文（CLI 工具、未来 headless 服务、单元测试、Tauri setup）复用的核心库，反向依赖 UI 框架会破坏分层与可测性。

后果：**tokio runtime 上下文的契约必须由调用方保证**，core 不替调用方抹平这条缝。

### D-02 `setup` 中进入 runtime 上下文后构造 Engine

**确定：在 `src-tauri/src/lib.rs:75-76`，把 `Engine::new` 包在 `tauri::async_runtime::handle().inner().enter()` 返回的 guard 作用域内。**

```rust
let device_io = DeviceIo::new();
let engine = {
    let _guard = tauri::async_runtime::handle().inner().enter();
    Engine::new(shared.clone(), device_io.clone())
};
```

- `_guard` 仅需被持有，`Handle::enter()` 来源于 tauri 已启用的 tokio，`src-tauri/Cargo.toml` 的 tokio features（`["time", "sync"]`）**暂无需追加 `rt`**（以编译结果为准，若编译器抱怨 `Handle` 类型不可达，再补 `rt` feature）。
- API 已核实存在于 tauri 2.11.5：`async_runtime::handle()` @ `src/async_runtime.rs:265`、`RuntimeHandle::inner()` @ `:181`。

### D-03 `core` 对隐式契约加防御性文档 + `debug_assert`

**确定：在 `Transport::new`（`transport.rs:66`）与 `Engine::new`（`engine.rs:115`）增加 `debug_assert!(tokio::runtime::Handle::try_current().is_ok(), ...)` 并在文档注释中显式声明该契约。**

理由：让"必须在 Tokio runtime 中调用"从注释里的约定变成**编译/调试期可验证的事实**。一旦未来调用方再犯，开发期立刻爆，比线上 abort 友好。

注：测试环境的 `#[tokio::test]` 仍满足此断言（保持 64 个测试全绿）。

### D-04 setup 回调里不再"裸"启动常驻任务

**确定：`src-tauri/src/lib.rs:80-102` 中现有的 3 处 `tauri::async_runtime::spawn` 维持现状（它们本身已在 runtime 上下文，安全）；新增的常驻任务一律走 `tauri::async_runtime::spawn`，**禁止**在 setup 回调里直接调用 `tokio::spawn`。**

理由：setup 回调是 AppKit ↔ Tokio 这两条执行模型的**缝**。缝里的代码应该一律只与 Tauri 的 async 抽象打交道，而不是直接绑到 tokio 的原语上——这条规则本质上是把 D-02 的"一次修复"升级为"长期不变量"。

---

## 备选方案

### 备选 A：在 `Engine::new` 内部用 `tokio::runtime::Handle::try_current()` 失败时自动 `block_on` 一个内部 runtime

否决：会让 Engine 在每次无 runtime 时新建一个 runtime，行为不可预期（任务到底跑在哪个 runtime？），且 core 库不该有"自起 runtime"的权力。

### 备选 B：把 `Transport::new` 改成 `(Transport, impl Future<Output = ()>)`，由调用方在 async fn 中 await 启动

优点：把"必须有 runtime"从注释变成签名。
否决：当前是 64 个测试覆盖的稳定 API，改签名牵动所有调用方与测试；本期作为 **后续改进** 留档（见 ADR 末尾"未决事项"），不在本轮落地。

### 备选 C：改用 `tauri::async_runtime::block_on(async { Engine::new(...) })`

效果与 D-02 等价，但 `Handle::block_on` 在**已被 runtime 持有**的线程上调用会直接 panic——比 `_guard = ... .enter()` 更脆。**仅在 D-02 编译失败时作兜底**。

---

## 后果

- **正面**：启动 panic 解决；core 保持分层独立；契约被断言化，未来同类问题在开发期可见
- **负面**：core 的 `new()` API 增加了隐式前提，对"非 async 入口"的友好度下降；但本仓库**唯一**的非 async 入口就是 setup 回调，D-02 已堵死
- **迁移成本**：仅 `src-tauri/src/lib.rs:75-76` 一次小改 + core 两处 `debug_assert` + 文档注释

---

## 关联

- 上游：`docs/specs/architecture.md` KAD-08（本 ADR 的实施条款）
- 平级：ADR-0001（接入层协议）、ADR-0002（主题格式）
- 解决：`docs/specs/architecture.md` §4 U-06（落地）
- 后续：备选 B（`Transport::new` 改为返回 future pair）作为更长期的方向，待 V1.0 稳定后另开 ADR

---

## 未决事项

1. **V1.1+ 候选**：将 `Transport::new` / `Engine::new` 改为返回 `(Self, impl Future<Output = ()>)`，把"必须有 runtime"提升为类型系统事实（备选 B）
2. **测试补强**：在 `ailight-core` 加一条"在非 runtime 上下文下构造应触发 `debug_assert`"的测试（用 `std::thread::spawn` 在新线程里构造并 `join`，验证调试构建 panic）