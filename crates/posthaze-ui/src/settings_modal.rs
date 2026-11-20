//! Comprehensive multi-tab Settings modal (`General`, `Downloads`, `Network`, `Automation`, `Interface`, `Reset`).

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{CollisionPolicy, DeleteDefaultAction, ProxyMode, Settings, TableDensity};

use crate::grid_cells::{
    CellBorder, GridTone, grid_boxed_button, grid_cell_button, grid_modal_footer,
    grid_section_header,
};
use crate::modals::{labeled_input, modal_backdrop};
use crate::theme::*;

/// Navigation tabs inside the comprehensive Settings modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    General,
    Downloads,
    Network,
    Notifications,
    Automation,
    Interface,
    Reset,
}

impl SettingsTab {
    pub const ALL: [Self; 7] = [
        Self::General,
        Self::Interface,
        Self::Downloads,
        Self::Network,
        Self::Notifications,
        Self::Automation,
        Self::Reset,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Downloads => "Downloads",
            Self::Network => "Network",
            Self::Notifications => "Notifications",
            Self::Automation => "Automation",
            Self::Interface => "Interface",
            Self::Reset => "Reset",
        }
    }

    pub fn subtitle(self) -> &'static str {
        match self {
            Self::General => "Default save location and folder sorting.",
            Self::Downloads => "File collisions, removal behavior, and automatic retries.",
            Self::Network => "Proxy routing, bandwidth cap, and connection limits.",
            Self::Notifications => "Windows desktop alerts for queue and transfer events.",
            Self::Automation => "External commands and antivirus scans run after completion.",
            Self::Interface => "Row interactions, Pre-Flight actions, and display scale.",
            Self::Reset => "Restore all preferences to factory defaults.",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Folder,
            Self::Downloads => IconName::Download,
            Self::Network => IconName::Globe,
            Self::Notifications => IconName::Bell,
            Self::Automation => IconName::ShieldCheck,
            Self::Interface => IconName::LayoutDashboard,
            Self::Reset => IconName::RotateCcw,
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
    pub collision_policy: CollisionPolicy,
    pub delete_action_default: DeleteDefaultAction,
    pub completion_cmd_enabled: bool,
    pub notify_on_added: bool,
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
            collision_policy: settings.collision_policy,
            delete_action_default: settings.delete_action_default,
            completion_cmd_enabled: settings.completion_cmd_enabled,
            notify_on_added: settings.notify_on_added,
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
    pub draft: SettingsDraft,
    pub error: Option<String>,
}

impl SettingsModalState {
    pub fn from_settings(settings: &Settings, window: &mut Window, cx: &mut App) -> Self {
        let draft = SettingsDraft::from_settings(settings);
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
            draft,
            error: None,
        }
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
                crate::theme::apply_theme_with_accent(
                    &self.draft.ui_theme,
                    self.draft.ui_accent_override.as_deref(),
                    Some(window),
                    cx,
                );
            }
            SettingsAction::SetTheme(theme_id) => {
                let palette = crate::theme::apply_theme_with_accent(
                    &theme_id,
                    self.draft.ui_accent_override.as_deref(),
                    Some(window),
                    cx,
                );
                self.draft.ui_theme = palette.id.to_string();
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
                    });
                    if let Ok(pretty) = serde_json::to_string_pretty(&payload) {
                        cx.write_to_clipboard(ClipboardItem::new_string(pretty));
                    }
                }
            }
            SettingsAction::ResetToDefaults => {
                *self = Self::from_settings(&Settings::default(), window, cx);
                self.active_tab = SettingsTab::Reset;
                crate::theme::apply_theme_with_accent(
                    &self.draft.ui_theme,
                    self.draft.ui_accent_override.as_deref(),
                    Some(window),
                    cx,
                );
            }
            SettingsAction::OpenLogsFolder => {}
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
    ToggleCompletionCmd,
    ApplyDefenderPreset,
    ToggleNotifyAdded,
    ToggleNotifyCompleted,
    ToggleNotifyFailed,
    ToggleNotifyOnlyInactive,
    ToggleClickOpensInspector,
    ToggleShowAddPaused,
    ToggleLockFileExtension,
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
    ResetToDefaults,
}

