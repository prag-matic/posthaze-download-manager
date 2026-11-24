# Independent Pre-Flight Window, Folder Picker & Extension Lock

Posthaze's download probing and confirmation flow runs in its own standalone, auto-activating GPUI window (`PreflightWindow` in `crates/posthaze-ui/src/preflight_window.rs`) rather than an in-app modal overlay.

---

## 1. Standalone `PreflightWindow`

- **Foreground Auto-Activation:** Opened with `WindowKind::Normal` and `TitleBar::window_options()`, followed by `window.activate_window()`, so browser-captured downloads (`posthaze-ui --enqueue-url` over Windows named-pipe IPC) pop directly to the foreground even when the main Posthaze window is minimized or unfocused.
- **Concurrent Probe Isolation:** Each probe request spawns its own `PreflightWindow` cascaded by `+28px` X/Y (`cascade_idx % 6`), preventing back-to-back browser captures from overwriting or dropping an in-flight probe.
- **Probing & Error States:**
  - `Probing` renders a `3px` sweeping selenium-amber shutter bar (`preflight-shutter-sweep`) and `HEAD / RANGE` indicator.
  - `Error` uses `errors::describe_error` to present plain-English failure diagnostics (e.g., expired signed URL `403`, rate limit `429`, DNS/timeout/TLS failure) alongside contextual recovery tips, **EDIT URL** (populates the main window's URL bar and activates it), **CLOSE**, and **RETRY**.

---

## 2. Titlebar Tabs (`ADD DOWNLOAD` & `OPTIONS`)

When a probe resolves (`PreflightModalState::Resolved`), the `TitleBar` exposes two compact tabs with dynamic window resizing:

| Tab | Window Size | Contents |
| :--- | :--- | :--- |
| **`ADD DOWNLOAD`** | `460 × 216` (expands dynamically for duplicate banner or `SAVE TO ▾` dropdown) | File title + `HTTP 206` / `HTTP 200` size badge, truncated final URL, `FILE` row with extension lock toggle, `SAVE TO` row with integrated **Directory Dropdown** (`Default Folder` + verified `Recent Locations`) and native folder picker button, plus an optional calm inline **Duplicate URL** notice (`SHOW` / `RESUME` / `RETRY` / `OPEN FILE`). |
| **`OPTIONS`** | `460 × 272` | Unified `N PARALLEL SEGMENTS` + per-segment chunk size + `- / N / +` worker stepper above the `8px` **Graduated Segment Ruler**, followed by flat `COOKIE`, `REFERER`, and `AGENT` request header inputs (with `cURL` badge when pre-populated). |

---

## 3. Native Windows Folder Picker, Integrated Dropdown & Duplicate Guards

- **Integrated `SAVE TO` Dropdown (`0011_folder_history.sql`):** Confirmed save folders and `Move To...` destinations are normalized (trimming trailing separators, deduplicating case-insensitively on Windows, and pruning missing directories on read, capped to 10) and exposed directly via the `▾` dropdown inside `SAVE TO` (`Default` + verified `Recent Locations`). Both **Pre-Flight (`SAVE TO`)**, **`Move To...`**, and **Settings → Downloads (`Default Save Folder` → `BROWSE`)** use the adjacent folder button (`cx.prompt_for_paths`) to add/pick another folder and return focus via `window.activate_window()`.
- **Ongoing Duplicate & `.part` Path Safeguards:** `find_duplicate_download` checks `#fragment`-stripped URLs first and falls back to canonical filename + `content_length` (ignoring ` (N)` disambiguation counters such as ` (1)`) to catch rotating-token CDN URLs and auto-renamed copies, ranking `Active`/`Probing`/`Queued`/`Paused`/`Failed` entries ahead of older `Completed` records. Before queueing, `Pre-Flight` runs a live conflict check (`ConfirmAction::DuplicateOngoing`) so clicking `DOWNLOAD NOW` / `ADD PAUSED` while the same URL or canonical file is ongoing or paused offers **`SHOW ONGOING` / `RESUME EXISTING`** vs. **`DOWNLOAD SEPARATE COPY`** (`disambiguate_output_path_with_reserved`), and `DownloadEngine` reserves claimed DB `output_path`s across `Queued`/`Probing`/`Active`/`Paused` entries so concurrent copies never overwrite or share a `.part` file.

---

## 4. File Extension Lock (`0007_lock_file_extension.sql`)

- **Behavior (`split_filename_extension`):** When enabled, `filename_input` holds only the filename stem while the trailing `pf-ext-lock` cell displays and locks the probed extension (e.g. `.RAR`), re-attaching it on confirmation so renaming cannot accidentally remove the extension.
- **Per-Download Override:** Clicking the extension badge (`pf-ext-lock`) toggles between locked stem-only editing and unlocked full-filename editing for that download.
- **Persisted Default:** Controlled by `Settings::ui_lock_file_extension` (default `true`, persisted via `0007_lock_file_extension.sql`) under **Settings → Downloads → Storage & Pre-Flight → Lock File Extension in Pre-Flight**.
