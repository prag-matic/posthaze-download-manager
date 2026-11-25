//! Comprehensive multi-tab Settings modal (`General`, `Downloads`, `Network`, `Automation`, `Interface`, `Reset`).

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::InputState;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{
    CollisionPolicy, DeleteDefaultAction, DownloadCategory, GENERAL_CATEGORY_ID, ProxyMode,
    Settings, TableDensity, default_builtin_categories, effective_category_dir,
    find_extension_conflicts, normalize_extension_list, normalize_pattern_list, resolve_category,
};

use crate::grid_cells::{
    BadgeTone, CellBorder, GridTone, NavRowTone, grid_badge, grid_boxed_button, grid_cell_button,
    grid_cell_button_dyn, grid_input_box, grid_kv_row_with_bg, grid_modal_footer, grid_nav_row,
    grid_progress_track, grid_section_header, grid_selection_bar,
};
use crate::modals::labeled_input;
use crate::theme::*;
use crate::updater::UpdateState;

/// Width of the left navigation sidebar in the standalone Settings window (shared with `TitleBar`
/// so the vertical hairline divider runs cleanly from titlebar into sidebar).
pub(crate) const SETTINGS_SIDEBAR_WIDTH: f32 = 176.0;

/// Navigation tabs inside the Settings window (`General`, `Downloads`, `Network`, `Appearance`,
/// `Integrations`, `System`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    General,
    Downloads,
    Network,
    Appearance,
    Integrations,
    System,
}

impl SettingsTab {
    pub const ALL: [Self; 6] = [
        Self::General,
        Self::Downloads,
        Self::Network,
        Self::Appearance,
        Self::Integrations,
        Self::System,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Downloads => "Downloads",
            Self::Network => "Network",
            Self::Appearance => "Appearance",
            Self::Integrations => "Integrations",
            Self::System => "System",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::General => "Startup behavior, System Tray, and Windows notifications",
            Self::Downloads => "Save location, Pre-Flight defaults, collision policy, and safety",
            Self::Network => "Proxy routing, global bandwidth cap, and connection limits",
            Self::Appearance => "Color themes, accents, table columns, scale, and window chrome",
            Self::Integrations => "Browser Capture extension host and post-download command hooks",
            Self::System => "Application updates, diagnostics, folder history, and factory reset",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Settings,
            Self::Downloads => IconName::Download,
            Self::Network => IconName::Globe,
            Self::Appearance => IconName::LayoutDashboard,
            Self::Integrations => IconName::ShieldCheck,
            Self::System => IconName::RefreshCw,
        }
    }
}

/// Pure, headless form draft for [`Settings`] validation without GPUI dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SettingsDraft {
    pub dir: String,
    pub workers: String,
    pub concurrent: String,
    pub proxy_url: String,
    pub speed_limit: String,
    pub auto_retry: String,
    pub cmd_path: String,
    pub cmd_args: String,
    pub sync_on_complete: bool,
    pub proxy_mode: ProxyMode,
    pub auto_categorize_folders: bool,
    pub category_url_rule_precedence: bool,
    pub set_file_time_from_server: bool,
    pub collision_policy: CollisionPolicy,
    pub delete_action_default: DeleteDefaultAction,
    pub completion_cmd_enabled: bool,
    pub notify_on_added: bool,
    pub notify_on_started: bool,
    pub notify_on_completed: bool,
    pub notify_on_failed: bool,
    pub notify_only_inactive: bool,
    pub ui_click_row_opens_inspector: bool,
    pub ui_show_add_paused: bool,
    pub ui_zoom_percent: u16,
    pub ui_theme: String,
    pub ui_table_density: TableDensity,
    pub ui_show_speed_col: bool,
    pub ui_show_eta_col: bool,
    pub ui_show_segs_col: bool,
    pub ui_show_toolbar_labels: bool,
    pub ui_show_scrollbar_minimap: bool,
    pub ui_show_titlebar_telemetry: bool,
    pub ui_start_sidebar_collapsed: bool,
    pub ui_accent_override: Option<String>,
    pub ui_lock_file_extension: bool,
    pub ui_close_to_tray: bool,
    pub launch_on_startup: bool,
    pub auto_check_updates: bool,
}

impl SettingsDraft {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            dir: settings.default_output_dir.to_string_lossy().to_string(),
            workers: settings.max_workers.to_string(),
            concurrent: settings.max_concurrent_downloads.to_string(),
            proxy_url: settings.proxy_url.clone(),
            speed_limit: settings.download_speed_limit_kib.to_string(),
            auto_retry: settings.auto_retry_count.to_string(),
            cmd_path: settings.completion_cmd_path.clone(),
            cmd_args: settings.completion_cmd_args.clone(),
            sync_on_complete: settings.sync_on_complete,
            proxy_mode: settings.proxy_mode,
            auto_categorize_folders: settings.auto_categorize_folders,
            category_url_rule_precedence: settings.category_url_rule_precedence,
            set_file_time_from_server: settings.set_file_time_from_server,
            collision_policy: settings.collision_policy,
            delete_action_default: settings.delete_action_default,
            completion_cmd_enabled: settings.completion_cmd_enabled,
            notify_on_added: settings.notify_on_added,
            notify_on_started: settings.notify_on_started,
            notify_on_completed: settings.notify_on_completed,
            notify_on_failed: settings.notify_on_failed,
            notify_only_inactive: settings.notify_only_inactive,
            ui_click_row_opens_inspector: settings.ui_click_row_opens_inspector,
            ui_show_add_paused: settings.ui_show_add_paused,
            ui_zoom_percent: settings.ui_zoom_percent,
            ui_theme: settings.ui_theme.clone(),
            ui_table_density: settings.ui_table_density,
            ui_show_speed_col: settings.ui_show_speed_col,
            ui_show_eta_col: settings.ui_show_eta_col,
            ui_show_segs_col: settings.ui_show_segs_col,
            ui_show_toolbar_labels: settings.ui_show_toolbar_labels,
            ui_show_scrollbar_minimap: settings.ui_show_scrollbar_minimap,
            ui_show_titlebar_telemetry: settings.ui_show_titlebar_telemetry,
            ui_start_sidebar_collapsed: settings.ui_start_sidebar_collapsed,
            ui_accent_override: settings.ui_accent_override.clone(),
            ui_lock_file_extension: settings.ui_lock_file_extension,
            ui_close_to_tray: settings.ui_close_to_tray,
            launch_on_startup: settings.launch_on_startup,
            auto_check_updates: settings.auto_check_updates,
        }
    }

    pub fn into_settings(self) -> Result<Settings, String> {
        let dir = self.dir.trim().to_string();
        if dir.is_empty() {
            return Err("Save folder cannot be empty.".to_string());
        }
        let max_workers = self
            .workers
            .trim()
            .parse::<usize>()
            .map_err(|_| "Workers per download must be an integer between 1 and 32.".to_string())?;
        if !(1..=32).contains(&max_workers) {
            return Err("Workers per download must be between 1 and 32.".to_string());
        }
        let max_concurrent_downloads = self
            .concurrent
            .trim()
            .parse::<usize>()
            .map_err(|_| "Parallel downloads must be at least 1.".to_string())?;
        let download_speed_limit_kib = self
            .speed_limit
            .trim()
            .parse::<u64>()
            .map_err(|_| "Speed limit must be a non-negative integer (KiB/s).".to_string())?;
        let auto_retry_count = self
            .auto_retry
            .trim()
            .parse::<u32>()
            .map_err(|_| "Auto-retry count must be between 0 and 10.".to_string())?;
        if auto_retry_count > 10 {
            return Err("Auto-retry count cannot exceed 10.".to_string());
        }

        let completion_cmd_path = self.cmd_path.trim().to_string();
        let completion_cmd_args = self.cmd_args.trim().to_string();
        if self.completion_cmd_enabled && completion_cmd_path.is_empty() {
            return Err("Program path is required when post-download command is enabled.".to_string());
        }

        let settings = Settings {
            default_output_dir: PathBuf::from(dir),
            max_workers,
            max_concurrent_downloads,
            sync_on_complete: self.sync_on_complete,
            proxy_mode: self.proxy_mode,
            proxy_url: self.proxy_url.trim().to_string(),
            download_speed_limit_kib,
            auto_categorize_folders: self.auto_categorize_folders,
            category_url_rule_precedence: self.category_url_rule_precedence,
            set_file_time_from_server: self.set_file_time_from_server,
            collision_policy: self.collision_policy,
            delete_action_default: self.delete_action_default,
            auto_retry_count,
            completion_cmd_enabled: self.completion_cmd_enabled,
            completion_cmd_path,
            completion_cmd_args: if completion_cmd_args.is_empty() {
                "%path%".to_string()
            } else {
                completion_cmd_args
            },
            notify_on_added: self.notify_on_added,
            notify_on_started: self.notify_on_started,
            notify_on_completed: self.notify_on_completed,
            notify_on_failed: self.notify_on_failed,
            notify_only_inactive: self.notify_only_inactive,
            ui_click_row_opens_inspector: self.ui_click_row_opens_inspector,
            ui_show_add_paused: self.ui_show_add_paused,
            ui_zoom_percent: self.ui_zoom_percent,
            ui_theme: self.ui_theme,
            ui_table_density: self.ui_table_density,
            ui_show_speed_col: self.ui_show_speed_col,
            ui_show_eta_col: self.ui_show_eta_col,
            ui_show_segs_col: self.ui_show_segs_col,
            ui_show_toolbar_labels: self.ui_show_toolbar_labels,
            ui_show_scrollbar_minimap: self.ui_show_scrollbar_minimap,
            ui_show_titlebar_telemetry: self.ui_show_titlebar_telemetry,
            ui_start_sidebar_collapsed: self.ui_start_sidebar_collapsed,
            ui_accent_override: self.ui_accent_override,
            ui_lock_file_extension: self.ui_lock_file_extension,
            ui_close_to_tray: self.ui_close_to_tray,
            launch_on_startup: self.launch_on_startup,
            auto_check_updates: self.auto_check_updates,
        };
        settings.validate().map_err(|e| e.to_string())?;
        Ok(settings)
    }
}