pub(crate) fn render_settings_modal<F1, F2, F3>(
    state: &SettingsModalState,
    on_action: F1,
    on_cancel: F2,
    on_save: F3,
) -> AnyElement
where
    F1: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
    F2: Fn(&mut Window, &mut App) + Clone + 'static,
    F3: Fn(&mut Window, &mut App) + 'static,
{
    let on_backdrop_close = on_cancel.clone();

    let body = match state.active_tab {
        SettingsTab::General => render_general_tab(state, on_action.clone()),
        SettingsTab::Downloads => render_downloads_tab(state, on_action.clone()),
        SettingsTab::Network => render_network_tab(state, on_action.clone()),
        SettingsTab::Notifications => render_notifications_tab(state, on_action.clone()),
        SettingsTab::Automation => render_automation_tab(state, on_action.clone()),
        SettingsTab::Interface => render_interface_tab(state, on_action.clone()),
        SettingsTab::Reset => render_reset_tab(on_action.clone()),
    };

    let card = div()
        .w(px(780.0))
        .h(px(540.0))
        .max_w_full()
        .max_h_full()
        .bg(graphite_bg())
        .border_1()
        .border_color(graphite_border())
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .h(px(36.0))
                .pl_5()
                .border_b_1()
                .border_color(graphite_border())
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_HEADING)
                        .line_height(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(silver_text())
                        .child("SETTINGS"),
                )
                .child(grid_cell_button(
                    "settings-x-close",
                    Some(IconName::X),
                    None,
                    Some("Close"),
                    GridTone::Graphite,
                    CellBorder::Left,
                    true,
                    {
                        let cancel = on_cancel.clone();
                        move |_, window, cx| cancel(window, cx)
                    },
                )),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .w(px(176.0))
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
                            div()
                                .id(SharedString::from(format!("settings-tab-{}", tab.label())))
                                .h(px(34.0))
                                .w_full()
                                .border_b_1()
                                .border_color(graphite_border())
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .bg(if active {
                                    graphite_elevated()
                                } else {
                                    graphite_bg()
                                })
                                .hover(|s| s.bg(graphite_elevated()))
                                .on_click(move |_, window, cx| {
                                    act(SettingsAction::SelectTab(tab), window, cx);
                                })
                                .child(div().w(px(3.0)).h_full().mr_3().bg(if active {
                                    selenium_amber()
                                } else {
                                    Hsla::transparent_black()
                                }))
                                .child(Icon::new(tab.icon()).size(px(13.0)).text_color(
                                    if active {
                                        selenium_amber()
                                    } else {
                                        silver_muted()
                                    },
                                ))
                                .child(
                                    div()
                                        .ml_2()
                                        .font_family(MONO_FONT)
                                        .text_size(TYPE_MICRO)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(if active { silver_text() } else { silver_muted() })
                                        .child(tab.label().to_uppercase()),
                                )
                        })),
                )
                .child(
                    div()
                        .id("settings-scroll-pane")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .px_5()
                        .py_4()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .gap_3()
                        .child(
                            div()
                                .pb_2p5()
                                .border_b_1()
                                .border_color(graphite_border())
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .font_family(MONO_FONT)
                                        .text_size(TYPE_HEADING)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(silver_text())
                                        .child(state.active_tab.label().to_uppercase()),
                                )
                                .child(
                                    div()
                                        .text_xs()
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
                    Some("CANCEL"),
                    None,
                    GridTone::Graphite,
                    CellBorder::Left,
                    false,
                    move |_, window, cx| on_cancel(window, cx),
                ))
                .child(grid_cell_button(
                    "settings-save",
                    Some(IconName::Check),
                    Some("SAVE SETTINGS"),
                    None,
                    GridTone::Primary,
                    CellBorder::Left,
                    false,
                    move |_, window, cx| on_save(window, cx),
                )),
        );

    modal_backdrop(card, on_backdrop_close)
}

fn section_heading(text: &'static str) -> impl IntoElement {
    div()
        .border_1()
        .border_color(graphite_border())
        .child(grid_section_header(text.to_uppercase(), GridTone::Graphite))
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
                .text_xs()
                .line_height(px(16.0))
                .text_color(silver_muted())
                .child(description),
        )
}

