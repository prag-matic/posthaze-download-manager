# Research: Evaluating `microsoft/windows-rs` Crates for Posthaze Download Manager Performance

> **Scope:** Static codebase audit of `posthaze-engine`, `posthaze-ui`, and `posthaze-cli`, evaluated against primary documentation for [`microsoft/windows-rs`](https://github.com/microsoft/windows-rs) and Microsoft Learn Win32 APIs.
> **Non-goal:** No application source code is modified by this report.

---

## 1. Executive Verdict

**TL;DR:** Posthaze Download Manager **already uses `windows-sys` 0.60**—the fastest, lowest-overhead crate in the `microsoft/windows-rs` family—in both `posthaze-engine` and `posthaze-ui`. Switching to the higher-level `windows` crate or replacing core Rust/Tokio subsystems (`reqwest` + `rustls`, `blake3`, `PartFileWriter`'s `FileExt::seek_write`) with Win32 APIs via `windows-rs` will **not** improve download throughput and would in several cases **hurt** binary size, compile time, portability, or feature parity (such as SOCKS5 proxy support).

A critical distinction is that `windows-rs` is a **Rust language projection (FFI binding)** over Windows OS APIs—it does not add algorithmic acceleration on its own. Any real performance, CPU, RAM, or battery improvement comes strictly from **which Windows OS API** is called, not which `windows-rs` crate wraps it.

### Where `windows-rs` / Win32 APIs Genuinely Help vs. Hurt

| Area | Verdict | Reason |
|---|---|---|
| **Process Power / EcoQoS in Tray** (`SetProcessInformation` via `windows-sys`) | **Helps (Battery / CPU / Thermals)** | Opting into `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` (EcoQoS) when hidden to tray and idle schedules background work onto E-cores at lower frequency. |
| **Browser Registry Checks/Writes** (`Win32_System_Registry` or `windows-registry` replacing `reg.exe` in `browser_integration.rs`) | **Helps (UI Latency / Process Overhead)** | Eliminates 3–6 `reg.exe` subprocess spawns (`CreateProcessW` + AV scan hooks) when querying or registering Chrome/Edge/Brave Native Messaging keys. |
| **Sequential Read Hint for Hashing** (`FILE_FLAG_SEQUENTIAL_SCAN` in `compute_file_blake3`) | **Minor Help (Disk Read-Ahead)** | Doubles Windows Cache Manager read-ahead and evicts read-behind pages when hashing large completed files. |
| **Replacing `windows-sys` with `windows` Crate** | **Hurts (Compile Time & Binary Size)** | `windows` generates COM/WinRT wrappers, `Result` plumbing, and vtable/trait codegen; `windows-sys` emits zero-overhead raw C ABI declarations. |
| **Replacing `reqwest` + `rustls` with WinHTTP** | **Hurts (Features & Async Integration)** | WinHTTP lacks SOCKS4/SOCKS5 proxy support, uses a callback-on-system-threadpool async model (`WINHTTP_FLAG_ASYNC`) that fights Tokio, and breaks cross-platform `posthaze-engine`. |
| **Disk `SetFileValidData` / `FILE_FLAG_NO_BUFFERING`** | **Harmful / Impractical** | `SetFileValidData` requires `SeManageVolumePrivilege` (admin-only); `FILE_FLAG_NO_BUFFERING` requires strict sector-aligned buffers (`VirtualAlloc`), offsets, and lengths. |
| **Replacing `blake3` with Windows CNG (`BCrypt`)** | **Not Applicable / Hurts** | Windows CNG has no BLAKE3 algorithm; `blake3` already uses SIMD (AVX2/AVX-512) in user space without kernel/handle overhead. |
| **Replacing Tokio Blocking Pool with `windows-threading`** | **Neutral to Negative** | `EngineBridge` already bounds Tokio to `2..=4` workers and `16` blocking threads with `5s` keep-alive; `PartFileWriter` runs 1 dedicated blocking loop per download. |

---

## 2. Inventory of `microsoft/windows-rs` Crates

The [`microsoft/windows-rs`](https://github.com/microsoft/windows-rs/tree/master/crates/libs) repository publishes a family of focused crates under `crates/libs/`. Inspecting `Cargo.lock`, `crates/posthaze-engine/Cargo.toml`, and `crates/posthaze-ui/Cargo.toml` shows that **all major `windows-rs` crates are already present in Posthaze's dependency graph** (either directly via `windows-sys` or transitively via `gpui-kit`, `tray-icon`, `tokio`, and `reqwest`):

| Crate | Purpose in `microsoft/windows-rs` | Status in Posthaze (`Cargo.lock`) | Direct Usage |
|---|---|---|---|
| `windows-sys` | Raw C-style Win32 FFI declarations, structs, and constants (`no_std`, zero wrapper overhead) | `0.60.2` (direct), plus `0.48.0`, `0.52.0`, `0.61.2` (transitive) | `posthaze-engine` (`Win32_System_Com`, `Win32_UI_Shell`), `posthaze-ui` (DWM, Registry, Threading, Shell, Accessibility, WDK, Security) |
| `windows` | High-level safe idiomatic Rust bindings for Win32, COM, and WinRT | `0.57.0`, `0.58.0`, `0.61.3`, `0.62.2` (transitive only) | None (pulled in by `gpui-kit` / `tray-icon` / ecosystem crates) |
| `windows-core` | COM/WinRT `Interface` traits, `Error`, `Result`, `HSTRING` interop | `0.57.0`, `0.58.0`, `0.61.2`, `0.62.2` (transitive) | None |
| `windows-strings` | `HSTRING`, `PCSTR`, `PCWSTR`, `BSTR`, UTF-16 literals (`w!`, `s!`) | `0.1.0`, `0.3.1`, `0.4.2`, `0.5.1` (transitive) | None (`platform_hooks.rs` uses manual `Vec<u16>` / `const` UTF-16 arrays) |
| `windows-registry` | Safe high-level wrapper over Win32 Registry (`CURRENT_USER`, `Key`, transactions) | `0.4.0`, `0.6.1` (transitive) | None (`platform_hooks.rs` uses `windows_sys::Win32::System::Registry`; `browser_integration.rs` shells out to `reg.exe`) |
| `windows-threading` | Safe wrapper over Windows Thread Pool (`submit`, `Pool`, `Scope`, `for_each`) | `0.1.0`, `0.2.1` (transitive) | None |
| `windows-result` | Lightweight `HRESULT` / `WIN32_ERROR` `Result` wrapper | `0.1.2`, `0.2.0`, `0.3.4`, `0.4.1` (transitive) | None |
| `windows-future` | Async `IAsyncAction` / `IAsyncOperation` WinRT future adapters | `0.2.1`, `0.3.2` (transitive) | None |
| `windows-numerics` | `Vector2`/`Vector3`/`Matrix4x4` math types for WinRT/DirectX | `0.2.0`, `0.3.1` (transitive) | None |
| `windows-collections` | WinRT `IVector` / `IMap` collection wrappers | `0.2.0`, `0.3.2` (transitive) | None |
| `windows-implement` / `windows-interface` | Proc-macros for implementing/defining COM & WinRT interfaces | `0.57.0`–`0.60.2` (transitive) | None |
| `windows-targets` / `windows-link` | Import-library / `#[link]` target-triple glue (`windows_x86_64_msvc`, etc.) | `0.48.5`–`0.53.5` / `0.1.3`, `0.2.1` (transitive) | Used implicitly by `windows-sys` |
| `windows-version` | Lightweight `RtlGetVersion` OS build check | `0.1.7` (transitive) | None (`platform_hooks.rs` calls `RtlGetVersion` via `windows-sys`) |

---

## 3. Subsystem-by-Subsystem Performance Evaluation

### 3.1 Disk I/O (`crates/posthaze-engine/src/writer.rs`)

**Current Implementation:**
- `PartFileWriter::spawn_with_checkpoint` (`writer.rs:284-310`) opens the `.part` file with `std::fs::OpenOptions` and pre-allocates size via `file.set_len(len)` (`writer.rs:300-302`).
- Writes run on a dedicated `tokio::task::spawn_blocking` thread (`writer.rs:352-399`) fed by a bounded `mpsc::channel` (`WRITER_CHANNEL_CAPACITY = 32`, `writer.rs:25`) and a lock-protected `ChunkBufferPool` (`writer.rs:35-93`).
- On Windows, `PositionalFile::write_all_at` (`writer.rs:224-228`) uses `std::os::windows::fs::FileExt::seek_write`, which issues a single `WriteFile` syscall with an `OVERLAPPED` offset—already eliminating the `seek()` + `write()` double-syscall overhead.

**Evaluation of Win32 File APIs via `windows-sys`:**
1. **`SetFileInformationByHandle` (`FileEndOfFileInfo` / `FileAllocationInfo`):**
   - `std::fs::File::set_len` on Windows already calls `SetFileInformationByHandle` with `FileEndOfFileInfo` (setting EOF). Calling `FileAllocationInfo` separately only reserves clusters without extending EOF, which `FileEndOfFileInfo` already accomplishes for `.part` preallocation. No measurable throughput gain over `file.set_len(len)`.
2. **`SetFileValidData`:**
   - Advances the file's Valid Data Length (VDL) without zero-filling clusters on disk, which can speed up sparse/random writes into huge preallocated files on NTFS.
   - **Blocking Caveat:** Per Microsoft Learn, `SetFileValidData` requires the **`SeManageVolumePrivilege`** (*"Perform volume maintenance tasks"*), because unzeroed clusters could expose previously deleted file contents from other users. Standard non-elevated desktop processes do **not** have this privilege, so `SetFileValidData` will fail with `ERROR_PRIVILEGE_NOT_HELD` for normal users. **Do not use.**
3. **`FILE_FLAG_WRITE_THROUGH` & `FILE_FLAG_NO_BUFFERING`:**
   - `FILE_FLAG_NO_BUFFERING` bypasses the Windows Cache Manager, but Microsoft Learn enforces three strict rules: memory buffer addresses, file offsets, and transfer lengths must **all** be integer multiples of the volume's physical sector size (typically 512 or 4,096 bytes), requiring `VirtualAlloc`-aligned buffers and manual tail-padding/truncation. Posthaze's `Chunk`s are arbitrary HTTP stream slices (`512 KiB` nominal, with odd tail chunks on segment boundaries and pause/resume). Bypassing the OS page cache also hurts sequential/near-sequential write coalescing on HDDs and SATA SSDs.
   - `FILE_FLAG_WRITE_THROUGH` forces every `WriteFile` to flush through to media immediately—drastically **reducing** multi-segment throughput in exchange for crash durability that Posthaze already handles safely via `PartFileWriter` checkpoints and `sync_on_complete` (`sink.sync_all()`, `writer.rs:395-397`). **Do not use.**
4. **`OVERLAPPED` Async I/O / IOCP vs. Dedicated `spawn_blocking` + `seek_write`:**
   - `PartFileWriter` already serializes writes for a single `.part` file onto one dedicated blocking thread using `FileExt::seek_write` (`512 KiB` chunks). Because NTFS serializes extending writes to a single file handle anyway, rewriting `PartFileWriter` around raw Win32 `CreateFileW(FILE_FLAG_OVERLAPPED)` + IOCP would add substantial `unsafe` buffer-pinning complexity for negligible gain on consumer NVMe/SSD drives.
5. **`FILE_FLAG_SEQUENTIAL_SCAN` (for `compute_file_blake3`):**
   - `DownloadEngine::compute_file_blake3` (`crates/posthaze-engine/src/lib.rs:537-563`) opens the completed file with `std::fs::File::open(&path)` and reads sequentially in `64 KiB` chunks. Opening with `OpenOptionsExt::custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)` (via `windows-sys` or raw constant `0x08000000`) hints the Windows Cache Manager to double read-ahead prefetching and aggressively evict read-behind pages, avoiding page-cache pollution when hashing multi-GiB downloads.

### 3.2 Networking / TLS / HTTP (`reqwest` + `rustls` vs. WinHTTP)

**Current Implementation:**
- `posthaze-engine` uses `reqwest` 0.12 (`features = ["rustls-tls", "stream"]`, `Cargo.toml:18`) with `pool_max_idle_per_host(6)`, `pool_idle_timeout(30s)`, `tcp_nodelay(true)`, and `ProxyMode::{System, None, Manual}` (`lib.rs:673-701`).

**Evaluation of `Win32_Networking_WinHttp` (`WinHttpOpen` / `WinHttpReadData`):**
- **RAM / Binary Size:** Using WinHTTP offloads TLS (Schannel) and HTTP/2 framing to `winhttp.dll` in Windows, which could marginally reduce binary size and user-space TLS state.
- **Critical Dealbreakers:**
  1. **No SOCKS Proxy Support:** Per Microsoft Learn (`WinHttpOpen` and `netsh winhttp set advproxy` docs), WinHTTP recognizes **only CERN-type (HTTP/HTTPS) proxies** and explicitly does **not** support SOCKS4 or SOCKS5 proxies. (`reqwest` supports SOCKS5 via its `socks` feature if enabled, whereas WinHTTP cannot.)
  2. **Async Model Mismatch:** WinHTTP's `WINHTTP_FLAG_ASYNC` delivers completions via a C status callback (`WinHttpSetStatusCallback`) invoked on arbitrary Windows system threadpool threads, requiring manual `OVERLAPPED`-style buffer lifetime pinning (`WinHttpReadData` with `lpdwNumberOfBytesRead = NULL`), state machines, and `WINHTTP_CALLBACK_STATUS_HANDLE_CLOSING` teardown synchronization before bridging back into Tokio channels.
  3. **Portability & Control:** `posthaze-engine` is a headless cross-platform crate (`Cargo.toml` / `writer.rs` `#[cfg(not(windows))]` paths). Replacing `reqwest` with WinHTTP would fracture the HTTP stack across OSes and lose fine-grained `tokio`/`hyper` stream backpressure integration.
- **Verdict:** **Keep `reqwest` + `rustls`.** Do not replace with WinHTTP.

### 3.3 OS Thread Pool (`windows-threading`)

**Current Implementation:**
- `EngineBridge::open` (`crates/posthaze-ui/src/bridge.rs:74-91`) configures Tokio with `worker_threads` clamped to `2..=4`, `max_blocking_threads(16)`, and `thread_keep_alive(5s)`.

**Evaluation:**
- `windows-threading` wraps the Win32 Thread Pool (`SubmitThreadpoolWork` / `TrySubmitThreadpoolCallback`) with `submit`, `Pool`, `Scope`, and `for_each`.
- While the Windows Thread Pool dynamically scales threads at the kernel level, Tokio's bounded `spawn_blocking` pool (`max_blocking_threads(16)`, `5s` idle expiry) already prevents thread explosion and integrates directly with `tokio::task::JoinHandle` (`PartFileWriter::handle`, `compute_file_blake3`, `sqlx`). Moving blocking work to `windows-threading` would require manual `oneshot` channels and lose Tokio's unified task lifecycle for zero throughput gain.
- **Verdict:** **Keep Tokio's bounded runtime.**

### 3.4 Registry & Strings (`windows-registry`, `windows-strings`)

**Current Implementation:**
- `crates/posthaze-ui/src/platform_hooks.rs` already uses `windows_sys::Win32::System::Registry` directly for reading `HKCU\...\Personalize` (`RegGetValueW`, lines `159-183`) and writing `HKCU\...\Run\Posthaze` (`RegCreateKeyW`/`RegSetValueExW`/`RegDeleteValueW`, lines `661-735`).
- **However**, `crates/posthaze-ui/src/browser_integration.rs` spawns **`reg.exe` subprocesses**:
  - `is_browser_registered` (`browser_integration.rs:108-124`) spawns `reg.exe query` for each of Chrome, Edge, and Brave (3 processes).
  - `register_native_host` (`browser_integration.rs:146-173`) spawns `reg.exe add` for each of Chrome, Edge, and Brave (3 processes).

**Evaluation:**
- Replacing `Vec<u16>` + `RegGetValueW` in `platform_hooks.rs` with `windows-registry` / `windows-strings` is purely an **ergonomics/safety** refactor—it does not measurably improve CPU or RAM, and adds `windows-result` / `windows-strings` codegen.
- **In contrast**, replacing `std::process::Command::new("reg.exe")` in `browser_integration.rs` with in-process Win32 Registry calls (using the `Win32_System_Registry` feature **already enabled** in `crates/posthaze-ui/Cargo.toml:31`, or `windows-registry`) eliminates 3–6 Windows `CreateProcessW` calls, console-host/antivirus inspection overhead, and dozens of milliseconds of UI-thread/background stall whenever browser status is queried or registered.

### 3.5 Memory, Process Power (EcoQoS), and `windows-sys` vs. `windows` Footprint

**Process Power Throttling (EcoQoS) via `SetProcessInformation`:**
- `platform_hooks.rs` already implements `hide_window_to_tray` (`platform_hooks.rs:611-623`) and `show_window_from_tray` (`platform_hooks.rs:628-649`).
- Per Microsoft Learn, calling `SetProcessInformation(GetCurrentProcess(), ProcessPowerThrottling, &state, sizeof(state))` with `PROCESS_POWER_THROTTLING_EXECUTION_SPEED` opts the process into **EcoQoS**: Windows lowers CPU clock frequency and schedules threads onto Efficiency cores (E-cores), cutting battery drain, thermals, and fan noise for background/tray applications.
- **Important nuance:** Do **not** enable EcoQoS unconditionally while active multi-Gbps downloads or `blake3` hashes are running, as throttling CPU frequency can bottleneck TLS decryption or UI responsiveness when visible. Enable EcoQoS **only when hidden to the system tray and no downloads are active** (and clear `StateMask = 0` immediately on `show_window_from_tray` or when a download starts).
- `SetProcessInformation` and `PROCESS_POWER_THROTTLING_STATE` are available directly in `windows-sys` under `Win32_System_Threading` (which `posthaze-ui` already enables at `crates/posthaze-ui/Cargo.toml:33`).

**`windows-sys` vs. `windows` Binary & Compile Footprint:**
- Per `microsoft/windows-rs` documentation, `windows-sys` contains **only raw FFI declarations** (no function bodies, no COM vtable wrappers, no `windows::core::Result` error formatting). Because `Cargo.toml` enables `lto = true` and `codegen-units = 1`, `windows-sys` allows the linker to strip every unused symbol cleanly, whereas switching direct dependencies to `windows` would pull in `windows-core` / `windows-result` / `windows-strings` wrapper codegen and slow down incremental builds.
- **Verdict:** **Keep `windows-sys` 0.60** for all direct Windows calls.

### 3.6 Hashing (`BCrypt` CNG vs. `blake3` / `sha2`)

**Current Implementation:**
- `posthaze-engine` uses `blake3 = "1"` (`crates/posthaze-engine/Cargo.toml:8`, `lib.rs:550`) for file integrity verification.
- `posthaze-ui` uses `sha2 = "0.10"` (`crates/posthaze-ui/Cargo.toml:18`, `updater.rs:18`) once per auto-update to verify the downloaded installer against `SHA256SUMS`.

**Evaluation:**
- Windows CNG (`BCryptCreateHash` / `BCryptHashData` / `BCryptFinishHash`) supports SHA-256/SHA-512/etc., but **does not support BLAKE3**.
- `blake3` is already SIMD-accelerated (AVX2/AVX-512 on x86_64) in pure Rust with zero kernel-handle or `BCryptGetProperty` allocation overhead.
- Replacing `sha2` (used once per app update) with `BCryptCreateHash` would save a few kilobytes of binary size at the cost of `unsafe` FFI boilerplate and platform-specific updater code.
- **Verdict:** **Keep `blake3` and `sha2`.**

---

## 4. Concrete Actionable Recommendations

1. **Enable Windows EcoQoS (`SetProcessInformation`) When Idle in System Tray (`posthaze-ui`):**
   - Use `windows_sys::Win32::System::Threading::{SetProcessInformation, GetCurrentProcess, ProcessPowerThrottling, PROCESS_POWER_THROTTLING_STATE, PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED}` (feature `Win32_System_Threading` is already enabled).
   - Toggle `StateMask = PROCESS_POWER_THROTTLING_EXECUTION_SPEED` when the main window is hidden to tray **and** active download count is `0`; reset `StateMask = 0` when `show_window_from_tray` runs or when a download becomes active.
2. **Replace `reg.exe` Subprocesses in `browser_integration.rs` with In-Process Win32 Registry Calls:**
   - Replace `Command::new("reg.exe")` in `is_browser_registered` (`browser_integration.rs:108-124`) and `register_native_host` (`browser_integration.rs:146-173`) with `Win32_System_Registry` (`RegGetValueW` / `RegCreateKeyExW` / `RegSetValueExW` via `windows-sys`, already enabled in `posthaze-ui/Cargo.toml:31`).
   - Eliminates 3–6 `CreateProcessW` spawns and reduces Settings/startup latency.
3. **Add `FILE_FLAG_SEQUENTIAL_SCAN` in `compute_file_blake3` (`posthaze-engine`):**
   - On `#[cfg(windows)]`, use `std::os::windows::fs::OpenOptionsExt::custom_flags(FILE_FLAG_SEQUENTIAL_SCAN)` (`0x0800_0000`) when opening completed files for `blake3` hashing in `lib.rs:549`.
4. **Do Not Change:**
   - Keep `windows-sys` 0.60 (do not migrate to `windows`).
   - Keep `PartFileWriter`'s `FileExt::seek_write` + `ChunkBufferPool` (do not use `SetFileValidData`, `FILE_FLAG_NO_BUFFERING`, or `FILE_FLAG_WRITE_THROUGH`).
   - Keep `reqwest` + `rustls` (do not migrate to WinHTTP).
   - Keep `EngineBridge`'s bounded Tokio pool (do not migrate to `windows-threading`).
   - Keep `blake3` and `sha2` (do not migrate to `BCrypt`).

---

## 5. Primary Sources Cited

### Repository Files
- Workspace dependencies & release profile: [Cargo.toml](file:///e:/Development/posthaze-download-manager/Cargo.toml#L15-L28)
- `posthaze-engine` dependencies (`windows-sys` 0.60, `blake3`, `reqwest`): [crates/posthaze-engine/Cargo.toml](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/Cargo.toml#L7-L21)
- `posthaze-ui` dependencies (`windows-sys` 0.60, `sha2`): [crates/posthaze-ui/Cargo.toml](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/Cargo.toml#L7-L38)
- `PartFileWriter` (`WRITER_CHANNEL_CAPACITY`, `ChunkBufferPool`, `set_len`, `FileExt::seek_write`): [crates/posthaze-engine/src/writer.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/writer.rs#L25-L399)
- `DownloadEngine::compute_file_blake3` & `build_http_client`: [crates/posthaze-engine/src/lib.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/lib.rs#L537-L701)
- `windows_known_downloads_dir` (`SHGetKnownFolderPath` via `windows-sys`): [crates/posthaze-engine/src/model.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-engine/src/model.rs#L680-L710)
- `EngineBridge` Tokio runtime bounds (`worker_threads(2..=4)`, `max_blocking_threads(16)`): [crates/posthaze-ui/src/bridge.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/bridge.rs#L74-L91)
- `platform_hooks.rs` (Registry, DWM, `RtlGetVersion`, `hide_window_to_tray` / `show_window_from_tray`): [crates/posthaze-ui/src/platform_hooks.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/platform_hooks.rs#L121-L735)
- `browser_integration.rs` (`reg.exe` subprocesses): [crates/posthaze-ui/src/browser_integration.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/browser_integration.rs#L108-L173)
- `ipc.rs` (Tokio `NamedPipe` server): [crates/posthaze-ui/src/ipc.rs](file:///e:/Development/posthaze-download-manager/crates/posthaze-ui/src/ipc.rs#L81-L150)

### External Primary Sources
- `microsoft/windows-rs` repository (`crates/libs/`): https://github.com/microsoft/windows-rs/tree/master/crates/libs
- `windows-sys` vs `windows` design & compile-time/binary-size rationale (Kenny Kerr / `windows-rs`): https://github.com/microsoft/windows-rs
- `windows-registry` docs: https://docs.rs/windows-registry/latest/windows_registry/
- `windows-strings` docs: https://docs.rs/windows-strings/latest/windows_strings/
- `windows-threading` docs: https://docs.rs/windows-threading/latest/windows_threading/
- Microsoft Learn — `SetFileInformationByHandle` / `FILE_ALLOCATION_INFO`: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfileinformationbyhandle
- Microsoft Learn — `SetFileValidData` (`SeManageVolumePrivilege`): https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfilevaliddata
- Microsoft Learn — File Buffering (`FILE_FLAG_NO_BUFFERING`, `FILE_FLAG_WRITE_THROUGH`, `FILE_FLAG_SEQUENTIAL_SCAN`): https://learn.microsoft.com/en-us/windows/win32/fileio/file-buffering
- Microsoft Learn — `SetProcessInformation` & `PROCESS_POWER_THROTTLING_STATE` (EcoQoS): https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-setprocessinformation
- Microsoft Learn — `WinHttpOpen` (CERN proxy limitation) & `WinHttpReadData` (`WINHTTP_FLAG_ASYNC`): https://learn.microsoft.com/en-us/windows/win32/api/winhttp/nf-winhttp-winhttpopen
- Microsoft Learn — `BCryptHash` / `BCryptCreateHash` (Windows CNG): https://learn.microsoft.com/en-us/windows/win32/api/bcrypt/nf-bcrypt-bcrypthash