/// Interactive state for the comprehensive Settings modal.
pub(crate) struct SettingsModalState {
    pub active_tab: SettingsTab,
    pub dir_input: Entity<InputState>,
    pub workers_input: Entity<InputState>,
    pub concurrent_input: Entity<InputState>,
    pub proxy_url_input: Entity<InputState>,
    pub speed_limit_input: Entity<InputState>,
    pub auto_retry_input: Entity<InputState>,
    pub cmd_path_input: Entity<InputState>,
    pub cmd_args_input: Entity<InputState>,
    pub extension_id_input: Entity<InputState>,
    pub cat_name_input: Entity<InputState>,
    pub cat_extensions_input: Entity<InputState>,
    pub cat_patterns_input: Entity<InputState>,
    pub cat_dir_input: Entity<InputState>,
    pub cat_filter_input: Entity<InputState>,
    pub cat_rule_test_input: Entity<InputState>,
    pub categories: Vec<DownloadCategory>,
    pub selected_category_id: String,
    pub cat_picker_open: bool,
    pub browser_statuses: Vec<(&'static str, bool)>,
    pub integration_feedback: Option<Result<String, String>>,
    pub system_feedback: Option<Result<String, String>>,
    pub draft: SettingsDraft,
    pub error: Option<String>,
}

impl SettingsModalState {
    pub fn from_settings(
        settings: &Settings,
        categories: Vec<DownloadCategory>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let draft = SettingsDraft::from_settings(settings);
        let categories = if categories.is_empty() {
            default_builtin_categories()
        } else {
            categories
        };
        let selected_category_id = categories
            .iter()
            .find(|c| c.id == GENERAL_CATEGORY_ID)
            .or_else(|| categories.first())
            .map(|c| c.id.clone())
            .unwrap_or_else(|| GENERAL_CATEGORY_ID.to_string());
        let initial_cat = categories
            .iter()
            .find(|c| c.id == selected_category_id)
            .cloned();
        let dir_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("Default save folder");
            st.set_value(draft.dir.clone(), window, cx);
            st
        });
        let workers_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("1–32");
            st.set_value(draft.workers.clone(), window, cx);
            st
        });
        let concurrent_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("1+");
            st.set_value(draft.concurrent.clone(), window, cx);
            st
        });
        let proxy_url_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx)
                .placeholder("http://127.0.0.1:8080 or socks5://127.0.0.1:1080");
            st.set_value(draft.proxy_url.clone(), window, cx);
            st
        });
        let speed_limit_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("0");
            st.set_value(draft.speed_limit.clone(), window, cx);
            st
        });
        let auto_retry_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("0–10");
            st.set_value(draft.auto_retry.clone(), window, cx);
            st
        });
        let cmd_path_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("C:\\Path\\To\\Program.exe");
            st.set_value(draft.cmd_path.clone(), window, cx);
            st
        });
        let cmd_args_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("%path%");
            st.set_value(draft.cmd_args.clone(), window, cx);
            st
        });
        let extension_id_input = cx.new(|cx| {
            let mut st =
                InputState::new(window, cx).placeholder("32-char unpacked/store Extension ID");
            st.set_value(
                crate::browser_integration::DEFAULT_UNPACKED_EXTENSION_ID,
                window,
                cx,
            );
            st
        });
        let cat_name_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("Category name");
            if let Some(ref c) = initial_cat {
                st.set_value(c.name.clone(), window, cx);
            }
            st
        });
        let cat_extensions_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Add type (Enter or ,)...")
        });
        let cat_patterns_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Add host/URL (Enter or ,)...")
        });
        let cat_dir_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("Default subfolder under save directory");
            if let Some(ref c) = initial_cat
                && let Some(ref dir) = c.output_dir
            {
                st.set_value(dir.to_string_lossy().to_string(), window, cx);
            }
            st
        });
        let cat_filter_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Filter categories")
        });
        let cat_rule_test_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Test filename or URL (e.g. ubuntu-24.04.iso)")
        });
        let browser_statuses = crate::browser_integration::query_browser_statuses();

        Self {
            active_tab: SettingsTab::General,
            dir_input,
            workers_input,
            concurrent_input,
            proxy_url_input,
            speed_limit_input,
            auto_retry_input,
            cmd_path_input,
            cmd_args_input,
            extension_id_input,
            cat_name_input,
            cat_extensions_input,
            cat_patterns_input,
            cat_dir_input,
            cat_filter_input,
            cat_rule_test_input,
            categories,
            selected_category_id,
            cat_picker_open: false,
            browser_statuses,
            integration_feedback: None,
            system_feedback: None,
            draft,
            error: None,
        }
    }

    /// Commits typed tokens from `cat_extensions_input` as chips on the active category.
    pub fn commit_extension_input(&mut self, window: &mut Window, cx: &mut App) {
        let raw = self.cat_extensions_input.read(cx).value().to_string();
        let additions = normalize_extension_list(&raw);
        if !additions.is_empty()
            && self.selected_category_id != GENERAL_CATEGORY_ID
            && let Some(cat) = self
                .categories
                .iter_mut()
                .find(|c| c.id == self.selected_category_id)
        {
            merge_unique_tokens(&mut cat.extensions, additions);
        }
        if !raw.is_empty() {
            self.cat_extensions_input.update(cx, |st, cx| {
                st.set_value("", window, cx);
            });
        }
    }

    /// Commits typed tokens from `cat_patterns_input` as chips on the active category.
    pub fn commit_pattern_input(&mut self, window: &mut Window, cx: &mut App) {
        let raw = self.cat_patterns_input.read(cx).value().to_string();
        let additions = normalize_pattern_list(&raw);
        if !additions.is_empty()
            && self.selected_category_id != GENERAL_CATEGORY_ID
            && let Some(cat) = self
                .categories
                .iter_mut()
                .find(|c| c.id == self.selected_category_id)
        {
            merge_unique_tokens(&mut cat.url_patterns, additions);
        }
        if !raw.is_empty() {
            self.cat_patterns_input.update(cx, |st, cx| {
                st.set_value("", window, cx);
            });
        }
    }

    /// Flushes edits from the active category text inputs back into `self.categories`.
    pub fn sync_selected_category_from_inputs(&mut self, cx: &App) {
        let Some(cat) = self
            .categories
            .iter_mut()
            .find(|c| c.id == self.selected_category_id)
        else {
            return;
        };
        let is_general = cat.id == GENERAL_CATEGORY_ID;
        if !is_general {
            let name = self.cat_name_input.read(cx).value().trim().to_string();
            if !name.is_empty() {
                cat.name = name;
            }
            merge_unique_tokens(
                &mut cat.extensions,
                normalize_extension_list(&self.cat_extensions_input.read(cx).value()),
            );
            merge_unique_tokens(
                &mut cat.url_patterns,
                normalize_pattern_list(&self.cat_patterns_input.read(cx).value()),
            );
        }
        let raw_dir = self.cat_dir_input.read(cx).value().trim().to_string();
        cat.output_dir = (!raw_dir.is_empty()).then(|| PathBuf::from(raw_dir));
    }

    /// Loads `self.selected_category_id` fields into the category text inputs.
    pub fn load_selected_category_into_inputs(&self, window: &mut Window, cx: &mut App) {
        let Some(cat) = self
            .categories
            .iter()
            .find(|c| c.id == self.selected_category_id)
        else {
            return;
        };
        let name = cat.name.clone();
        let dir_str = cat
            .output_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        self.cat_name_input.update(cx, |st, cx| {
            st.set_value(name, window, cx);
        });
        self.cat_extensions_input.update(cx, |st, cx| {
            st.set_value("", window, cx);
        });
        self.cat_patterns_input.update(cx, |st, cx| {
            st.set_value("", window, cx);
        });
        self.cat_dir_input.update(cx, |st, cx| {
            st.set_value(dir_str, window, cx);
        });
    }

    /// Returns a live preview snapshot of `self.categories` incorporating unsaved input edits.
    pub fn live_categories_snapshot(&self, cx: &App) -> Vec<DownloadCategory> {
        let mut copy = self.categories.clone();
        if let Some(cat) = copy.iter_mut().find(|c| c.id == self.selected_category_id) {
            let is_general = cat.id == GENERAL_CATEGORY_ID;
            if !is_general {
                let name = self.cat_name_input.read(cx).value().trim().to_string();
                if !name.is_empty() {
                    cat.name = name;
                }
                merge_unique_tokens(
                    &mut cat.extensions,
                    normalize_extension_list(&self.cat_extensions_input.read(cx).value()),
                );
                merge_unique_tokens(
                    &mut cat.url_patterns,
                    normalize_pattern_list(&self.cat_patterns_input.read(cx).value()),
                );
            }
            let raw_dir = self.cat_dir_input.read(cx).value().trim().to_string();
            cat.output_dir = (!raw_dir.is_empty()).then(|| PathBuf::from(raw_dir));
        }
        copy
    }

    /// Validates and returns the category list for persistence.
    pub fn apply_categories(&mut self, cx: &App) -> Result<Vec<DownloadCategory>, String> {
        self.sync_selected_category_from_inputs(cx);
        for cat in &self.categories {
            if cat.name.trim().is_empty() {
                return Err("Category name cannot be empty.".to_string());
            }
        }
        if self.selected_category_id != GENERAL_CATEGORY_ID
            && self.cat_name_input.read(cx).value().trim().is_empty()
        {
            return Err("Category name cannot be empty.".to_string());
        }
        Ok(self.categories.clone())
    }

    pub fn handle_action(&mut self, action: SettingsAction, window: &mut Window, cx: &mut App) {
        match action {
            SettingsAction::SelectTab(tab) => self.active_tab = tab,
            SettingsAction::SetProxyMode(mode) => self.draft.proxy_mode = mode,
            SettingsAction::SetSpeedPreset(kib) => {
                self.speed_limit_input.update(cx, |st, cx| {
                    st.set_value(kib.to_string(), window, cx);
                });
            }
            SettingsAction::SetCollisionPolicy(policy) => self.draft.collision_policy = policy,
            SettingsAction::SetDeleteDefault(def) => self.draft.delete_action_default = def,
            SettingsAction::SetZoom(zoom) => self.draft.ui_zoom_percent = zoom,
            SettingsAction::ToggleSyncOnComplete => {
                self.draft.sync_on_complete = !self.draft.sync_on_complete;
            }
            SettingsAction::ToggleAutoCategorize => {
                self.draft.auto_categorize_folders = !self.draft.auto_categorize_folders;
            }
            SettingsAction::ToggleCategoryUrlPrecedence => {
                self.draft.category_url_rule_precedence = !self.draft.category_url_rule_precedence;
            }
            SettingsAction::ToggleSetFileTimeFromServer => {
                self.draft.set_file_time_from_server = !self.draft.set_file_time_from_server;
            }
            SettingsAction::ToggleCategoryPicker => {
                self.sync_selected_category_from_inputs(cx);
                self.cat_picker_open = !self.cat_picker_open;
            }
            SettingsAction::SelectCategory(id) => {
                self.sync_selected_category_from_inputs(cx);
                if self.categories.iter().any(|c| c.id == id) {
                    self.selected_category_id = id;
                    self.load_selected_category_into_inputs(window, cx);
                }
                self.cat_picker_open = false;
            }
            SettingsAction::NewCategory => {
                self.sync_selected_category_from_inputs(cx);
                let mut idx = self.categories.len();
                let (id, name) = loop {
                    idx += 1;
                    let cand_id = format!("custom-{idx}");
                    let cand_name = format!("Custom {idx}");
                    if !self
                        .categories
                        .iter()
                        .any(|c| c.id == cand_id || c.name.eq_ignore_ascii_case(&cand_name))
                    {
                        break (cand_id, cand_name);
                    }
                };
                let next_order = self
                    .categories
                    .iter()
                    .filter(|c| c.id != GENERAL_CATEGORY_ID)
                    .map(|c| c.sort_order)
                    .max()
                    .unwrap_or(0)
                    + 10;
                let new_cat = DownloadCategory {
                    id: id.clone(),
                    name,
                    extensions: Vec::new(),
                    url_patterns: Vec::new(),
                    output_dir: None,
                    remember_last_folder: false,
                    is_builtin: false,
                    sort_order: next_order.min(9990),
                };
                let insert_pos = self
                    .categories
                    .iter()
                    .position(|c| c.id == GENERAL_CATEGORY_ID)
                    .unwrap_or(self.categories.len());
                self.categories.insert(insert_pos, new_cat);
                self.selected_category_id = id;
                self.cat_picker_open = false;
                self.cat_filter_input.update(cx, |st, cx| {
                    st.set_value("", window, cx);
                });
                self.load_selected_category_into_inputs(window, cx);
            }
            SettingsAction::DeleteSelectedCategory => {
                if self.selected_category_id != GENERAL_CATEGORY_ID
                    && self
                        .categories
                        .iter()
                        .any(|c| c.id == self.selected_category_id && !c.is_builtin)
                {
                    self.categories.retain(|c| c.id != self.selected_category_id);
                    self.selected_category_id = GENERAL_CATEGORY_ID.to_string();
                    self.cat_picker_open = false;
                    self.load_selected_category_into_inputs(window, cx);
                }
            }
            SettingsAction::ResetDefaultCategories => {
                self.categories = default_builtin_categories();
                self.selected_category_id = GENERAL_CATEGORY_ID.to_string();
                self.cat_picker_open = false;
                self.cat_filter_input.update(cx, |st, cx| {
                    st.set_value("", window, cx);
                });
                self.load_selected_category_into_inputs(window, cx);
            }
            SettingsAction::ToggleCategoryRememberFolder => {
                self.sync_selected_category_from_inputs(cx);
                if let Some(cat) = self
                    .categories
                    .iter_mut()
                    .find(|c| c.id == self.selected_category_id)
                {
                    cat.remember_last_folder = !cat.remember_last_folder;
                }
            }
            SettingsAction::ClearCategoryFolder => {
                self.cat_dir_input.update(cx, |st, cx| {
                    st.set_value("", window, cx);
                });
                if let Some(cat) = self
                    .categories
                    .iter_mut()
                    .find(|c| c.id == self.selected_category_id)
                {
                    cat.output_dir = None;
                }
            }
            SettingsAction::AddCategoryExtension => {
                self.commit_extension_input(window, cx);
            }
            SettingsAction::RemoveCategoryExtension(ext) => {
                if self.selected_category_id != GENERAL_CATEGORY_ID
                    && let Some(cat) = self
                        .categories
                        .iter_mut()
                        .find(|c| c.id == self.selected_category_id)
                {
                    cat.extensions.retain(|e| e != &ext);
                }
            }
            SettingsAction::AddCategoryPattern => {
                self.commit_pattern_input(window, cx);
            }
            SettingsAction::RemoveCategoryPattern(pat) => {
                if self.selected_category_id != GENERAL_CATEGORY_ID
                    && let Some(cat) = self
                        .categories
                        .iter_mut()
                        .find(|c| c.id == self.selected_category_id)
                {
                    cat.url_patterns.retain(|p| p != &pat);
                }
            }
            SettingsAction::BrowseCategoryFolder => {}
            SettingsAction::ToggleCompletionCmd => {
                self.draft.completion_cmd_enabled = !self.draft.completion_cmd_enabled;
            }
            SettingsAction::ApplyDefenderPreset => {
                self.draft.completion_cmd_enabled = true;
                self.cmd_path_input.update(cx, |st, cx| {
                    st.set_value(
                        r"C:\Program Files\Windows Defender\MpCmdRun.exe".to_string(),
                        window,
                        cx,
                    );
                });
                self.cmd_args_input.update(cx, |st, cx| {
                    st.set_value("-Scan -ScanType 3 -File \"%path%\"".to_string(), window, cx);
                });
            }
            SettingsAction::ToggleNotifyAdded => {
                self.draft.notify_on_added = !self.draft.notify_on_added;
            }
            SettingsAction::ToggleNotifyStarted => {
                self.draft.notify_on_started = !self.draft.notify_on_started;
            }
            SettingsAction::ToggleNotifyCompleted => {
                self.draft.notify_on_completed = !self.draft.notify_on_completed;
            }
            SettingsAction::ToggleNotifyFailed => {
                self.draft.notify_on_failed = !self.draft.notify_on_failed;
            }
            SettingsAction::ToggleNotifyOnlyInactive => {
                self.draft.notify_only_inactive = !self.draft.notify_only_inactive;
            }
            SettingsAction::ToggleClickOpensInspector => {
                self.draft.ui_click_row_opens_inspector = !self.draft.ui_click_row_opens_inspector;
            }
            SettingsAction::ToggleShowAddPaused => {
                self.draft.ui_show_add_paused = !self.draft.ui_show_add_paused;
            }
            SettingsAction::ToggleLockFileExtension => {
                self.draft.ui_lock_file_extension = !self.draft.ui_lock_file_extension;
            }
            SettingsAction::ToggleCloseToTray => {
                self.draft.ui_close_to_tray = !self.draft.ui_close_to_tray;
            }
            SettingsAction::ToggleLaunchOnStartup => {
                self.draft.launch_on_startup = !self.draft.launch_on_startup;
            }
            SettingsAction::ToggleAutoCheckUpdates => {
                self.draft.auto_check_updates = !self.draft.auto_check_updates;
            }
            SettingsAction::CheckForUpdates
            | SettingsAction::InstallUpdate
            | SettingsAction::OpenReleaseNotes(_) => {}
            SettingsAction::SetTableDensity(density) => {
                self.draft.ui_table_density = density;
            }
            SettingsAction::ToggleSpeedCol => {
                self.draft.ui_show_speed_col = !self.draft.ui_show_speed_col;
            }
            SettingsAction::ToggleEtaCol => {
                self.draft.ui_show_eta_col = !self.draft.ui_show_eta_col;
            }
            SettingsAction::ToggleSegsCol => {
                self.draft.ui_show_segs_col = !self.draft.ui_show_segs_col;
            }
            SettingsAction::ToggleToolbarLabels => {
                self.draft.ui_show_toolbar_labels = !self.draft.ui_show_toolbar_labels;
            }
            SettingsAction::ToggleScrollbarMinimap => {
                self.draft.ui_show_scrollbar_minimap = !self.draft.ui_show_scrollbar_minimap;
            }
            SettingsAction::ToggleTitlebarTelemetry => {
                self.draft.ui_show_titlebar_telemetry = !self.draft.ui_show_titlebar_telemetry;
            }
            SettingsAction::ToggleStartSidebarCollapsed => {
                self.draft.ui_start_sidebar_collapsed = !self.draft.ui_start_sidebar_collapsed;
            }
            SettingsAction::BrowseSaveFolder => {}
            SettingsAction::SetAccentOverride(accent) => {
                self.draft.ui_accent_override = accent;
                crate::sync_theme_and_backdrops_in_window(
                    &self.draft.ui_theme,
                    self.draft.ui_accent_override.as_deref(),
                    window,
                    cx,
                );
            }
            SettingsAction::SetTheme(theme_id) => {
                let preset = crate::theme::find_theme_preset(&theme_id);
                self.draft.ui_theme = preset.id.to_string();
                crate::sync_theme_and_backdrops_in_window(
                    &self.draft.ui_theme,
                    self.draft.ui_accent_override.as_deref(),
                    window,
                    cx,
                );
            }
            SettingsAction::CopyConfigJson => {
                if let Ok(settings) = self.apply_to_settings(cx) {
                    let payload = serde_json::json!({
                        "default_output_dir": settings.default_output_dir,
                        "max_workers": settings.max_workers,
                        "max_concurrent_downloads": settings.max_concurrent_downloads,
                        "sync_on_complete": settings.sync_on_complete,
                        "proxy_mode": format!("{:?}", settings.proxy_mode),
                        "proxy_url": settings.proxy_url,
                        "download_speed_limit_kib": settings.download_speed_limit_kib,
                        "auto_categorize_folders": settings.auto_categorize_folders,
                        "category_url_rule_precedence": settings.category_url_rule_precedence,
                        "set_file_time_from_server": settings.set_file_time_from_server,
                        "collision_policy": format!("{:?}", settings.collision_policy),
                        "delete_action_default": format!("{:?}", settings.delete_action_default),
                        "auto_retry_count": settings.auto_retry_count,
                        "completion_cmd_enabled": settings.completion_cmd_enabled,
                        "completion_cmd_path": settings.completion_cmd_path,
                        "completion_cmd_args": settings.completion_cmd_args,
                        "ui_theme": settings.ui_theme,
                        "ui_accent_override": settings.ui_accent_override,
                        "ui_zoom_percent": settings.ui_zoom_percent,
                        "ui_table_density": settings.ui_table_density.as_str(),
                        "ui_show_speed_col": settings.ui_show_speed_col,
                        "ui_show_eta_col": settings.ui_show_eta_col,
                        "ui_show_segs_col": settings.ui_show_segs_col,
                        "ui_show_toolbar_labels": settings.ui_show_toolbar_labels,
                        "ui_show_scrollbar_minimap": settings.ui_show_scrollbar_minimap,
                        "ui_show_titlebar_telemetry": settings.ui_show_titlebar_telemetry,
                        "ui_start_sidebar_collapsed": settings.ui_start_sidebar_collapsed,
                        "ui_lock_file_extension": settings.ui_lock_file_extension,
                        "ui_close_to_tray": settings.ui_close_to_tray,
                        "launch_on_startup": settings.launch_on_startup,
                        "auto_check_updates": settings.auto_check_updates,
                    });
                    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
                        cx.write_to_clipboard(ClipboardItem::new_string(pretty));
                        self.system_feedback =
                            Some(Ok("Configuration JSON copied to clipboard.".to_string()));
                    }
                } else {
                    self.system_feedback = Some(Err(
                        "Fix invalid input fields before exporting configuration JSON.".to_string(),
                    ));
                }
            }
            SettingsAction::ResetToDefaults => {
                *self = Self::from_settings(
                    &Settings::default(),
                    default_builtin_categories(),
                    window,
                    cx,
                );
                self.active_tab = SettingsTab::System;
                self.system_feedback = Some(Ok(
                    "Restored factory defaults. Click Apply or Save Settings to keep.".to_string(),
                ));
                crate::sync_theme_and_backdrops_in_window(
                    &self.draft.ui_theme,
                    self.draft.ui_accent_override.as_deref(),
                    window,
                    cx,
                );
            }
            SettingsAction::OpenLogsFolder => {}
            SettingsAction::ClearRecentFolders => {}
            SettingsAction::RegisterNativeHost => {}
            SettingsAction::RefreshBrowserStatus => {}
        }
    }

    pub fn apply_to_settings(&self, cx: &App) -> Result<Settings, String> {
        let mut draft = self.draft.clone();
        draft.dir = self.dir_input.read(cx).value().to_string();
        draft.workers = self.workers_input.read(cx).value().to_string();
        draft.concurrent = self.concurrent_input.read(cx).value().to_string();
        draft.proxy_url = self.proxy_url_input.read(cx).value().to_string();
        draft.speed_limit = self.speed_limit_input.read(cx).value().to_string();
        draft.auto_retry = self.auto_retry_input.read(cx).value().to_string();
        draft.cmd_path = self.cmd_path_input.read(cx).value().to_string();
        draft.cmd_args = self.cmd_args_input.read(cx).value().to_string();
        draft.into_settings()
    }
}

