//! Comprehensive multi-tab Settings modal (`General`, `Network`, `Automation`, `Interface`, `Reset`).

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::InputState;
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
            let mut st = InputState::new(window, cx).placeholder("Default output directory");
            st.set_value(
                settings.default_output_dir.to_string_lossy().to_string(),
                window,
                cx,
            );
            st
        });
        let workers_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("1..32");
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
            let mut st = InputState::new(window, cx).placeholder("0 = Unlimited");
            st.set_value(settings.download_speed_limit_kib.to_string(), window, cx);
            st
        });
        let auto_retry_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("0..10");
            st.set_value(settings.auto_retry_count.to_string(), window, cx);
            st
        });
        let cmd_path_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("Executable path...");
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
            return Err("Default output directory cannot be empty.".to_string());
        }
        let max_workers = self
            .workers_input
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .map_err(|_| "Max workers must be an integer between 1 and 32.".to_string())?;
        if !(1..=32).contains(&max_workers) {
            return Err("Max workers must be between 1 and 32.".to_string());
        }
        let max_concurrent_downloads = self
            .concurrent_input
            .read(cx)
            .value()
            .trim()
            .parse::<usize>()
            .map_err(|_| "Max simultaneous downloads must be at least 1.".to_string())?;
        if max_concurrent_downloads == 0 {
            return Err("Max simultaneous downloads must be at least 1.".to_string());
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
            .map_err(|_| "Download speed limit must be a non-negative integer (KiB/s).".to_string())?;
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
            return Err("Post-completion command path is required when enabled.".to_string());
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
            .child(labeled_input("Default Download Folder", &state.dir_input))
            .child(toggle_row(
                "settings-auto-categorize",
                "Auto-categorize subfolders by file type (Archives, Video, Audio, Documents, Programs)",
                state.auto_categorize_folders,
                SettingsAction::ToggleAutoCategorize,
                on_action.clone(),
            )),

        SettingsTab::Downloads => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(section_label("File Exists Reaction"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(choice_btn(
                        "coll-rename",
                        "Rename",
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
                        "Always Ask",
                        state.collision_policy == CollisionPolicy::Ask,
                        SettingsAction::SetCollisionPolicy(CollisionPolicy::Ask),
                        on_action.clone(),
                    )),
            )
            .child(section_label("Row Delete Button Default"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(choice_btn(
                        "del-ask",
                        "Always Ask",
                        state.delete_action_default == DeleteDefaultAction::Ask,
                        SettingsAction::SetDeleteDefault(DeleteDefaultAction::Ask),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "del-remove",
                        "Remove from List",
                        state.delete_action_default == DeleteDefaultAction::Remove,
                        SettingsAction::SetDeleteDefault(DeleteDefaultAction::Remove),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "del-file",
                        "Delete File & Entry",
                        state.delete_action_default == DeleteDefaultAction::DeleteFile,
                        SettingsAction::SetDeleteDefault(DeleteDefaultAction::DeleteFile),
                        on_action.clone(),
                    )),
            )
            .child(labeled_input(
                "Auto-Retry Failed Downloads (0..10)",
                &state.auto_retry_input,
            )),

        SettingsTab::Network => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(section_label("Proxy Mode"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .child(choice_btn(
                        "proxy-sys",
                        "System Proxy",
                        state.proxy_mode == ProxyMode::System,
                        SettingsAction::SetProxyMode(ProxyMode::System),
                        on_action.clone(),
                    ))
                    .child(choice_btn(
                        "proxy-none",
                        "No Proxy",
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
            )
            .when(state.proxy_mode == ProxyMode::Manual, |el| {
                el.child(labeled_input("Proxy URL", &state.proxy_url_input))
            })
            .child(section_label("Traffic Speed Profile"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap_1p5()
                    .child(preset_btn(
                        "spd-unlim",
                        "Unlimited",
                        SettingsAction::SetSpeedPreset(0),
                        on_action.clone(),
                    ))
                    .child(preset_btn(
                        "spd-low",
                        "Low (256 KiB/s)",
                        SettingsAction::SetSpeedPreset(256),
                        on_action.clone(),
                    ))
                    .child(preset_btn(
                        "spd-med",
                        "Medium (2 MiB/s)",
                        SettingsAction::SetSpeedPreset(2048),
                        on_action.clone(),
                    ))
                    .child(preset_btn(
                        "spd-high",
                        "High (8 MiB/s)",
                        SettingsAction::SetSpeedPreset(8192),
                        on_action.clone(),
                    )),
            )
            .child(labeled_input(
                "Global Download Speed Limit (KiB/s, 0 = Unlimited)",
                &state.speed_limit_input,
            ))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .child(labeled_input("Max Workers (1..32)", &state.workers_input)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(labeled_input("Max Simultaneous (1+)", &state.concurrent_input)),
                    ),
            )
            .child(toggle_row(
                "settings-sync",
                "Flush to disk on completion (sync_all)",
                state.sync_on_complete,
                SettingsAction::ToggleSyncOnComplete,
                on_action.clone(),
            )),

        SettingsTab::Automation => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(section_label("Windows Notifications"))
            .child(toggle_row(
                "notify-added",
                "Notify when a Download is added",
                state.notify_on_added,
                SettingsAction::ToggleNotifyAdded,
                on_action.clone(),
            ))
            .child(toggle_row(
                "notify-completed",
                "Notify when a Download completes",
                state.notify_on_completed,
                SettingsAction::ToggleNotifyCompleted,
                on_action.clone(),
            ))
            .child(toggle_row(
                "notify-failed",
                "Notify when a Download fails",
                state.notify_on_failed,
                SettingsAction::ToggleNotifyFailed,
                on_action.clone(),
            ))
            .child(toggle_row(
                "notify-inactive",
                "Notify only when Posthaze window is unfocused",
                state.notify_only_inactive,
                SettingsAction::ToggleNotifyOnlyInactive,
                on_action.clone(),
            ))
            .child(div().h(px(1.0)).w_full().bg(graphite_border()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(section_label("Post-Completion Command / Antivirus Scan"))
                    .child({
                        let act = on_action.clone();
                        Button::new("settings-defender-preset")
                            .xsmall()
                            .icon(IconName::ShieldCheck)
                            .label("Windows Defender Preset")
                            .on_click(move |_, window, cx| {
                                act(SettingsAction::ApplyDefenderPreset, window, cx);
                            })
                    }),
            )
            .child(toggle_row(
                "cmd-enabled",
                "Run external executable or antivirus scan when Download completes",
                state.completion_cmd_enabled,
                SettingsAction::ToggleCompletionCmd,
                on_action.clone(),
            ))
            .when(state.completion_cmd_enabled, |el| {
                el.child(labeled_input("Executable Path", &state.cmd_path_input))
                    .child(labeled_input("Arguments (%path% = file path)", &state.cmd_args_input))
            }),

        SettingsTab::Interface => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(section_label("Interaction Behavior"))
            .child(toggle_row(
                "ui-click-insp",
                "Single-click download row opens Byte-Map Inspector",
                state.ui_click_row_opens_inspector,
                SettingsAction::ToggleClickOpensInspector,
                on_action.clone(),
            ))
            .child(toggle_row(
                "ui-add-paused",
                "Show \"Add Paused\" button in Pre-Flight modal",
                state.ui_show_add_paused,
                SettingsAction::ToggleShowAddPaused,
                on_action.clone(),
            ))
            .child(section_label("UI & Font Zoom"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
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
            ),

        SettingsTab::Reset => div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .p_3()
                    .bg(graphite_elevated())
                    .border_1()
                    .border_color(graphite_border())
                    .rounded(px(4.0))
                    .text_xs()
                    .line_height(px(18.0))
                    .text_color(silver_muted())
                    .child("Restores all Posthaze settings—including default download folder, proxy, speed limits, collision/delete rules, notifications, post-completion command, and UI zoom—back to canonical defaults. Existing downloads in your Queue are untouched."),
            )
            .child(
                div().flex().child({
                    let act = on_action.clone();
                    Button::new("settings-reset-defaults")
                        .small()
                        .danger()
                        .icon(IconName::RotateCcw)
                        .label("Reset All Settings to Defaults")
                        .on_click(move |_, window, cx| {
                            act(SettingsAction::ResetToDefaults, window, cx);
                        })
                }),
            ),
    };

    let card = div()
        .w(px(760.0))
        .h(px(520.0))
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
                        .xsmall()
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
                        .w(px(172.0))
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
                                .px_2p5()
                                .py_1p5()
                                .rounded(px(4.0))
                                .flex()
                                .items_center()
                                .gap_2()
                                .cursor_pointer()
                                .bg(if active {
                                    paper_hover()
                                } else {
                                    graphite_elevated()
                                })
                                .text_size(TYPE_BODY)
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
                        .p_4()
                        .overflow_y_scroll()
                        .child(body),
                ),
        )
        .when_some(state.error.clone(), |el, err| {
            el.child(
                div()
                    .mx_4()
                    .mb_2()
                    .px_3()
                    .py_1p5()
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

fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .text_size(TYPE_MICRO)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(silver_muted())
        .child(text)
}

fn toggle_row<F>(
    id: &'static str,
    label: &'static str,
    checked: bool,
    action: SettingsAction,
    on_action: F,
) -> impl IntoElement
where
    F: Fn(SettingsAction, &mut Window, &mut App) + 'static,
{
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .on_click(move |_, window, cx| on_action(action.clone(), window, cx))
        .child(
            div()
                .w(px(15.0))
                .h(px(15.0))
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
        .child(div().text_xs().text_color(silver_text()).child(label))
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
    let mut btn = Button::new(id).xsmall().label(label);
    if selected {
        btn = btn.primary();
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
        .xsmall()
        .label(label)
        .on_click(move |_, window, cx| on_action(action.clone(), window, cx))
}