fn setting_row(
    title: &'static str,
    description: &'static str,
    control: impl IntoElement,
) -> impl IntoElement {
    div()
        .py_3()
        .border_b_1()
        .border_color(graphite_border())
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(setting_text(title, description))
        .child(div().flex_shrink_0().child(control))
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
                .bg(graphite_elevated())
                .border_1()
                .border_color(graphite_border())
                .flex()
                .items_center()
                .font_family(MONO_FONT)
                .text_size(TYPE_BODY)
                .child(Input::new(state).appearance(false)),
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
) -> impl IntoElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + 'static,
{
    div()
        .id(id)
        .py_1p5()
        .flex()
        .items_start()
        .gap_2p5()
        .cursor_pointer()
        .on_click(move |_, window, cx| on_action(action.clone(), window, cx))
        .child(
            div()
                .mt(px(2.0))
                .w(px(16.0))
                .h(px(16.0))
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
                .justify_center()
                .when(checked, |el| {
                    el.child(
                        Icon::new(IconName::Check)
                            .size(px(11.0))
                            .text_color(graphite_bg()),
                    )
                }),
        )
        .child(setting_text(title, description))
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
    let browse_action = on_action.clone();
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .w_full()
                .flex()
                .items_stretch()
                .child(div().flex_1().min_w_0().child(labeled_input("Save Folder", &state.dir_input)))
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
                            Some("BROWSE"),
                            Some("Choose default download folder"),
                            GridTone::Graphite,
                            CellBorder::Left,
                            false,
                            move |_, window, cx| browse_action(SettingsAction::BrowseSaveFolder, window, cx),
                        )),
                ),
        )
        .child(toggle_row(
            "settings-auto-categorize",
            "Sort by File Type",
            "Route files into Archives, Video, Audio, Documents, and Programs subfolders.",
            state.draft.auto_categorize_folders,
            SettingsAction::ToggleAutoCategorize,
            on_action,
        ))
        .into_any_element()
}

fn render_downloads_tab<F>(state: &SettingsModalState, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    div()
        .flex()
        .flex_col()
        .child(setting_row(
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
        ))
        .child(setting_row(
            "Remove Button Action",
            "Default behavior for the row Trash icon (right-click always shows both).",
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
                    "Delete File",
                    state.draft.delete_action_default == DeleteDefaultAction::DeleteFile,
                    SettingsAction::SetDeleteDefault(DeleteDefaultAction::DeleteFile),
                    on_action,
                )),
        ))
        .child(setting_row(
            "Auto-Retry on Error",
            "Automatic retry attempts (0–10) after a transient failure.",
            compact_input(&state.auto_retry_input, px(88.0), Some("retries")),
        ))
        .into_any_element()
}

fn render_network_tab<F>(state: &SettingsModalState, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    div()
        .flex()
        .flex_col()
        .child(setting_row(
            "Proxy",
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
        ))
        .when(state.draft.proxy_mode == ProxyMode::Manual, |el| {
            el.child(
                div()
                    .py_2p5()
                    .border_b_1()
                    .border_color(graphite_border())
                    .child(labeled_input("Proxy URL", &state.proxy_url_input)),
            )
        })
        .child(
            div()
                .py_2p5()
                .border_b_1()
                .border_color(graphite_border())
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
                            on_action.clone(),
                        )),
                ),
        )
        .child(setting_row(
            "Workers per Download",
            "Maximum concurrent byte-range segments per file (1–32).",
            compact_input(&state.workers_input, px(88.0), Some("workers")),
        ))
        .child(setting_row(
            "Parallel Downloads",
            "Maximum active downloads running at the same time (1+).",
            compact_input(&state.concurrent_input, px(88.0), Some("slots")),
        ))
        .child(
            div().pt_2p5().child(toggle_row(
                "settings-sync",
                "Flush File Buffers on Finish",
                "Call sync_all before finalizing each download to protect against power loss.",
                state.draft.sync_on_complete,
                SettingsAction::ToggleSyncOnComplete,
                on_action,
            )),
        )
        .into_any_element()
}

fn render_notifications_tab<F>(state: &SettingsModalState, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(toggle_row(
            "notify-added",
            "Download Added",
            "Show a Windows notification when a new download is queued.",
            state.draft.notify_on_added,
            SettingsAction::ToggleNotifyAdded,
            on_action.clone(),
        ))
        .child(toggle_row(
            "notify-completed",
            "Download Completed",
            "Show a Windows notification when a file finishes.",
            state.draft.notify_on_completed,
            SettingsAction::ToggleNotifyCompleted,
            on_action.clone(),
        ))
        .child(toggle_row(
            "notify-failed",
            "Download Failed",
            "Show a Windows notification if a Download stops with an error.",
            state.draft.notify_on_failed,
            SettingsAction::ToggleNotifyFailed,
            on_action.clone(),
        ))
        .child(div().h(px(1.0)).w_full().my_1().bg(graphite_border()))
        .child(toggle_row(
            "notify-inactive",
            "Only When Window Is Unfocused",
            "Suppress notifications while you are actively viewing Posthaze.",
            state.draft.notify_only_inactive,
            SettingsAction::ToggleNotifyOnlyInactive,
            on_action,
        ))
        .into_any_element()
}