/// Actions emitted by the Settings modal back to `PosthazeApp`.
#[derive(Debug, Clone)]
pub(crate) enum SettingsAction {
    SelectTab(SettingsTab),
    SetProxyMode(ProxyMode),
    SetSpeedPreset(u64),
    SetCollisionPolicy(CollisionPolicy),
    SetDeleteDefault(DeleteDefaultAction),
    SetZoom(u16),
    SetTheme(String),
    ToggleSyncOnComplete,
    ToggleAutoCategorize,
    ToggleCategoryUrlPrecedence,
    ToggleSetFileTimeFromServer,
    ToggleCategoryPicker,
    SelectCategory(String),
    NewCategory,
    DeleteSelectedCategory,
    ResetDefaultCategories,
    ToggleCategoryRememberFolder,
    BrowseCategoryFolder,
    ClearCategoryFolder,
    AddCategoryExtension,
    RemoveCategoryExtension(String),
    AddCategoryPattern,
    RemoveCategoryPattern(String),
    ToggleCompletionCmd,
    ApplyDefenderPreset,
    ToggleNotifyAdded,
    ToggleNotifyStarted,
    ToggleNotifyCompleted,
    ToggleNotifyFailed,
    ToggleNotifyOnlyInactive,
    ToggleClickOpensInspector,
    ToggleShowAddPaused,
    ToggleLockFileExtension,
    ToggleCloseToTray,
    ToggleLaunchOnStartup,
    ToggleAutoCheckUpdates,
    CheckForUpdates,
    InstallUpdate,
    OpenReleaseNotes(String),
    SetTableDensity(TableDensity),
    ToggleSpeedCol,
    ToggleEtaCol,
    ToggleSegsCol,
    ToggleToolbarLabels,
    ToggleScrollbarMinimap,
    ToggleTitlebarTelemetry,
    ToggleStartSidebarCollapsed,
    BrowseSaveFolder,
    SetAccentOverride(Option<String>),
    CopyConfigJson,
    OpenLogsFolder,
    ClearRecentFolders,
    RegisterNativeHost,
    RefreshBrowserStatus,
    ResetToDefaults,
}

