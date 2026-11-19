//! Comprehensive multi-tab Settings modal (`General`, `Downloads`, `Network`, `Automation`, `Interface`, `Reset`).

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{CollisionPolicy, DeleteDefaultAction, ProxyMode, Settings};

use crate::modals::{labeled_input, modal_backdrop};
use crate::theme::*;

/// Navigation tabs inside the comprehensive Settings modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    General,
    Downloads,
    Network,
    Automation,
    Interface,
    Reset,
}

impl SettingsTab {
    pub const ALL: [Self; 6] = [
        Self::General,
        Self::Downloads,
        Self::Network,
        Self::Automation,
        Self::Interface,
        Self::Reset,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Downloads => "Downloads",
            Self::Network => "Network",
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
            Self::Automation => "Windows notifications and post-download commands.",
            Self::Interface => "Row interactions, Pre-Flight actions, and display scale.",
            Self::Reset => "Restore all preferences to factory defaults.",
        }
    }

    pub fn icon(self) -> IconName {
        match self {
            Self::General => IconName::Folder,
            Self::Downloads => IconName::Download,
            Self::Network => IconName::Globe,
            Self::Automation => IconName::Bell,
            Self::Interface => IconName::LayoutDashboard,
            Self::Reset => IconName::RotateCcw,
        }
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
    pub error: Option<String>,
}