fn render_automation_tab<F>(state: &SettingsModalState, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(section_heading("Run After Completion"))
                .child({
                    let act = on_action.clone();
                    grid_boxed_button(
                        "settings-defender-preset",
                        px(28.0),
                        Some(IconName::ShieldCheck),
                        Some("USE WINDOWS DEFENDER"),
                        None,
                        GridTone::Graphite,
                        true,
                        move |_, window, cx| {
                            act(SettingsAction::ApplyDefenderPreset, window, cx);
                        },
                    )
                }),
        )
        .child(toggle_row(
            "cmd-enabled",
            "Launch Program on Finish",
            "Run an antivirus scanner or script when a download completes.",
            state.draft.completion_cmd_enabled,
            SettingsAction::ToggleCompletionCmd,
            on_action,
        ))
        .when(state.draft.completion_cmd_enabled, |el| {
            el.child(
                div()
                    .pt_1()
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .child(labeled_input("Program Path", &state.cmd_path_input))
                    .child(labeled_input(
                        "Arguments (%path% = completed file)",
                        &state.cmd_args_input,
                    )),
            )
        })
        .into_any_element()
}

fn render_interface_tab<F>(state: &SettingsModalState, on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(section_heading("Appearance Theme"))
                .child(
                    div()
                        .text_size(TYPE_MICRO)
                        .text_color(silver_muted())
                        .child("Select a color theme (previews immediately; Cancel reverts)."),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .children(BUILTIN_THEMES.iter().map(|palette| {
                            let is_selected = state.draft.ui_theme == palette.id
                                || (find_theme(&state.draft.ui_theme).id == palette.id
                                    && state.draft.ui_theme.is_empty());
                            let theme_id = palette.id.to_string();
                            let act = on_action.clone();
                            let tone_label = match palette.tone {
                                PaletteTone::Dark => "DARK",
                                PaletteTone::Light => "LIGHT",
                            };
                            div()
                                .id(SharedString::from(format!("theme-{}", palette.id)))
                                .w(px(236.0))
                                .px_3()
                                .py_2()
                                .cursor_pointer()
                                .bg(if is_selected {
                                    graphite_elevated()
                                } else {
                                    graphite_bg()
                                })
                                .border_1()
                                .border_color(if is_selected {
                                    selenium_amber()
                                } else {
                                    graphite_border()
                                })
                                .hover(|s| s.bg(graphite_elevated()))
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
                                                .text_size(TYPE_BODY)
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(silver_text())
                                                .child(palette.name),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .items_center()
                                                .gap_1()
                                                .child(swatch_dot(palette.graphite_bg))
                                                .child(swatch_dot(palette.paper_bg))
                                                .child(swatch_dot(palette.accent))
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
                                        .child(palette.description),
                                )
                        })),
                ),
        )
        .child(section_heading("Accent Override"))
        .child(
            div()
                .pb_2p5()
                .border_b_1()
                .border_color(graphite_border())
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
                            graphite_elevated()
                        } else {
                            graphite_bg()
                        })
                        .hover(|s| s.bg(graphite_elevated()))
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
                            graphite_elevated()
                        } else {
                            graphite_bg()
                        })
                        .hover(|s| s.bg(graphite_elevated()))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if active { silver_text() } else { silver_muted() })
                        .on_click(move |_, window, cx| {
                            act(SettingsAction::SetAccentOverride(Some(pid.clone())), window, cx);
                        })
                        .child(swatch_dot(preset.accent))
                        .child(preset.name.to_uppercase())
                })),
        )
        .child(section_heading("Table Density & Columns"))
        .child(setting_row(
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
        ))
        .child(setting_row(
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
        ))
        .child(setting_row(
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
        ))
        .child(section_heading("Workspace & Chrome"))
        .child(
            div()
                .pb_2p5()
                .flex()
                .flex_col()
                .gap_2()
                .child(toggle_row(
                    "ui-click-insp",
                    "Open Byte-Map Inspector on Click",
                    "Automatically expand the bottom inspector when selecting a download row.",
                    state.draft.ui_click_row_opens_inspector,
                    SettingsAction::ToggleClickOpensInspector,
                    on_action.clone(),
                ))
                .child(toggle_row(
                    "ui-add-paused",
                    "Show \"Add Paused\" in Pre-Flight",
                    "Include the Add Paused button alongside Download Now.",
                    state.draft.ui_show_add_paused,
                    SettingsAction::ToggleShowAddPaused,
                    on_action.clone(),
                ))
                .child(toggle_row(
                    "ui-lock-ext",
                    "Lock File Extension in Pre-Flight",
                    "Separate and lock the probed file extension so renaming cannot accidentally remove it.",
                    state.draft.ui_lock_file_extension,
                    SettingsAction::ToggleLockFileExtension,
                    on_action.clone(),
                ))
                .child(toggle_row(
                    "ui-start-rail",
                    "Start with Sidebar Collapsed",
                    "Launch into the compact left icon rail instead of the expanded filter rail.",
                    state.draft.ui_start_sidebar_collapsed,
                    SettingsAction::ToggleStartSidebarCollapsed,
                    on_action.clone(),
                ))
                .child(toggle_row(
                    "ui-titlebar-tel",
                    "Show Live Telemetry in Titlebar",
                    "Display active transfer slots, throughput, and queue count in the top bar.",
                    state.draft.ui_show_titlebar_telemetry,
                    SettingsAction::ToggleTitlebarTelemetry,
                    on_action.clone(),
                ))
                .child(toggle_row(
                    "ui-toolbar-labels",
                    "Show Toolbar Button Labels",
                    "Display text labels on top-bar action cells (uncheck for icon-only cells).",
                    state.draft.ui_show_toolbar_labels,
                    SettingsAction::ToggleToolbarLabels,
                    on_action.clone(),
                ))
                .child(toggle_row(
                    "ui-minimap",
                    "Show Scrollbar Minimap",
                    "Highlight Active and Failed row positions along the table scrollbar rail.",
                    state.draft.ui_show_scrollbar_minimap,
                    SettingsAction::ToggleScrollbarMinimap,
                    on_action,
                )),
        )
        .into_any_element()
}