/// Renders the two-pane Settings navigation, scrollable tab content, validation error bar,
/// and `CANCEL` / `APPLY` / `SAVE SETTINGS` footer for [`SettingsWindow`](crate::settings_window::SettingsWindow).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_settings_window_body<F1, F2, F3, F4>(
    state: &SettingsModalState,
    update_state: &UpdateState,
    is_portable: bool,
    cx: &App,
    on_action: F1,
    on_cancel: F2,
    on_apply: F3,
    on_save: F4,
) -> AnyElement
where
    F1: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
    F2: Fn(&mut Window, &mut App) + Clone + 'static,
    F3: Fn(&mut Window, &mut App) + 'static,
    F4: Fn(&mut Window, &mut App) + 'static,
{
    let body = match state.active_tab {
        SettingsTab::General => render_general_tab(state, on_action.clone()),
        SettingsTab::Downloads => render_downloads_tab(state, cx, on_action.clone()),
        SettingsTab::Network => render_network_tab(state, on_action.clone()),
        SettingsTab::Appearance => render_appearance_tab(state, on_action.clone()),
        SettingsTab::Integrations => render_integrations_tab(state, on_action.clone()),
        SettingsTab::System => render_system_tab(state, update_state, is_portable, on_action.clone()),
    };

    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .w(px(SETTINGS_SIDEBAR_WIDTH))
                        .flex_shrink_0()
                        .h_full()
                        .bg(graphite_bg())
                        .border_r_1()
                        .border_color(graphite_border())
                        .flex()
                        .flex_col()
                        .child(grid_section_header("SECTIONS", GridTone::Graphite))
                        .children(SettingsTab::ALL.into_iter().map(|tab| {
                            let active = state.active_tab == tab;
                            let act = on_action.clone();
                            grid_nav_row(
                                SharedString::from(format!("settings-tab-{}", tab.label())),
                                tab.icon(),
                                tab.label(),
                                active,
                                None,
                                NavRowTone::PaperSelection,
                                move |_, window, cx| {
                                    act(SettingsAction::SelectTab(tab), window, cx);
                                },
                            )
                        })),
                )
                .child(
                    div()
                        .id("settings-scroll-pane")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .bg(paper_bg())
                        .px_5()
                        .py_4()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(
                            div()
                                .pb_3()
                                .border_b_1()
                                .border_color(graphite_border())
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .font_family(MONO_FONT)
                                        .text_size(TYPE_HEADING)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(silver_text())
                                        .child(state.active_tab.label()),
                                )
                                .child(
                                    div()
                                        .text_size(TYPE_BODY)
                                        .text_color(silver_muted())
                                        .child(state.active_tab.subtitle()),
                                ),
                        )
                        .child(body),
                ),
        )
        .when_some(state.error.clone(), |el, err| {
            el.child(
                div()
                    .w_full()
                    .px_4()
                    .py_2()
                    .bg(errata_surface_bg())
                    .border_t_1()
                    .border_color(errata_vermilion())
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .text_color(errata_vermilion())
                    .child(err),
            )
        })
        .child(
            grid_modal_footer(GridTone::Graphite)
                .h(px(36.0))
                .child(grid_cell_button(
                    "settings-cancel",
                    Some(IconName::X),
                    Some("Cancel"),
                    None,
                    GridTone::Graphite,
                    CellBorder::Left,
                    false,
                    move |_, window, cx| on_cancel(window, cx),
                ))
                .child(grid_cell_button(
                    "settings-apply",
                    Some(IconName::Check),
                    Some("Apply"),
                    Some("Save settings and keep this window open"),
                    GridTone::Graphite,
                    CellBorder::Left,
                    false,
                    move |_, window, cx| on_apply(window, cx),
                ))
                .child(grid_cell_button(
                    "settings-save",
                    Some(IconName::Check),
                    Some("Save settings"),
                    None,
                    GridTone::Primary,
                    CellBorder::Left,
                    false,
                    move |_, window, cx| on_save(window, cx),
                )),
        )
        .into_any_element()
}

fn settings_section(title: &'static str, rows: Vec<AnyElement>) -> AnyElement {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .px_0p5()
                .font_family(MONO_FONT)
                .text_size(TYPE_HEADER)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(silver_muted())
                .child(title),
        )
        .child(
            div()
                .w_full()
                .bg(graphite_elevated().opacity(0.45))
                .border_1()
                .border_color(graphite_border())
                .flex()
                .flex_col()
                .children(rows.into_iter().enumerate().map(|(idx, row)| {
                    div()
                        .w_full()
                        .when(idx > 0, |el| el.border_t_1().border_color(graphite_border()))
                        .child(row)
                })),
        )
        .into_any_element()
}

fn setting_text(title: &'static str, description: &'static str) -> impl IntoElement {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .text_size(TYPE_BODY)
                .font_weight(FontWeight::MEDIUM)
                .text_color(silver_text())
                .child(title),
        )
        .child(
            div()
                .text_size(TYPE_BODY)
                .line_height(px(16.0))
                .text_color(silver_muted())
                .child(description),
        )
}

fn setting_row(
    title: &'static str,
    description: &'static str,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .w_full()
        .px_3p5()
        .py_2p5()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(setting_text(title, description))
        .child(div().flex_shrink_0().child(control))
        .into_any_element()
}

fn stacked_row(content: impl IntoElement) -> AnyElement {
    div()
        .w_full()
        .px_3p5()
        .py_2p5()
        .flex()
        .flex_col()
        .gap_2p5()
        .child(content)
        .into_any_element()
}

fn inline_feedback_banner(feedback: &Result<String, String>) -> AnyElement {
    let (bg, border, text_col, msg) = match feedback {
        Ok(msg) => (
            graphite_elevated(),
            halide_emerald(),
            halide_emerald(),
            msg.clone(),
        ),
        Err(err) => (
            errata_surface_bg(),
            errata_vermilion(),
            errata_vermilion(),
            err.clone(),
        ),
    };
    div()
        .w_full()
        .p_2p5()
        .bg(bg)
        .border_1()
        .border_color(border)
        .font_family(MONO_FONT)
        .text_size(TYPE_MICRO)
        .text_color(text_col)
        .child(msg)
        .into_any_element()
}

fn segmented_track() -> Div {
    div()
        .h(px(28.0))
        .bg(graphite_elevated())
        .border_t_1()
        .border_b_1()
        .border_l_1()
        .border_color(graphite_border())
        .flex()
        .items_center()
}

fn compact_input(
    state: &Entity<InputState>,
    width: Pixels,
    unit: Option<&'static str>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .child(
            div()
                .w(width)
                .h(px(30.0))
                .px_2p5()
                .bg(input_bg())
                .border_1()
                .border_color(graphite_border())
                .flex()
                .items_center()
                .child(grid_input_box(state)),
        )
        .when_some(unit, |el, u| {
            el.child(
                div()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .text_color(silver_muted())
                    .child(u),
            )
        })
}

fn toggle_row<F>(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    checked: bool,
    action: SettingsAction,
    on_action: F,
) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + 'static,
{
    toggle_row_dyn(
        id,
        title.to_string(),
        description.to_string(),
        checked,
        action,
        on_action,
    )
}

fn toggle_row_dyn<F>(
    id: &'static str,
    title: String,
    description: String,
    checked: bool,
    action: SettingsAction,
    on_action: F,
) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + 'static,
{
    div()
        .id(id)
        .w_full()
        .px_3p5()
        .py_2p5()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .cursor_pointer()
        .hover(|s| s.bg(paper_hover()))
        .on_click(move |_, window, cx| on_action(action.clone(), window, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_size(TYPE_BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(silver_text())
                        .child(title),
                )
                .child(
                    div()
                        .text_size(TYPE_BODY)
                        .line_height(px(16.0))
                        .text_color(silver_muted())
                        .child(description),
                ),
        )
        .child(
            div()
                .w(px(34.0))
                .h(px(18.0))
                .p(px(2.0))
                .rounded(px(9.0))
                .flex_shrink_0()
                .border_1()
                .border_color(if checked {
                    selenium_amber()
                } else {
                    graphite_border()
                })
                .bg(if checked {
                    selenium_amber()
                } else {
                    graphite_elevated()
                })
                .flex()
                .items_center()
                .when(checked, |el| el.justify_end())
                .child(
                    div()
                        .w(px(12.0))
                        .h(px(12.0))
                        .rounded(px(6.0))
                        .bg(if checked {
                            active_theme().accent_foreground.hsla()
                        } else {
                            silver_muted()
                        }),
                ),
        )
        .into_any_element()
}

fn choice_btn<F>(
    id: &'static str,
    label: &'static str,
    selected: bool,
    action: SettingsAction,
    on_action: F,
) -> impl IntoElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + 'static,
{
    let tone = if selected {
        GridTone::Primary
    } else {
        GridTone::Graphite
    };
    grid_cell_button(
        id,
        None,
        Some(label),
        None,
        tone,
        CellBorder::Right,
        true,
        move |_, window, cx| on_action(action.clone(), window, cx),
    )
}

fn preset_btn<F>(
    id: &'static str,
    label: &'static str,
    action: SettingsAction,
    on_action: F,
) -> impl IntoElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + 'static,
{
    grid_boxed_button(
        id,
        px(28.0),
        None,
        Some(label),
        None,
        GridTone::Graphite,
        true,
        move |_, window, cx| on_action(action.clone(), window, cx),
    )
}

fn swatch_dot(hex: u32) -> impl IntoElement {
    div()
        .w(px(10.0))
        .h(px(10.0))
        .rounded(px(2.0))
        .bg(rgb(hex))
        .border_1()
        .border_color(graphite_border())
}

fn render_general_tab<F>(state: &SettingsModalState, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section(
            "Window & Startup",
            vec![
                toggle_row(
                    "settings-close-to-tray",
                    "Close to System Tray",
                    "Keep active Downloads and Browser Capture running in the tray when closing the main window (Ctrl+Q or Tray -> Quit to exit).",
                    state.draft.ui_close_to_tray,
                    SettingsAction::ToggleCloseToTray,
                    on_action.clone(),
                ),
                toggle_row(
                    "settings-launch-startup",
                    "Launch on Windows Startup",
                    "Start Posthaze minimized to the System Tray automatically when you sign in to Windows.",
                    state.draft.launch_on_startup,
                    SettingsAction::ToggleLaunchOnStartup,
                    on_action.clone(),
                ),
            ],
        ))
        .child(settings_section(
            "Desktop Notifications",
            vec![
                toggle_row(
                    "notify-added",
                    "Download Added",
                    "Show a Windows notification when a new download is added to the queue.",
                    state.draft.notify_on_added,
                    SettingsAction::ToggleNotifyAdded,
                    on_action.clone(),
                ),
                toggle_row(
                    "notify-started",
                    "Download Started",
                    "Show a Windows notification when a new download begins transferring.",
                    state.draft.notify_on_started,
                    SettingsAction::ToggleNotifyStarted,
                    on_action.clone(),
                ),
                toggle_row(
                    "notify-completed",
                    "Download Completed",
                    "Show a Windows notification when a file finishes.",
                    state.draft.notify_on_completed,
                    SettingsAction::ToggleNotifyCompleted,
                    on_action.clone(),
                ),
                toggle_row(
                    "notify-failed",
                    "Download Failed",
                    "Show a Windows notification if a download stops with an error.",
                    state.draft.notify_on_failed,
                    SettingsAction::ToggleNotifyFailed,
                    on_action.clone(),
                ),
                toggle_row(
                    "notify-inactive",
                    "Only When Window Is Unfocused",
                    "Suppress notifications while you are actively viewing Posthaze.",
                    state.draft.notify_only_inactive,
                    SettingsAction::ToggleNotifyOnlyInactive,
                    on_action,
                ),
            ],
        ))
        .into_any_element()
}