impl SettingsModalState {
    pub fn from_settings(settings: &Settings, window: &mut Window, cx: &mut App) -> Self {
        let dir_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("Default save folder");
            st.set_value(
                settings.default_output_dir.to_string_lossy().to_string(),
                window,
                cx,
            );
            st
        });
        let workers_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("1–32");
            st.set_value(settings.max_workers.to_string(), window, cx);
            st
        });
        let concurrent_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("1+");
            st.set_value(settings.max_concurrent_downloads.to_string(), window, cx);
            st
        });
        let proxy_url_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx)
                .placeholder("http://127.0.0.1:8080 or socks5://127.0.0.1:1080");
            st.set_value(settings.proxy_url.clone(), window, cx);
            st
        });
        let speed_limit_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("0");
            st.set_value(settings.download_speed_limit_kib.to_string(), window, cx);
            st
        });
        let auto_retry_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("0–10");
            st.set_value(settings.auto_retry_count.to_string(), window, cx);
            st
        });
        let cmd_path_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("C:\\Path\\To\\Program.exe");
            st.set_value(settings.completion_cmd_path.clone(), window, cx);
            st
        });
        let cmd_args_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("%path%");
            st.set_value(settings.completion_cmd_args.clone(), window, cx);
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
            error: None,
        }
    }

    pub fn handle_action(&mut self, action: SettingsAction, window: &mut Window, cx: &mut App) {
        match action {
            SettingsAction::SelectTab(tab) => self.active_tab = tab,
            SettingsAction::SetProxyMode(mode) => self.proxy_mode = mode,
            SettingsAction::SetSpeedPreset(kib) => {
                self.speed_limit_input.update(cx, |st, cx| {
                    st.set_value(kib.to_string(), window, cx);
                });
            }
            SettingsAction::SetCollisionPolicy(policy) => self.collision_policy = policy,
            SettingsAction::SetDeleteDefault(def) => self.delete_action_default = def,
            SettingsAction::SetZoom(zoom) => self.ui_zoom_percent = zoom,
            SettingsAction::ToggleSyncOnComplete => {
                self.sync_on_complete = !self.sync_on_complete;
            }
            SettingsAction::ToggleAutoCategorize => {
                self.auto_categorize_folders = !self.auto_categorize_folders;
            }
            SettingsAction::ToggleCompletionCmd => {
                self.completion_cmd_enabled = !self.completion_cmd_enabled;
            }
            SettingsAction::ApplyDefenderPreset => {
                self.completion_cmd_enabled = true;
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
            SettingsAction::ToggleNotifyAdded => self.notify_on_added = !self.notify_on_added,
            SettingsAction::ToggleNotifyCompleted => {
                self.notify_on_completed = !self.notify_on_completed;
            }
            SettingsAction::ToggleNotifyFailed => self.notify_on_failed = !self.notify_on_failed,
            SettingsAction::ToggleNotifyOnlyInactive => {
                self.notify_only_inactive = !self.notify_only_inactive;
            }
            SettingsAction::ToggleClickOpensInspector => {
                self.ui_click_row_opens_inspector = !self.ui_click_row_opens_inspector;
            }
            SettingsAction::ToggleShowAddPaused => {
                self.ui_show_add_paused = !self.ui_show_add_paused;
            }
            SettingsAction::ResetToDefaults => {
                *self = Self::from_settings(&Settings::default(), window, cx);
                self.active_tab = SettingsTab::Reset;
            }
        }
    }

    pub fn apply_to_settings(&self, cx: &App) -> Result<Settings, String> {
        let dir = self.dir_input.read(cx).value().trim().to_string();
        if dir.is_empty() {
            return Err("Save folder cannot be empty.".to_string());
        }
        let max_workers = self
            .workers_input
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .map_err(|_| "Workers per download must be an integer between 1 and 32.".to_string())?;
        if !(1..=32).contains(&max_workers) {
            return Err("Workers per download must be between 1 and 32.".to_string());
        }
        let max_concurrent_downloads = self
            .concurrent_input
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .map_err(|_| "Parallel downloads must be at least 1.".to_string())?;
        if max_concurrent_downloads == 0 {
            return Err("Parallel downloads must be at least 1.".to_string());
        }
        let proxy_url = self.proxy_url_input.read(cx).value().trim().to_string();
        if self.proxy_mode == ProxyMode::Manual && proxy_url.is_empty() {
            return Err("Manual proxy mode requires a proxy URL.".to_string());
        }
        let download_speed_limit_kib = self
            .speed_limit_input
            .read(cx)
            .value()
            .trim()
            .parse::<u64>()
            .map_err(|_| "Speed limit must be a non-negative integer (KiB/s).".to_string())?;
        let auto_retry_count = self
            .auto_retry_input
            .read(cx)
            .value()
            .trim()
            .parse::<u32>()
            .map_err(|_| "Auto-retry count must be between 0 and 10.".to_string())?;
        if auto_retry_count > 10 {
            return Err("Auto-retry count cannot exceed 10.".to_string());
        }

        let completion_cmd_path = self.cmd_path_input.read(cx).value().trim().to_string();
        let completion_cmd_args = self.cmd_args_input.read(cx).value().trim().to_string();
        if self.completion_cmd_enabled && completion_cmd_path.is_empty() {
            return Err("Program path is required when post-download command is enabled.".to_string());
        }

        Ok(Settings {
            default_output_dir: PathBuf::from(dir),
            max_workers,
            max_concurrent_downloads,
            sync_on_complete: self.sync_on_complete,
            proxy_mode: self.proxy_mode,
            proxy_url,
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
        })
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
        SettingsTab::General => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(labeled_input("Save Folder", &state.dir_input))
            .child(toggle_row(
                "settings-auto-categorize",
                "Sort by File Type",
                "Route files into Archives, Video, Audio, Documents, and Programs subfolders.",
                state.auto_categorize_folders,
                SettingsAction::ToggleAutoCategorize,
                on_action.clone(),
            )),

        SettingsTab::Downloads => div()
            .flex()
            .flex_col()
            .child(setting_row(
                "When File Exists",
                "Action when the destination filename already exists on disk.",
                segmented_track()
                    .child(choice_btn(
                        "coll-rename",
                        "Rename Copy",
                        state.collision_policy == CollisionPolicy::Rename,
                        SettingsAction::SetCollisionPolicy(CollisionPolicy::Rename),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "coll-overwrite",
                        "Overwrite",
                        state.collision_policy == CollisionPolicy::Overwrite,
                        SettingsAction::SetCollisionPolicy(CollisionPolicy::Overwrite),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "coll-ask",
                        "Ask Each Time",
                        state.collision_policy == CollisionPolicy::Ask,
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
                        state.delete_action_default == DeleteDefaultAction::Ask,
                        SettingsAction::SetDeleteDefault(DeleteDefaultAction::Ask),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "del-remove",
                        "Remove Entry",
                        state.delete_action_default == DeleteDefaultAction::Remove,
                        SettingsAction::SetDeleteDefault(DeleteDefaultAction::Remove),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "del-file",
                        "Delete File",
                        state.delete_action_default == DeleteDefaultAction::DeleteFile,
                        SettingsAction::SetDeleteDefault(DeleteDefaultAction::DeleteFile),
                        on_action.clone(),
                    )),
            ))
            .child(setting_row(
                "Auto-Retry on Error",
                "Automatic retry attempts (0–10) after a transient failure.",
                compact_input(&state.auto_retry_input, px(88.0), Some("retries")),
            )),

        SettingsTab::Network => div()
            .flex()
            .flex_col()
            .child(setting_row(
                "Proxy",
                "How HTTP/HTTPS requests are routed.",
                segmented_track()
                    .child(choice_btn(
                        "proxy-sys",
                        "System",
                        state.proxy_mode == ProxyMode::System,
                        SettingsAction::SetProxyMode(ProxyMode::System),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "proxy-none",
                        "Direct",
                        state.proxy_mode == ProxyMode::None,
                        SettingsAction::SetProxyMode(ProxyMode::None),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "proxy-manual",
                        "Manual",
                        state.proxy_mode == ProxyMode::Manual,
                        SettingsAction::SetProxyMode(ProxyMode::Manual),
                        on_action.clone(),
                    )),
            ))
            .when(state.proxy_mode == ProxyMode::Manual, |el| {
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
                    state.sync_on_complete,
                    SettingsAction::ToggleSyncOnComplete,
                    on_action.clone(),
                )),
            ),

        SettingsTab::Automation => div()
            .flex()
            .flex_col()
            .gap_2()
            .child(section_heading("Desktop Notifications"))
            .child(toggle_row(
                "notify-added",
                "Download Added",
                "Show a Windows notification when a new download is queued.",
                state.notify_on_added,
                SettingsAction::ToggleNotifyAdded,
                on_action.clone(),
            ))
            .child(toggle_row(
                "notify-completed",
                "Download Completed",
                "Show a Windows notification when a file finishes.",
                state.notify_on_completed,
                SettingsAction::ToggleNotifyCompleted,
                on_action.clone(),
            ))
            .child(toggle_row(
                "notify-failed",
                "Download Failed",
                "Show a Windows notification if a transfer stops with an error.",
                state.notify_on_failed,
                SettingsAction::ToggleNotifyFailed,
                on_action.clone(),
            ))
            .child(toggle_row(
                "notify-inactive",
                "Only When Window Is Unfocused",
                "Suppress notifications while you are actively viewing Posthaze.",
                state.notify_only_inactive,
                SettingsAction::ToggleNotifyOnlyInactive,
                on_action.clone(),
            ))
            .child(div().h(px(1.0)).w_full().my_1().bg(graphite_border()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_heading("Run After Completion"))
                    .child({
                        let act = on_action.clone();
                        Button::new("settings-defender-preset")
                            .small()
                            .icon(IconName::ShieldCheck)
                            .label("Use Windows Defender")
                            .on_click(move |_, window, cx| {
                                act(SettingsAction::ApplyDefenderPreset, window, cx);
                            })
                    }),
            )
            .child(toggle_row(
                "cmd-enabled",
                "Launch Program on Finish",
                "Run an antivirus scanner or script when a download completes.",
                state.completion_cmd_enabled,
                SettingsAction::ToggleCompletionCmd,
                on_action.clone(),
            ))
            .when(state.completion_cmd_enabled, |el| {
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
            }),

        SettingsTab::Interface => div()
            .flex()
            .flex_col()
            .child(
                div()
                    .pb_2p5()
                    .border_b_1()
                    .border_color(graphite_border())
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .child(toggle_row(
                        "ui-click-insp",
                        "Open Byte-Map Inspector on Click",
                        "Automatically expand the bottom inspector when selecting a download row.",
                        state.ui_click_row_opens_inspector,
                        SettingsAction::ToggleClickOpensInspector,
                        on_action.clone(),
                    ))
                    .child(toggle_row(
                        "ui-add-paused",
                        "Show \"Add Paused\" in Pre-Flight",
                        "Include the Add Paused button alongside Download Now.",
                        state.ui_show_add_paused,
                        SettingsAction::ToggleShowAddPaused,
                        on_action.clone(),
                    )),
            )
            .child(setting_row(
                "Interface Scale",
                "Adjust overall text and control sizing.",
                segmented_track()
                    .child(choice_btn(
                        "zoom-90",
                        "90%",
                        state.ui_zoom_percent == 90,
                        SettingsAction::SetZoom(90),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "zoom-100",
                        "100%",
                        state.ui_zoom_percent == 100,
                        SettingsAction::SetZoom(100),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "zoom-110",
                        "110%",
                        state.ui_zoom_percent == 110,
                        SettingsAction::SetZoom(110),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "zoom-125",
                        "125%",
                        state.ui_zoom_percent == 125,
                        SettingsAction::SetZoom(125),
                        on_action.clone(),
                    )),
            )),

        SettingsTab::Reset => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .p_3p5()
                    .bg(graphite_elevated())
                    .border_1()
                    .border_color(graphite_border())
                    .rounded(px(4.0))
                    .text_xs()
                    .line_height(px(18.0))
                    .text_color(silver_muted())
                    .child("Restores save folder, collision and removal rules, proxy, speed limit, notifications, post-download command, and interface scale to factory defaults. Downloads in your Queue are kept untouched."),
            )
            .child(
                div().flex().child({
                    let act = on_action.clone();
                    Button::new("settings-reset-defaults")
                        .small()
                        .danger()
                        .icon(IconName::RotateCcw)
                        .label("Restore Default Settings")
                        .on_click(move |_, window, cx| {
                            act(SettingsAction::ResetToDefaults, window, cx);
                        })
                }),
            ),
    };

    let card = div()
        .w(px(780.0))
        .h(px(540.0))
        .max_w_full()
        .max_h_full()
        .bg(graphite_bg())
        .border_1()
        .border_color(graphite_border())
        .rounded(px(6.0))
        .flex()
        .flex_col()
        .overflow_hidden()
        .child(
            div()
                .px_5()
                .py_3()
                .border_b_1()
                .border_color(graphite_border())
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(TYPE_HEADING)
                        .line_height(px(22.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(silver_text())
                        .child("Settings"),
                )
                .child(
                    Button::new("settings-x-close")
                        .small()
                        .ghost()
                        .icon(IconName::X)
                        .on_click({
                            let cancel = on_cancel.clone();
                            move |_, window, cx| cancel(window, cx)
                        }),
                ),
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
                        .bg(graphite_elevated())
                        .border_r_1()
                        .border_color(graphite_border())
                        .p_2p5()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(SettingsTab::ALL.into_iter().map(|tab| {
                            let active = state.active_tab == tab;
                            let act = on_action.clone();
                            div()
                                .id(SharedString::from(format!("settings-tab-{}", tab.label())))
                                .px_3()
                                .py_2()
                                .rounded(px(4.0))
                                .flex()
                                .items_center()
                                .gap_2p5()
                                .cursor_pointer()
                                .bg(if active {
                                    paper_hover()
                                } else {
                                    graphite_elevated()
                                })
                                .text_size(TYPE_BODY)
                                .font_weight(if active {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if active { silver_text() } else { silver_muted() })
                                .hover(|s| s.bg(graphite_border()))
                                .on_click(move |_, window, cx| {
                                    act(SettingsAction::SelectTab(tab), window, cx);
                                })
                                .child(Icon::new(tab.icon()).size(px(14.0)).text_color(
                                    if active {
                                        selenium_amber()
                                    } else {
                                        silver_muted()
                                    },
                                ))
                                .child(tab.label())
                        })),
                )
                .child(
                    div()
                        .id("settings-scroll-pane")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .px_6()
                        .py_5()
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
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_size(TYPE_HEADING)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(silver_text())
                                        .child(state.active_tab.label()),
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
                    .mx_5()
                    .mb_2()
                    .px_3()
                    .py_2()
                    .rounded(px(4.0))
                    .bg(rgba(0xEF444420))
                    .border_1()
                    .border_color(errata_vermilion())
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .text_color(errata_vermilion())
                    .child(err),
            )
        })
        .child(
            div()
                .px_5()
                .py_3()
                .border_t_1()
                .border_color(graphite_border())
                .flex()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("settings-cancel")
                        .small()
                        .icon(IconName::X)
                        .label("Cancel")
                        .on_click(move |_, window, cx| on_cancel(window, cx)),
                )
                .child(
                    Button::new("settings-save")
                        .small()
                        .primary()
                        .icon(IconName::Check)
                        .label("Save Settings")
                        .on_click(move |_, window, cx| on_save(window, cx)),
                ),
        );

    modal_backdrop(card, on_backdrop_close)
}

fn section_heading(text: &'static str) -> impl IntoElement {
    div()
        .text_size(TYPE_BODY)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(silver_text())
        .child(text)
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
        .p_0p5()
        .bg(table_header_bg())
        .border_1()
        .border_color(graphite_border())
        .rounded(px(4.0))
        .flex()
        .items_center()
        .gap_1()
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
                .bg(table_header_bg())
                .border_1()
                .border_color(paper_border())
                .rounded(px(4.0))
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
                .rounded(px(3.0))
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
                    table_header_bg()
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
    let mut btn = Button::new(id).small().label(label);
    if selected {
        btn = btn.primary();
    } else {
        btn = btn.ghost();
    }
    btn.on_click(move |_, window, cx| on_action(action.clone(), window, cx))
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
    Button::new(id)
        .small()
        .ghost()
        .label(label)
        .on_click(move |_, window, cx| on_action(action.clone(), window, cx))
}