fn render_reset_tab<F>(on_action: F) -> AnyElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + Clone + 'static,
{
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .p_3p5()
                .bg(graphite_elevated())
                .border_1()
                .border_color(graphite_border())
                .text_xs()
                .line_height(px(18.0))
                .text_color(silver_muted())
                .child("Export your active configuration as JSON, or restore save folder, collision and removal rules, proxy, speed limit, notifications, post-download command, and interface customization to factory defaults. Downloads in your Queue are kept untouched."),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .child({
                    let act = on_action.clone();
                    grid_boxed_button(
                        "settings-copy-json",
                        px(32.0),
                        Some(IconName::Copy),
                        Some("COPY CONFIG JSON"),
                        Some("Copy current settings as formatted JSON"),
                        GridTone::Graphite,
                        false,
                        move |_, window, cx| {
                            act(SettingsAction::CopyConfigJson, window, cx);
                        },
                    )
                })
                .child({
                    let act = on_action.clone();
                    grid_boxed_button(
                        "settings-open-logs",
                        px(32.0),
                        Some(IconName::FolderOpen),
                        Some("OPEN LOGS FOLDER"),
                        Some("Open the diagnostic logs directory in Windows Explorer"),
                        GridTone::Graphite,
                        false,
                        move |_, window, cx| {
                            act(SettingsAction::OpenLogsFolder, window, cx);
                        },
                    )
                })
                .child(grid_boxed_button(
                    "settings-reset-defaults",
                    px(32.0),
                    Some(IconName::RotateCcw),
                    Some("RESTORE DEFAULT SETTINGS"),
                    None,
                    GridTone::Danger,
                    false,
                    move |_, window, cx| {
                        on_action(SettingsAction::ResetToDefaults, window, cx);
                    },
                )),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::SettingsDraft;
    use posthaze_engine::{ProxyMode, Settings};

    #[test]
    fn settings_draft_round_trips_and_delegates_validation() {
        let defaults = Settings::default();
        let mut draft = SettingsDraft::from_settings(&defaults);
        draft.workers = "16".to_string();
        draft.concurrent = "4".to_string();
        draft.cmd_args = "   ".to_string();
        let parsed = draft.into_settings().expect("valid draft");
        assert_eq!(parsed.max_workers, 16);
        assert_eq!(parsed.max_concurrent_downloads, 4);
        assert_eq!(parsed.completion_cmd_args, "%path%");

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