fn render_downloads_tab<F>(state: &SettingsModalState, cx: &App, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    let browse_action = on_action.clone();
    let cat_browse_action = on_action.clone();
    let cat_clear_action = on_action.clone();
    let picker_toggle_action = on_action.clone();
    let new_cat_action = on_action.clone();
    let del_cat_action = on_action.clone();
    let reset_cat_action = on_action.clone();

    let live_cats = state.live_categories_snapshot(cx);
    let selected_cat = live_cats
        .iter()
        .find(|c| c.id == state.selected_category_id)
        .or_else(|| live_cats.first())
        .cloned()
        .unwrap_or_else(|| default_builtin_categories().into_iter().last().unwrap());
    let is_general = selected_cat.id == GENERAL_CATEGORY_ID;
    let base_dir_str = state.dir_input.read(cx).value();
    let base_dir = PathBuf::from(base_dir_str.trim());
    let resolved_cat_dir = effective_category_dir(&selected_cat, &base_dir)
        .to_string_lossy()
        .to_string();

    let conflicts = find_extension_conflicts(&live_cats);
    let active_conflicts: Vec<String> = selected_cat
        .extensions
        .iter()
        .filter_map(|ext| {
            conflicts
                .get(ext)
                .map(|owners| format!(".{ext} ({})", owners.join(", ")))
        })
        .collect();

    let test_query = state.cat_rule_test_input.read(cx).value().trim().to_string();
    let test_preview = if test_query.is_empty() {
        "Type a filename or URL to preview routing".to_string()
    } else {
        let is_url = test_query.contains("://");
        let fname = if is_url {
            test_query
                .split(['/', '?', '#'])
                .rfind(|s| !s.is_empty())
                .unwrap_or("file")
        } else {
            test_query.as_str()
        };
        let url_opt = is_url.then_some(test_query.as_str());
        if let Some(matched) = resolve_category(
            &live_cats,
            fname,
            url_opt,
            state.draft.category_url_rule_precedence,
        ) {
            let dest = effective_category_dir(matched, &base_dir);
            format!("{} -> {}", matched.name, dest.display())
        } else {
            "No category matched".to_string()
        }
    };

    let cat_filter_query = state.cat_filter_input.read(cx).value().trim().to_ascii_lowercase();
    let filtered_cats: Vec<_> = live_cats
        .iter()
        .filter(|cat| {
            if cat_filter_query.is_empty() {
                return true;
            }
            cat.name.to_ascii_lowercase().contains(&cat_filter_query)
                || cat
                    .extensions
                    .iter()
                    .any(|ext| ext.contains(&cat_filter_query))
                || cat
                    .url_patterns
                    .iter()
                    .any(|pat| pat.contains(&cat_filter_query))
        })
        .cloned()
        .collect();
    let total_cat_count = live_cats.len();
    let shown_cat_count = filtered_cats.len();
    let selected_rule_count = selected_cat.extensions.len() + selected_cat.url_patterns.len();
    let selector_summary = if is_general {
        format!("Catch-all · {total_cat_count} total")
    } else if !active_conflicts.is_empty() {
        format!(
            "! {} conflict(s) · {total_cat_count} total",
            active_conflicts.len()
        )
    } else {
        format!("{selected_rule_count} rules · {total_cat_count} total")
    };
    let cat_role_tag = if is_general {
        "Fallback"
    } else if selected_cat.is_builtin {
        "Built-in"
    } else {
        "Custom"
    };

    let committed_cat = state
        .categories
        .iter()
        .find(|c| c.id == selected_cat.id)
        .unwrap_or(&selected_cat);
    let committed_exts = committed_cat.extensions.clone();
    let committed_pats = committed_cat.url_patterns.clone();
    let has_ext_draft = !state.cat_extensions_input.read(cx).value().trim().is_empty();
    let has_pat_draft = !state.cat_patterns_input.read(cx).value().trim().is_empty();

    let compact_cat_field = |label: &'static str, input: &Entity<InputState>| {
        div()
            .w_full()
            .border_t_1()
            .border_l_1()
            .border_r_1()
            .border_color(graphite_border())
            .child(grid_kv_row_with_bg(
                label,
                96.0,
                grid_input_box(input),
                GridTone::Graphite,
                Some(input_bg()),
            ))
    };

    let compact_chip_field = {
        let conflicts_ref = &conflicts;
        let on_action_chip = on_action.clone();
        move |row_id: &'static str,
              label: &'static str,
              items: Vec<String>,
              is_ext_field: bool,
              input: &Entity<InputState>,
              has_draft: bool| {
            let add_act = on_action_chip.clone();
            div()
                .w_full()
                .border_1()
                .border_color(graphite_border())
                .flex()
                .items_stretch()
                .child(
                    div()
                        .w(px(96.0))
                        .flex_shrink_0()
                        .bg(graphite_elevated())
                        .border_r_1()
                        .border_color(graphite_border())
                        .px_2p5()
                        .py_2()
                        .flex()
                        .items_start()
                        .text_size(TYPE_MICRO)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(silver_muted())
                        .child(label),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .bg(input_bg())
                        .px_2p5()
                        .py_1p5()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_1p5()
                        .children(items.into_iter().enumerate().map(|(idx, token)| {
                            let is_conflicted = is_ext_field && conflicts_ref.contains_key(&token);
                            let remove_act = on_action_chip.clone();
                            let token_for_remove = token.clone();
                            div()
                                .h(px(22.0))
                                .px_1p5()
                                .bg(graphite_elevated())
                                .border_1()
                                .border_color(if is_conflicted {
                                    selenium_amber()
                                } else {
                                    graphite_border()
                                })
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .font_family(MONO_FONT)
                                        .text_size(TYPE_MICRO)
                                        .text_color(if is_conflicted {
                                            selenium_amber()
                                        } else {
                                            silver_text()
                                        })
                                        .child(token),
                                )
                                .child(
                                    div()
                                        .id((row_id, idx))
                                        .cursor_pointer()
                                        .text_color(silver_muted())
                                        .hover(|s| s.text_color(errata_vermilion()))
                                        .on_click(move |_, window, cx| {
                                            let action = if is_ext_field {
                                                SettingsAction::RemoveCategoryExtension(
                                                    token_for_remove.clone(),
                                                )
                                            } else {
                                                SettingsAction::RemoveCategoryPattern(
                                                    token_for_remove.clone(),
                                                )
                                            };
                                            remove_act(action, window, cx);
                                        })
                                        .child(Icon::new(IconName::X).size(px(10.0))),
                                )
                        }))
                        .child(
                            grid_input_box(input)
                                .min_w(px(136.0))
                                .h(px(22.0))
                                .flex()
                                .items_center()
                                .text_size(TYPE_MICRO),
                        )
                        .when(has_draft, |el| {
                            el.child(
                                div()
                                    .id((row_id, 9999usize))
                                    .h(px(22.0))
                                    .px_2()
                                    .bg(graphite_elevated())
                                    .border_1()
                                    .border_color(selenium_amber())
                                    .cursor_pointer()
                                    .hover(|s| s.bg(paper_hover()))
                                    .flex()
                                    .items_center()
                                    .font_family(MONO_FONT)
                                    .text_size(px(10.0))
                                    .text_color(selenium_amber())
                                    .on_click(move |_, window, cx| {
                                        let action = if is_ext_field {
                                            SettingsAction::AddCategoryExtension
                                        } else {
                                            SettingsAction::AddCategoryPattern
                                        };
                                        add_act(action, window, cx);
                                    })
                                    .child("+ Add"),
                            )
                        }),
                )
        }
    };

    let remember_title = format!(
        "Change folder for \"{}\" on last selected",
        selected_cat.name
    );

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section(
            "Storage & Pre-Flight",
            vec![
                stacked_row(
                    div()
                        .w_full()
                        .flex()
                        .items_stretch()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(labeled_input("Default Save Folder", &state.dir_input)),
                        )
                        .child(
                            div()
                                .border_t_1()
                                .border_b_1()
                                .border_r_1()
                                .border_color(graphite_border())
                                .flex()
                                .items_stretch()
                                .child(grid_cell_button(
                                    "settings-browse-dir",
                                    Some(IconName::FolderOpen),
                                    Some("Browse"),
                                    Some("Choose default download folder"),
                                    GridTone::Graphite,
                                    CellBorder::Left,
                                    false,
                                    move |_, window, cx| {
                                        browse_action(SettingsAction::BrowseSaveFolder, window, cx)
                                    },
                                )),
                        ),
                ),
                toggle_row(
                    "ui-lock-ext",
                    "Lock File Extension in Pre-Flight",
                    "Separate and lock the probed file extension so renaming cannot accidentally remove it.",
                    state.draft.ui_lock_file_extension,
                    SettingsAction::ToggleLockFileExtension,
                    on_action.clone(),
                ),
                toggle_row(
                    "ui-add-paused",
                    "Show \"Add Paused\" in Pre-Flight",
                    "Include the Add Paused button alongside Download Now.",
                    state.draft.ui_show_add_paused,
                    SettingsAction::ToggleShowAddPaused,
                    on_action.clone(),
                ),
            ],
        ))
        .child(settings_section(
            "Categories, File Types & Folders",
            vec![
                toggle_row(
                    "settings-auto-categorize",
                    "Auto-categorize downloads",
                    "Route new downloads into category folders using file types and host/URL rules.",
                    state.draft.auto_categorize_folders,
                    SettingsAction::ToggleAutoCategorize,
                    on_action.clone(),
                ),
                toggle_row(
                    "settings-url-rule-precedence",
                    "Prioritize host / URL rules over extensions",
                    "When enabled, matching URL or domain rules take precedence over file extension rules.",
                    state.draft.category_url_rule_precedence,
                    SettingsAction::ToggleCategoryUrlPrecedence,
                    on_action.clone(),
                ),
                stacked_row(
                    div()
                        .w_full()
                        .bg(graphite_bg())
                        .border_1()
                        .border_color(graphite_border())
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        // Top Category Selector Bar: Full-width picker trigger + crisp right actions
                        .child(
                            div()
                                .h(px(36.0))
                                .w_full()
                                .bg(graphite_elevated())
                                .border_b_1()
                                .border_color(graphite_border())
                                .flex()
                                .items_center()
                                .child(
                                    div()
                                        .id("settings-cat-selector")
                                        .flex_1()
                                        .min_w_0()
                                        .h_full()
                                        .cursor_pointer()
                                        .hover(|s| s.bg(paper_hover()))
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .on_click(move |_, window, cx| {
                                            picker_toggle_action(
                                                SettingsAction::ToggleCategoryPicker,
                                                window,
                                                cx,
                                            );
                                        })
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .h_full()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .pr_2()
                                                .child(grid_selection_bar(true))
                                                .child(
                                                    Icon::new(IconName::Folder)
                                                        .size(px(13.0))
                                                        .text_color(silver_muted()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(TYPE_BODY)
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(silver_text())
                                                        .truncate()
                                                        .child(selected_cat.name.clone()),
                                                )
                                                .child(grid_badge(cat_role_tag, BadgeTone::Muted)),
                                        )
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .pr_2p5()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .font_family(MONO_FONT)
                                                        .text_size(px(10.0))
                                                        .text_color(if active_conflicts.is_empty() {
                                                            silver_muted()
                                                        } else {
                                                            selenium_amber()
                                                        })
                                                        .child(selector_summary),
                                                )
                                                .child(
                                                    Icon::new(if state.cat_picker_open {
                                                        IconName::ChevronUp
                                                    } else {
                                                        IconName::ChevronDown
                                                    })
                                                    .size(px(13.0))
                                                    .text_color(silver_muted()),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .child(grid_cell_button(
                                            "settings-cat-new",
                                            Some(IconName::Plus),
                                            Some("New"),
                                            Some("Create a new custom category"),
                                            GridTone::Graphite,
                                            CellBorder::Left,
                                            true,
                                            move |_, window, cx| {
                                                new_cat_action(
                                                    SettingsAction::NewCategory,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        ))
                                        .when(!selected_cat.is_builtin, |el| {
                                            el.child(grid_cell_button(
                                                "settings-cat-del",
                                                Some(IconName::Trash),
                                                Some("Delete"),
                                                Some("Delete selected custom category"),
                                                GridTone::Danger,
                                                CellBorder::Left,
                                                true,
                                                move |_, window, cx| {
                                                    del_cat_action(
                                                        SettingsAction::DeleteSelectedCategory,
                                                        window,
                                                        cx,
                                                    );
                                                },
                                            ))
                                        })
                                        .child(grid_cell_button(
                                            "settings-cat-reset",
                                            Some(IconName::RotateCcw),
                                            None,
                                            Some("Restore default built-in categories"),
                                            GridTone::Graphite,
                                            CellBorder::Left,
                                            true,
                                            move |_, window, cx| {
                                                reset_cat_action(
                                                    SettingsAction::ResetDefaultCategories,
                                                    window,
                                                    cx,
                                                );
                                            },
                                        )),
                                ),
                        )
                        // Expandable Searchable Picker Drawer: handles 10..200 categories at full width
                        .when(state.cat_picker_open, |el| {
                            el.child(
                                div()
                                    .w_full()
                                    .border_b_1()
                                    .border_color(graphite_border())
                                    .bg(graphite_elevated().opacity(0.45))
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .h(px(30.0))
                                            .w_full()
                                            .px_2p5()
                                            .bg(input_bg())
                                            .border_b_1()
                                            .border_color(graphite_border())
                                            .flex()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                Icon::new(IconName::Search)
                                                    .size(px(12.0))
                                                    .text_color(silver_muted()),
                                            )
                                            .child(
                                                grid_input_box(&state.cat_filter_input)
                                                    .text_size(TYPE_MICRO),
                                            )
                                            .child(
                                                div()
                                                    .flex_shrink_0()
                                                    .font_family(MONO_FONT)
                                                    .text_size(px(10.0))
                                                    .text_color(silver_muted())
                                                    .child(format!(
                                                        "{shown_cat_count}/{total_cat_count}"
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("settings-cat-picker-list")
                                            .w_full()
                                            .max_h(px(192.0))
                                            .overflow_y_scroll()
                                            .flex()
                                            .flex_col()
                                            .when(filtered_cats.is_empty(), |list| {
                                                list.child(
                                                    div()
                                                        .px_3()
                                                        .py_3()
                                                        .font_family(MONO_FONT)
                                                        .text_size(TYPE_MICRO)
                                                        .text_color(silver_muted())
                                                        .child("No matching categories"),
                                                )
                                            })
                                            .children(filtered_cats.into_iter().enumerate().map(
                                                |(idx, cat)| {
                                                    let is_sel =
                                                        cat.id == state.selected_category_id;
                                                    let cat_id = cat.id.clone();
                                                    let act = on_action.clone();
                                                    let has_conflict = cat
                                                        .extensions
                                                        .iter()
                                                        .any(|e| conflicts.contains_key(e));
                                                    let rule_preview = if cat.id
                                                        == GENERAL_CATEGORY_ID
                                                    {
                                                        "Fallback for unmatched downloads"
                                                            .to_string()
                                                    } else {
                                                        let mut parts: Vec<String> = cat
                                                            .extensions
                                                            .iter()
                                                            .cloned()
                                                            .chain(cat.url_patterns.iter().cloned())
                                                            .collect();
                                                        if parts.is_empty() {
                                                            "No rules configured".to_string()
                                                        } else {
                                                            let total_parts = parts.len();
                                                            parts.truncate(4);
                                                            if total_parts > 4 {
                                                                format!(
                                                                    "{} (+{})",
                                                                    parts.join(", "),
                                                                    total_parts - 4
                                                                )
                                                            } else {
                                                                parts.join(", ")
                                                            }
                                                        }
                                                    };
                                                    let role = if cat.id == GENERAL_CATEGORY_ID {
                                                        "Fallback"
                                                    } else if cat.is_builtin {
                                                        "Built-in"
                                                    } else {
                                                        "Custom"
                                                    };
                                                    div()
                                                        .id(("settings-cat-row", idx))
                                                        .h(px(30.0))
                                                        .w_full()
                                                        .flex_shrink_0()
                                                        .border_b_1()
                                                        .border_color(graphite_border())
                                                        .bg(if is_sel {
                                                            paper_selected()
                                                        } else {
                                                            Hsla::transparent_black()
                                                        })
                                                        .hover(|s| s.bg(paper_hover()))
                                                        .cursor_pointer()
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .gap_3()
                                                        .on_click(move |_, window, cx| {
                                                            act(
                                                                SettingsAction::SelectCategory(
                                                                    cat_id.clone(),
                                                                ),
                                                                window,
                                                                cx,
                                                            );
                                                        })
                                                        .child(
                                                            div()
                                                                .flex()
                                                                .items_center()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .h_full()
                                                                .gap_3()
                                                                .child(grid_selection_bar(is_sel))
                                                                .child(
                                                                    div()
                                                                        .w(px(124.0))
                                                                        .flex_shrink_0()
                                                                        .text_size(TYPE_MICRO)
                                                                        .font_weight(if is_sel {
                                                                            FontWeight::SEMIBOLD
                                                                        } else {
                                                                            FontWeight::NORMAL
                                                                        })
                                                                        .text_color(if is_sel {
                                                                            silver_text()
                                                                        } else {
                                                                            silver_muted()
                                                                        })
                                                                        .truncate()
                                                                        .child(cat.name),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .flex_1()
                                                                        .min_w_0()
                                                                        .font_family(MONO_FONT)
                                                                        .text_size(px(10.0))
                                                                        .text_color(silver_muted())
                                                                        .truncate()
                                                                        .child(rule_preview),
                                                                ),
                                                        )
                                                        .child(
                                                            div()
                                                                .px_2p5()
                                                                .flex_shrink_0()
                                                                .flex()
                                                                .items_center()
                                                                .gap_2()
                                                                .when(has_conflict, |el| {
                                                                    el.child(
                                                                        div()
                                                                            .font_family(MONO_FONT)
                                                                            .text_size(px(10.0))
                                                                            .text_color(
                                                                                selenium_amber(),
                                                                            )
                                                                            .child("!"),
                                                                    )
                                                                })
                                                                .child(
                                                                    div()
                                                                        .font_family(MONO_FONT)
                                                                        .text_size(px(10.0))
                                                                        .text_color(if is_sel {
                                                                            silver_text()
                                                                        } else {
                                                                            silver_muted()
                                                                        })
                                                                        .child(role),
                                                                ),
                                                        )
                                                },
                                            )),
                                    ),
                            )
                        })
                        // Full-Width Active Category Editor: unconstrained height so nothing clips
                        .child(
                            div()
                                .id("settings-cat-detail")
                                .w_full()
                                .p_3()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .when(!is_general, |el| {
                                    el.child(compact_cat_field("Name", &state.cat_name_input))
                                        .child(compact_chip_field(
                                            "settings-cat-ext-chip",
                                            "File types",
                                            committed_exts,
                                            true,
                                            &state.cat_extensions_input,
                                            has_ext_draft,
                                        ))
                                        .child(compact_chip_field(
                                            "settings-cat-pat-chip",
                                            "Host / URL",
                                            committed_pats,
                                            false,
                                            &state.cat_patterns_input,
                                            has_pat_draft,
                                        ))
                                })
                                .when(is_general, |el| {
                                    el.child(
                                        div()
                                            .w_full()
                                            .px_3()
                                            .py_2()
                                            .bg(graphite_elevated())
                                            .border_1()
                                            .border_color(graphite_border())
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .text_color(silver_muted())
                                            .child("Catches any file types and URLs not listed in another category"),
                                    )
                                })
                                .when(!active_conflicts.is_empty(), |el| {
                                    el.child(
                                        div()
                                            .w_full()
                                            .px_2p5()
                                            .py_1p5()
                                            .bg(graphite_elevated())
                                            .border_1()
                                            .border_color(selenium_amber())
                                            .font_family(MONO_FONT)
                                            .text_size(TYPE_MICRO)
                                            .text_color(selenium_amber())
                                            .child(format!(
                                                "Shared extension(s): {}",
                                                active_conflicts.join(", ")
                                            )),
                                    )
                                })
                                .child(
                                    div()
                                        .w_full()
                                        .flex()
                                        .items_stretch()
                                        .child(
                                            div().flex_1().min_w_0().child(compact_cat_field(
                                                "Folder",
                                                &state.cat_dir_input,
                                            )),
                                        )
                                        .when(selected_cat.output_dir.is_some(), |el| {
                                            el.child(
                                                div()
                                                    .border_t_1()
                                                    .border_b_1()
                                                    .border_color(graphite_border())
                                                    .flex()
                                                    .items_stretch()
                                                    .child(grid_cell_button(
                                                        "settings-cat-clear-dir",
                                                        Some(IconName::X),
                                                        None,
                                                        Some("Reset to dynamic subfolder"),
                                                        GridTone::Graphite,
                                                        CellBorder::Left,
                                                        true,
                                                        move |_, window, cx| {
                                                            cat_clear_action(
                                                                SettingsAction::ClearCategoryFolder,
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            )
                                        })
                                        .child(
                                            div()
                                                .border_t_1()
                                                .border_b_1()
                                                .border_r_1()
                                                .border_color(graphite_border())
                                                .flex()
                                                .items_stretch()
                                                .child(grid_cell_button(
                                                    "settings-cat-browse-dir",
                                                    Some(IconName::FolderOpen),
                                                    Some("Browse"),
                                                    Some("Choose directory for this category"),
                                                    GridTone::Graphite,
                                                    CellBorder::Left,
                                                    true,
                                                    move |_, window, cx| {
                                                        cat_browse_action(
                                                            SettingsAction::BrowseCategoryFolder,
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .font_family(MONO_FONT)
                                        .text_size(TYPE_MICRO)
                                        .text_color(silver_muted())
                                        .truncate()
                                        .child(format!("Resolved: {resolved_cat_dir}")),
                                )
                                .child(
                                    div()
                                        .border_1()
                                        .border_color(graphite_border())
                                        .bg(graphite_elevated().opacity(0.35))
                                        .child(toggle_row_dyn(
                                            "settings-cat-remember-dir",
                                            remember_title,
                                            "Update this folder when changed in Pre-Flight"
                                                .to_string(),
                                            selected_cat.remember_last_folder,
                                            SettingsAction::ToggleCategoryRememberFolder,
                                            on_action.clone(),
                                        )),
                                ),
                        ),
                ),
                stacked_row(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(labeled_input("Rule Tester", &state.cat_rule_test_input))
                        .child(
                            div()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_MICRO)
                                .text_color(if test_query.is_empty() {
                                    silver_muted()
                                } else {
                                    selenium_amber()
                                })
                                .child(test_preview),
                        ),
                ),
                toggle_row(
                    "settings-server-file-time",
                    "Set file creation date as provided by the server",
                    "Apply the HTTP Last-Modified timestamp to completed files on disk.",
                    state.draft.set_file_time_from_server,
                    SettingsAction::ToggleSetFileTimeFromServer,
                    on_action.clone(),
                ),
            ],
        ))
        .child(settings_section(
            "Queue, Collisions & Safety",
            vec![
                setting_row(
                    "When File Exists",
                    "Action when the destination filename already exists on disk.",
                    segmented_track()
                        .child(choice_btn(
                            "coll-rename",
                            "Rename Copy",
                            state.draft.collision_policy == CollisionPolicy::Rename,
                            SettingsAction::SetCollisionPolicy(CollisionPolicy::Rename),
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "coll-overwrite",
                            "Overwrite",
                            state.draft.collision_policy == CollisionPolicy::Overwrite,
                            SettingsAction::SetCollisionPolicy(CollisionPolicy::Overwrite),
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "coll-ask",
                            "Ask Each Time",
                            state.draft.collision_policy == CollisionPolicy::Ask,
                            SettingsAction::SetCollisionPolicy(CollisionPolicy::Ask),
                            on_action.clone(),
                        )),
                ),
                setting_row(
                    "Remove Button Action",
                    "Default behavior for Trash / Delete key. Disk deletions use Recycle Bin (Shift+Delete deletes permanently).",
                    segmented_track()
                        .child(choice_btn(
                            "del-ask",
                            "Ask Each Time",
                            state.draft.delete_action_default == DeleteDefaultAction::Ask,
                            SettingsAction::SetDeleteDefault(DeleteDefaultAction::Ask),
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "del-remove",
                            "Remove Entry",
                            state.draft.delete_action_default == DeleteDefaultAction::Remove,
                            SettingsAction::SetDeleteDefault(DeleteDefaultAction::Remove),
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "del-file",
                            "Recycle Bin",
                            state.draft.delete_action_default == DeleteDefaultAction::DeleteFile,
                            SettingsAction::SetDeleteDefault(DeleteDefaultAction::DeleteFile),
                            on_action.clone(),
                        )),
                ),
                setting_row(
                    "Auto-Retry on Error",
                    "Automatic retry attempts (0–10) after a transient failure.",
                    compact_input(&state.auto_retry_input, px(88.0), Some("retries")),
                ),
                toggle_row(
                    "settings-sync",
                    "Flush File Buffers on Finish",
                    "Call sync_all before finalizing each download to protect against power loss.",
                    state.draft.sync_on_complete,
                    SettingsAction::ToggleSyncOnComplete,
                    on_action,
                ),
            ],
        ))
        .into_any_element()
}

fn render_network_tab<F>(state: &SettingsModalState, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    let mut proxy_rows = vec![setting_row(
        "Proxy Mode",
        "How HTTP/HTTPS requests are routed.",
        segmented_track()
            .child(choice_btn(
                "proxy-sys",
                "System",
                state.draft.proxy_mode == ProxyMode::System,
                SettingsAction::SetProxyMode(ProxyMode::System),
                on_action.clone(),
            ))
            .child(choice_btn(
                "proxy-none",
                "Direct",
                state.draft.proxy_mode == ProxyMode::None,
                SettingsAction::SetProxyMode(ProxyMode::None),
                on_action.clone(),
            ))
            .child(choice_btn(
                "proxy-manual",
                "Manual",
                state.draft.proxy_mode == ProxyMode::Manual,
                SettingsAction::SetProxyMode(ProxyMode::Manual),
                on_action.clone(),
            )),
    )];
    if state.draft.proxy_mode == ProxyMode::Manual {
        proxy_rows.push(stacked_row(labeled_input("Proxy URL", &state.proxy_url_input)));
    }

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section("Proxy Routing", proxy_rows))
        .child(settings_section(
            "Bandwidth & Concurrency",
            vec![
                stacked_row(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_4()
                                .child(setting_text(
                                    "Speed Limit",
                                    "Global bandwidth cap across active workers (0 = Unlimited).",
                                ))
                                .child(compact_input(&state.speed_limit_input, px(108.0), Some("KiB/s"))),
                        )
                        .child(
                            segmented_track()
                                .child(preset_btn(
                                    "spd-unlim",
                                    "Unlimited",
                                    SettingsAction::SetSpeedPreset(0),
                                    on_action.clone(),
                                ))
                                .child(preset_btn(
                                    "spd-low",
                                    "Low · 256 KiB/s",
                                    SettingsAction::SetSpeedPreset(256),
                                    on_action.clone(),
                                ))
                                .child(preset_btn(
                                    "spd-med",
                                    "Medium · 2 MiB/s",
                                    SettingsAction::SetSpeedPreset(2048),
                                    on_action.clone(),
                                ))
                                .child(preset_btn(
                                    "spd-high",
                                    "High · 8 MiB/s",
                                    SettingsAction::SetSpeedPreset(8192),
                                    on_action,
                                )),
                        ),
                ),
                setting_row(
                    "Workers per Download",
                    "Maximum concurrent byte-range segments per file (1–32).",
                    compact_input(&state.workers_input, px(88.0), Some("workers")),
                ),
                setting_row(
                    "Parallel Downloads",
                    "Maximum active downloads running at the same time (1+).",
                    compact_input(&state.concurrent_input, px(88.0), Some("slots")),
                ),
            ],
        ))
        .into_any_element()
}

fn render_appearance_tab<F>(state: &SettingsModalState, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    let render_theme_card = |preset: &ThemePreset| {
        let is_selected = find_theme_preset(&state.draft.ui_theme).id == preset.id;
        let theme_id = preset.id.to_string();
        let act = on_action.clone();
        let tone_label = match preset.badge {
            ThemeBadge::Dark => "DARK",
            ThemeBadge::Light => "LIGHT",
            ThemeBadge::Auto => "AUTO",
        };
        div()
            .id(SharedString::from(format!("theme-{}", preset.id)))
            .flex_1()
            .min_w_0()
            .px_3()
            .py_2()
            .cursor_pointer()
            .bg(if is_selected {
                paper_selected()
            } else {
                Hsla::transparent_black()
            })
            .border_1()
            .border_color(if is_selected {
                selenium_amber()
            } else {
                graphite_border()
            })
            .hover(|s| s.bg(paper_hover()))
            .on_click(move |_, window, cx| {
                act(SettingsAction::SetTheme(theme_id.clone()), window, cx);
            })
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(TYPE_BODY)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(silver_text())
                            .child(preset.name),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(swatch_dot(preset.swatches[0]))
                            .child(swatch_dot(preset.swatches[1]))
                            .child(swatch_dot(preset.swatches[2]))
                            .child(
                                div()
                                    .ml_1()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_HEADER)
                                    .text_color(silver_muted())
                                    .child(tone_label),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(TYPE_MICRO)
                    .text_color(silver_muted())
                    .truncate()
                    .child(preset.description),
            )
    };

    let theme_grid_rows = BUILTIN_THEME_PRESETS.chunks(2).map(|pair| {
        div()
            .w_full()
            .flex()
            .gap_2()
            .children(pair.iter().map(render_theme_card))
            .when(pair.len() == 1, |el| el.child(div().flex_1().min_w_0()))
    });

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section(
            "Theme & Accent",
            vec![
                stacked_row(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_size(TYPE_MICRO)
                                .text_color(silver_muted())
                                .child("Select a color theme (previews live across open windows; Cancel reverts)."),
                        )
                        .child(div().flex().flex_col().gap_2().children(theme_grid_rows))
                        .when_some(
                            mica_fallback_status_note(
                                &state.draft.ui_theme,
                                crate::platform_hooks::mica_availability(),
                            ),
                            |el, note| {
                                el.child(
                                    div()
                                        .id("mica-fallback-status-note")
                                        .pt_1()
                                        .text_size(TYPE_MICRO)
                                        .line_height(px(16.0))
                                        .text_color(silver_muted())
                                        .child(note),
                                )
                            },
                        ),
                ),
                stacked_row(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .text_size(TYPE_BODY)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(silver_text())
                                .child("Accent Color Override"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1p5()
                                .child({
                                    let active = state.draft.ui_accent_override.is_none();
                                    let act = on_action.clone();
                                    div()
                                        .id("accent-default")
                                        .h(px(28.0))
                                        .px_2p5()
                                        .border_1()
                                        .border_color(if active {
                                            selenium_amber()
                                        } else {
                                            graphite_border()
                                        })
                                        .bg(if active {
                                            paper_selected()
                                        } else {
                                            Hsla::transparent_black()
                                        })
                                        .hover(|s| s.bg(paper_hover()))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .font_family(MONO_FONT)
                                        .text_size(TYPE_MICRO)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if active { silver_text() } else { silver_muted() })
                                        .on_click(move |_, window, cx| {
                                            act(SettingsAction::SetAccentOverride(None), window, cx);
                                        })
                                        .child("THEME DEFAULT")
                                })
                                .children(ACCENT_PRESETS.iter().map(|preset| {
                                    let active = state.draft.ui_accent_override.as_deref() == Some(preset.id);
                                    let act = on_action.clone();
                                    let pid = preset.id.to_string();
                                    div()
                                        .id(SharedString::from(format!("accent-{}", preset.id)))
                                        .h(px(28.0))
                                        .px_2p5()
                                        .border_1()
                                        .border_color(if active {
                                            rgb(preset.accent).into()
                                        } else {
                                            graphite_border()
                                        })
                                        .bg(if active {
                                            paper_selected()
                                        } else {
                                            Hsla::transparent_black()
                                        })
                                        .hover(|s| s.bg(paper_hover()))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .font_family(MONO_FONT)
                                        .text_size(TYPE_MICRO)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if active { silver_text() } else { silver_muted() })
                                        .on_click(move |_, window, cx| {
                                            act(
                                                SettingsAction::SetAccentOverride(Some(pid.clone())),
                                                window,
                                                cx,
                                            );
                                        })
                                        .child(swatch_dot(preset.accent))
                                        .child(preset.name)
                                })),
                        ),
                ),
            ],
        ))
        .child(settings_section(
            "Table & Scale",
            vec![
                setting_row(
                    "Table Row Density",
                    "Control vertical row height across the virtualized Downloads table.",
                    segmented_track()
                        .child(choice_btn(
                            "density-compact",
                            "COMPACT",
                            state.draft.ui_table_density == TableDensity::Compact,
                            SettingsAction::SetTableDensity(TableDensity::Compact),
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "density-standard",
                            "STANDARD",
                            state.draft.ui_table_density == TableDensity::Standard,
                            SettingsAction::SetTableDensity(TableDensity::Standard),
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "density-comfortable",
                            "COMFORTABLE",
                            state.draft.ui_table_density == TableDensity::Comfortable,
                            SettingsAction::SetTableDensity(TableDensity::Comfortable),
                            on_action.clone(),
                        )),
                ),
                setting_row(
                    "Visible Columns",
                    "Toggle optional telemetry columns (automatically hidden on narrow windows).",
                    segmented_track()
                        .child(choice_btn(
                            "col-speed",
                            "SPEED",
                            state.draft.ui_show_speed_col,
                            SettingsAction::ToggleSpeedCol,
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "col-eta",
                            "ETA",
                            state.draft.ui_show_eta_col,
                            SettingsAction::ToggleEtaCol,
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "col-segs",
                            "SEGS",
                            state.draft.ui_show_segs_col,
                            SettingsAction::ToggleSegsCol,
                            on_action.clone(),
                        )),
                ),
                setting_row(
                    "Interface Scale",
                    "Adjust overall text and control sizing.",
                    segmented_track()
                        .child(choice_btn(
                            "zoom-90",
                            "90%",
                            state.draft.ui_zoom_percent == 90,
                            SettingsAction::SetZoom(90),
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "zoom-100",
                            "100%",
                            state.draft.ui_zoom_percent == 100,
                            SettingsAction::SetZoom(100),
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "zoom-110",
                            "110%",
                            state.draft.ui_zoom_percent == 110,
                            SettingsAction::SetZoom(110),
                            on_action.clone(),
                        ))
                        .child(choice_btn(
                            "zoom-125",
                            "125%",
                            state.draft.ui_zoom_percent == 125,
                            SettingsAction::SetZoom(125),
                            on_action.clone(),
                        )),
                ),
            ],
        ))
        .child(settings_section(
            "Workspace Chrome",
            vec![
                toggle_row(
                    "ui-click-insp",
                    "Open Byte-Map Inspector on Click",
                    "Automatically expand the bottom inspector when selecting a download row.",
                    state.draft.ui_click_row_opens_inspector,
                    SettingsAction::ToggleClickOpensInspector,
                    on_action.clone(),
                ),
                toggle_row(
                    "ui-start-rail",
                    "Start with Sidebar Collapsed",
                    "Launch into the compact left icon rail instead of the expanded filter rail.",
                    state.draft.ui_start_sidebar_collapsed,
                    SettingsAction::ToggleStartSidebarCollapsed,
                    on_action.clone(),
                ),
                toggle_row(
                    "ui-titlebar-tel",
                    "Show Live Telemetry in Titlebar",
                    "Display active transfer slots, throughput, and queue count in the top bar.",
                    state.draft.ui_show_titlebar_telemetry,
                    SettingsAction::ToggleTitlebarTelemetry,
                    on_action.clone(),
                ),
                toggle_row(
                    "ui-toolbar-labels",
                    "Show Toolbar Button Labels",
                    "Display text labels on top-bar action cells (uncheck for icon-only cells).",
                    state.draft.ui_show_toolbar_labels,
                    SettingsAction::ToggleToolbarLabels,
                    on_action.clone(),
                ),
                toggle_row(
                    "ui-minimap",
                    "Show Scrollbar Minimap",
                    "Highlight Active and Failed row positions along the table scrollbar rail.",
                    state.draft.ui_show_scrollbar_minimap,
                    SettingsAction::ToggleScrollbarMinimap,
                    on_action,
                ),
            ],
        ))
        .into_any_element()
}

fn render_integrations_tab<F>(state: &SettingsModalState, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    let reg_act = on_action.clone();
    let refresh_act = on_action.clone();
    let defender_act = on_action.clone();

    let mut browser_rows = vec![
        stacked_row(
            div()
                .flex()
                .flex_col()
                .gap_2p5()
                .child(
                    div()
                        .text_size(TYPE_BODY)
                        .line_height(px(18.0))
                        .text_color(silver_muted())
                        .child("Load the bundled extension in chrome://extensions (or edge://extensions) and click REGISTER NATIVE HOST. The Extension ID below is pre-filled for unpacked installs (edit only if using a custom ID)."),
                )
                .child(labeled_input("Extension ID", &state.extension_id_input))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(grid_boxed_button(
                            "settings-register-host",
                            px(30.0),
                            Some(IconName::Check),
                            Some("REGISTER NATIVE HOST"),
                            Some("Write host manifest and register HKCU keys for Chrome, Edge, and Brave"),
                            GridTone::Primary,
                            false,
                            move |_, window, cx| {
                                reg_act(SettingsAction::RegisterNativeHost, window, cx)
                            },
                        ))
                        .child(grid_boxed_button(
                            "settings-refresh-host",
                            px(30.0),
                            Some(IconName::RefreshCw),
                            Some("REFRESH STATUS"),
                            Some("Re-check HKCU Native Messaging registration"),
                            GridTone::Graphite,
                            false,
                            move |_, window, cx| {
                                refresh_act(SettingsAction::RefreshBrowserStatus, window, cx)
                            },
                        )),
                )
                .when_some(state.integration_feedback.as_ref(), |el, feedback| {
                    el.child(inline_feedback_banner(feedback))
                }),
        ),
    ];

    for (name, ok) in &state.browser_statuses {
        browser_rows.push(
            div()
                .w_full()
                .px_3p5()
                .h(px(36.0))
                .flex()
                .items_center()
                .justify_between()
                .child(div().text_size(TYPE_BODY).text_color(silver_text()).child(*name))
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .text_color(if *ok {
                            halide_emerald()
                        } else {
                            silver_muted()
                        })
                        .child(if *ok { "REGISTERED" } else { "NOT REGISTERED" }),
                )
                .into_any_element(),
        );
    }

    let mut hook_rows = vec![
        toggle_row(
            "cmd-enabled",
            "Launch Program on Finish",
            "Run an antivirus scanner or script automatically after each completed download.",
            state.draft.completion_cmd_enabled,
            SettingsAction::ToggleCompletionCmd,
            on_action,
        ),
        setting_row(
            "Antivirus Preset",
            "Fill Program Path and Arguments (%path%) with Windows Defender scanner defaults.",
            grid_boxed_button(
                "settings-defender-preset",
                px(28.0),
                Some(IconName::ShieldCheck),
                Some("USE WINDOWS DEFENDER"),
                None,
                GridTone::Graphite,
                true,
                move |_, window, cx| {
                    defender_act(SettingsAction::ApplyDefenderPreset, window, cx);
                },
            ),
        ),
    ];
    if state.draft.completion_cmd_enabled {
        hook_rows.push(stacked_row(
            div()
                .flex()
                .flex_col()
                .gap_2p5()
                .child(labeled_input("Program Path", &state.cmd_path_input))
                .child(labeled_input(
                    "Arguments (%path% = completed file)",
                    &state.cmd_args_input,
                )),
        ));
    }

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section("Browser Capture", browser_rows))
        .child(settings_section("Post-Download Command", hook_rows))
        .into_any_element()
}

