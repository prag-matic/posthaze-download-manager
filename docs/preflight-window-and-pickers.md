# Independent Pre-Flight Window, Folder Picker & Extension Lock

Posthaze's download probing and confirmation flow runs in its own standalone, auto-activating GPUI window (`PreflightWindow` in `crates/posthaze-ui/src/preflight_window.rs`) rather than an in-app modal overlay.

---

## 1. Standalone `PreflightWindow`

- **Foreground Auto-Activation:** Opened with `WindowKind::Normal` and `TitleBar::window_options()`, followed by `window.activate_window()`, so browser-captured downloads (`posthaze-ui --enqueue-url` over Windows named-pipe IPC) pop directly to the foreground even when the main Posthaze window is minimized or unfocused.
- **Concurrent Probe Isolation:** Each probe request spawns its own `PreflightWindow` cascaded by `+28px` X/Y (`cascade_idx % 6`), preventing back-to-back browser captures from overwriting or dropping an in-flight probe.
- **Probing & Error States:**
  - `Probing` renders a `3px` sweeping selenium-amber shutter bar (`preflight-shutter-sweep`) and `HEAD / RANGE` indicator.
  - `Error` surfaces the failure reason, optional `403` cURL hint, **EDIT URL** (populates the main window's URL bar and activates it), **CLOSE**, and **RETRY**.

---

## 2. Titlebar Tabs (`ADD DOWNLOAD` & `OPTIONS`)

When a probe resolves (`PreflightModalState::Resolved`), the `TitleBar` exposes two compact tabs with dynamic window resizing:

| Tab | Window Size | Contents |
| :--- | :--- | :--- |
| **`ADD DOWNLOAD`** | `460 × 198` | File title + `HTTP 206` / `HTTP 200` size badge, truncated final URL, `FILE` row with extension lock toggle, and `SAVE TO` row with native folder picker. |
| **`OPTIONS`** | `460 × 264` | Unified `N PARALLEL SEGMENTS` + per-segment chunk size + `- / N / +` worker stepper above the `8px` **Graduated Segment Ruler**, followed by flat `COOKIE`, `REFERER`, and `AGENT` request header inputs (with `cURL` badge when pre-populated). |

---

## 3. Native Windows Folder Picker

Both **Pre-Flight (`SAVE TO`)** and **Settings → General (`Save Folder` → `BROWSE`)** use GPUI's built-in `cx.prompt_for_paths(PathPromptOptions { files: false, directories: true, .. })` dialog—requiring no external file-dialog crate—and return focus to the owning window via `window.activate_window()`.

---

## 4. File Extension Lock (`0007_lock_file_extension.sql`)

- **Behavior (`split_filename_extension`):** When enabled, `filename_input` holds only the filename stem while the trailing `pf-ext-lock` cell displays and locks the probed extension (e.g. `.RAR`), re-attaching it on confirmation so renaming cannot accidentally remove the extension.
- **Per-Download Override:** Clicking the extension badge (`pf-ext-lock`) toggles between locked stem-only editing and unlocked full-filename editing for that download.
- **Persisted Default:** Controlled by `Settings::ui_lock_file_extension` (default `true`, persisted via `0007_lock_file_extension.sql`) under **Settings → Interface → Pre-Flight & Interactions → Lock File Extension in Pre-Flight**.