fn render_system_tab<F>(
    state: &SettingsModalState,
    update_state: &UpdateState,
    is_portable: bool,
    on_action: F,
) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    let install_mode = if is_portable { "PORTABLE" } else { "INSTALLED" };
    let (status_color, status_text, progress_pct) = match update_state {
        UpdateState::Idle => (
            silver_muted(),
            "Click Check Now to check GitHub Releases for a newer version.".to_string(),
            None,
        ),
        UpdateState::Checking => (
            selenium_amber(),
            "Checking GitHub Releases for updates...".to_string(),
            None,
        ),
        UpdateState::UpToDate => (
            halide_emerald(),
            format!("Posthaze v{} is up to date.", env!("CARGO_PKG_VERSION")),
            None,
        ),
        UpdateState::Available(info) => (
            selenium_amber(),
            if is_portable {
                format!(
                    "Posthaze v{} is available. Portable builds update manually from GitHub Releases.",
                    info.version
                )
            } else {
                format!(
                    "Posthaze v{} is available (SHA-256 + Ed25519 verified).",
                    info.version
                )
            },
            None,
        ),
        UpdateState::Downloading { info, pct } => (
            selenium_amber(),
            format!("Downloading & verifying Posthaze v{} ({pct}%)...", info.version),
            Some(*pct),
        ),
        UpdateState::Installing => (
            halide_emerald(),
            "Verified! Launching silent installer and restarting...".to_string(),
            Some(100),
        ),
        UpdateState::Failed(err) => (errata_vermilion(), format!("Update error: {err}"), None),
    };

    let available = update_state.available_release().cloned();
    let is_busy = matches!(
        update_state,
        UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Installing
    );

    let act_copy = on_action.clone();
    let act_logs = on_action.clone();
    let act_folders = on_action.clone();
    let act_reset = on_action.clone();

    let mut maintenance_rows = vec![stacked_row(
        div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(grid_boxed_button(
                "settings-copy-json",
                px(30.0),
                Some(IconName::Copy),
                Some("COPY CONFIG JSON"),
                Some("Copy active settings as formatted JSON"),
                GridTone::Graphite,
                false,
                move |_, window, cx| act_copy(SettingsAction::CopyConfigJson, window, cx),
            ))
            .child(grid_boxed_button(
                "settings-open-logs",
                px(30.0),
                Some(IconName::FolderOpen),
                Some("OPEN LOGS FOLDER"),
                Some("Open the diagnostic logs directory in Windows Explorer"),
                GridTone::Graphite,
                false,
                move |_, window, cx| act_logs(SettingsAction::OpenLogsFolder, window, cx),
            ))
            .child(grid_boxed_button(
                "settings-clear-folders",
                px(30.0),
                Some(IconName::Trash),
                Some("CLEAR RECENT FOLDERS"),
                Some("Clear saved destination folder history used by Pre-Flight and Move To"),
                GridTone::Graphite,
                false,
                move |_, window, cx| act_folders(SettingsAction::ClearRecentFolders, window, cx),
            )),
    )];
    if let Some(feedback) = state.system_feedback.as_ref() {
        maintenance_rows.push(stacked_row(inline_feedback_banner(feedback)));
    }

    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(settings_section(
            "Software Updates",
            vec![
                setting_row(
                    concat!("Posthaze v", env!("CARGO_PKG_VERSION")),
                    "Signed GitHub Releases update channel",
                    grid_badge(install_mode, BadgeTone::Muted),
                ),
                toggle_row(
                    "settings-auto-check-updates",
                    "Check for Updates Automatically",
                    "Check GitHub Releases at startup and every 24 hours.",
                    state.draft.auto_check_updates,
                    SettingsAction::ToggleAutoCheckUpdates,
                    on_action.clone(),
                ),
                stacked_row(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2p5()
                        .child(
                            div()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_MICRO)
                                .text_color(status_color)
                                .child(status_text),
                        )
                        .when_some(progress_pct, |el, pct| {
                            el.child(
                                grid_progress_track(px(6.0), Some(graphite_border())).child(
                                    div()
                                        .h_full()
                                        .w(relative(f32::from(pct) / 100.0))
                                        .bg(selenium_amber()),
                                ),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap_2()
                                .when(!is_busy, |el| {
                                    let act = on_action.clone();
                                    el.child(grid_boxed_button(
                                        "settings-check-updates",
                                        px(30.0),
                                        Some(IconName::RefreshCw),
                                        Some("CHECK NOW"),
                                        Some("Check GitHub Releases for a newer version"),
                                        GridTone::Graphite,
                                        false,
                                        move |_, window, cx| {
                                            act(SettingsAction::CheckForUpdates, window, cx)
                                        },
                                    ))
                                })
                                .when_some(
                                    available.as_ref().filter(|_| {
                                        !is_portable
                                            && matches!(update_state, UpdateState::Available(_))
                                    }),
                                    |el, info| {
                                        let act = on_action.clone();
                                        let lbl =
                                            SharedString::from(format!("INSTALL v{}", info.version));
                                        el.child(
                                            div()
                                                .h(px(30.0))
                                                .border_1()
                                                .border_color(graphite_border())
                                                .child(grid_cell_button_dyn(
                                                    "settings-install-update",
                                                    Some(IconName::Download),
                                                    Some(lbl),
                                                    Some(
                                                        "Download, verify signature, and install silently",
                                                    ),
                                                    GridTone::Primary,
                                                    CellBorder::None,
                                                    false,
                                                    move |_, window, cx| {
                                                        act(
                                                            SettingsAction::InstallUpdate,
                                                            window,
                                                            cx,
                                                        )
                                                    },
                                                )),
                                        )
                                    },
                                )
                                .when_some(available.as_ref(), |el, info| {
                                    let act = on_action.clone();
                                    let url = info.notes_url.clone();
                                    let label = if is_portable {
                                        "DOWNLOAD FROM GITHUB"
                                    } else {
                                        "RELEASE NOTES"
                                    };
                                    el.child(grid_boxed_button(
                                        "settings-release-notes",
                                        px(30.0),
                                        Some(IconName::Globe),
                                        Some(label),
                                        Some("Open release page in default browser"),
                                        GridTone::Graphite,
                                        false,
                                        move |_, window, cx| {
                                            act(
                                                SettingsAction::OpenReleaseNotes(url.clone()),
                                                window,
                                                cx,
                                            )
                                        },
                                    ))
                                }),
                        ),
                ),
            ],
        ))
        .child(settings_section(
            "Diagnostics & Storage",
            maintenance_rows,
        ))
        .child(settings_section(
            "Factory Reset",
            vec![setting_row(
                "Restore Default Settings",
                "Reset preferences, proxy, notifications, hooks, and appearance to defaults. Downloads in your queue remain untouched.",
                grid_boxed_button(
                    "settings-reset-defaults",
                    px(30.0),
                    Some(IconName::RotateCcw),
                    Some("RESTORE DEFAULTS"),
                    None,
                    GridTone::Danger,
                    false,
                    move |_, window, cx| {
                        act_reset(SettingsAction::ResetToDefaults, window, cx);
                    },
                ),
            )],
        ))
        .into_any_element()
}

fn merge_unique_tokens(existing: &mut Vec<String>, additions: Vec<String>) {
    for token in additions {
        if !existing.iter().any(|cur| cur == &token) {
            existing.push(token);
        }
    }
}

fn mica_fallback_status_note(
    theme_id: &str,
    availability: Result<(), crate::platform_hooks::MicaFallbackReason>,
) -> Option<&'static str> {
    if !find_theme_preset(theme_id).is_mica {
        return None;
    }
    availability
        .err()
        .map(crate::platform_hooks::MicaFallbackReason::status_note)
}

#[cfg(test)]
mod tests {
    use super::{SettingsDraft, mica_fallback_status_note};
    use crate::platform_hooks::MicaFallbackReason;
    use posthaze_engine::{ProxyMode, Settings};

    #[test]
    fn mica_fallback_status_note_only_shows_for_mica_presets_when_unavailable() {
        assert_eq!(
            mica_fallback_status_note("chronophotographic", Err(MicaFallbackReason::UnsupportedOs)),
            None
        );
        assert_eq!(
            mica_fallback_status_note("archival", Err(MicaFallbackReason::TransparencyDisabled)),
            None
        );
        assert_eq!(mica_fallback_status_note("mica-system", Ok(())), None);
        assert_eq!(mica_fallback_status_note("mica-dark", Ok(())), None);
        assert_eq!(mica_fallback_status_note("mica-light", Ok(())), None);

        let all_reasons = [
            MicaFallbackReason::UnsupportedOs,
            MicaFallbackReason::DirectCompositionDisabled,
            MicaFallbackReason::HighContrastActive,
            MicaFallbackReason::TransparencyDisabled,
            MicaFallbackReason::DwmRejected,
        ];
        for reason in all_reasons {
            for mica_id in ["mica-system", "mica-dark", "mica-light"] {
                assert_eq!(
                    mica_fallback_status_note(mica_id, Err(reason)),
                    Some(reason.status_note()),
                    "expected status note for {mica_id} with {reason:?}"
                );
            }
        }
    }

    #[test]
    fn settings_draft_round_trips_and_delegates_validation() {
        let defaults = Settings::default();
        let mut draft = SettingsDraft::from_settings(&defaults);
        draft.workers = "16".to_string();
        draft.concurrent = "4".to_string();
        draft.cmd_args = "   ".to_string();
        draft.ui_close_to_tray = false;
        draft.launch_on_startup = true;
        draft.category_url_rule_precedence = false;
        draft.set_file_time_from_server = true;
        let parsed = draft.into_settings().expect("valid draft");
        assert_eq!(parsed.max_workers, 16);
        assert_eq!(parsed.max_concurrent_downloads, 4);
        assert_eq!(parsed.completion_cmd_args, "%path%");
        assert!(!parsed.ui_close_to_tray);
        assert!(parsed.launch_on_startup);
        assert!(!parsed.category_url_rule_precedence);
        assert!(parsed.set_file_time_from_server);

        let mut bad_workers = SettingsDraft::from_settings(&defaults);
        bad_workers.workers = "33".to_string();
        assert!(bad_workers.into_settings().is_err());

        let mut bad_concurrent = SettingsDraft::from_settings(&defaults);
        bad_concurrent.concurrent = "0".to_string();
        assert!(bad_concurrent.into_settings().is_err());

        let mut bad_proxy = SettingsDraft::from_settings(&defaults);
        bad_proxy.proxy_mode = ProxyMode::Manual;
        bad_proxy.proxy_url = "   ".to_string();
        assert!(bad_proxy.into_settings().is_err());

        let mut bad_cmd = SettingsDraft::from_settings(&defaults);
        bad_cmd.completion_cmd_enabled = true;
        bad_cmd.cmd_path = "".to_string();
        assert!(bad_cmd.into_settings().is_err());

        let mut bad_dir = SettingsDraft::from_settings(&defaults);
        bad_dir.dir = "   ".to_string();
        assert!(bad_dir.into_settings().is_err());

        let mut bad_retry = SettingsDraft::from_settings(&defaults);
        bad_retry.auto_retry = "11".to_string();
        assert!(bad_retry.into_settings().is_err());

        let mut bad_speed = SettingsDraft::from_settings(&defaults);
        bad_speed.speed_limit = "abc".to_string();
        assert!(bad_speed.into_settings().is_err());
    }
}
