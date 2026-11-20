//! Independent, auto-activating GPUI window for pre-flight download probing.

use std::path::PathBuf;

use gpui_kit::component::TitleBar;
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{
    CollisionPolicy, DownloadRequest, Settings, resolve_target_path,
};

use crate::PosthazeApp;
use crate::assets::BRAND_LOGO_SVG;
use crate::bridge::{EngineBridge, EngineCommand};
use crate::curl_parser::{find_header_value, set_or_remove_header};
use crate::modals::{
    ConfirmAction, PreflightModalState, PreflightTab, render_confirm_modal, render_preflight_card,
    split_filename_extension,
};
use crate::platform_hooks::spawn_windows_toast;
use crate::theme::*;

/// Standalone pre-flight probe & confirmation window for a single `Download` URL.
pub(crate) struct PreflightWindow {
    bridge: EngineBridge,
    settings: Settings,
    state: PreflightModalState,
    overwrite_confirm: Option<ConfirmAction>,
    focus_handle: FocusHandle,
    main_window: AnyWindowHandle,
    main_app: WeakEntity<PosthazeApp>,
    probe_generation: u64,
}

impl PreflightWindow {
    /// Opens a new foreground `PreflightWindow` for `url` and starts probing immediately.
    pub(crate) fn open(
        bridge: EngineBridge,
        settings: Settings,
        url: String,
        headers: Vec<(String, String)>,
        main_window: AnyWindowHandle,
        main_app: WeakEntity<PosthazeApp>,
        cascade_idx: usize,
        cx: &mut App,
    ) {
        let mut bounds = Bounds::centered(None, size(px(460.0), px(226.0)), cx);
        let cascade = ((cascade_idx % 6) as f32) * 28.0;
        bounds.origin.x += px(cascade);
        bounds.origin.y += px(cascade);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            window_min_size: Some(size(px(420.0), px(210.0))),
            ..TitleBar::window_options()
        };

        if let Ok((handle, _)) = gpui_kit::open_window(options, cx, move |window, cx| {
            let entity = cx.new(|cx| {
                Self::new(
                    bridge,
                    settings,
                    url,
                    headers,
                    main_window,
                    main_app,
                    window,
                    cx,
                )
            });
            window.activate_window();
            entity
        }) {
            let _ = handle.update(cx, |_, window, _| {
                window.activate_window();
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        bridge: EngineBridge,
        settings: Settings,
        url: String,
        headers: Vec<(String, String)>,
        main_window: AnyWindowHandle,
        main_app: WeakEntity<PosthazeApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let mut this = Self {
            bridge,
            settings,
            state: PreflightModalState::Probing {
                url: url.clone(),
                headers: headers.clone(),
            },
            overwrite_confirm: None,
            focus_handle,
            main_window,
            main_app,
            probe_generation: 0,
        };
        this.begin_probe(url, headers, window, cx);
        this
    }

    fn begin_probe(
        &mut self,
        url: String,
        headers: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.probe_generation = self.probe_generation.wrapping_add(1);
        let generation = self.probe_generation;
        self.state = PreflightModalState::Probing {
            url: url.clone(),
            headers: headers.clone(),
        };
        cx.notify();

        let out_dir = self.settings.default_output_dir.clone();
        let default_workers = self.settings.max_workers.clamp(1, 32);
        let window_handle = window.window_handle();
        let probe_headers = headers.clone();
        let rx = self.bridge.probe(url.clone(), out_dir, probe_headers);

        cx.spawn(async move |this, cx| {
            let res = rx.await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |win, cx| {
                    if win.probe_generation != generation {
                        return;
                    }
                    match res {
                        Ok(Ok(info)) => {
                            let initial_name =
                                if win.settings.collision_policy == CollisionPolicy::Rename {
                                    info.disambiguated_output_path
                                        .file_name()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or(&info.suggested_filename)
                                        .to_string()
                                } else {
                                    info.suggested_filename.clone()
                                };
                            let base_dir = info
                                .disambiguated_output_path
                                .parent()
                                .map(PathBuf::from)
                                .unwrap_or_else(|| win.settings.default_output_dir.clone());
                            let (stem, probed_ext) = split_filename_extension(&initial_name);
                            let extension_locked =
                                win.settings.ui_lock_file_extension && probed_ext.is_some();
                            let display_name = if extension_locked {
                                stem
                            } else {
                                initial_name
                            };
                            let filename_input = cx.new(|cx| {
                                let mut st = InputState::new(window, cx).placeholder("Filename");
                                st.set_value(display_name, window, cx);
                                st
                            });
                            let dir_input = cx.new(|cx| {
                                let mut st =
                                    InputState::new(window, cx).placeholder("Output directory");
                                st.set_value(base_dir.to_string_lossy().to_string(), window, cx);
                                st
                            });
                            let init_cookie =
                                find_header_value(&headers, "Cookie").to_string();
                            let init_referer =
                                find_header_value(&headers, "Referer").to_string();
                            let init_ua =
                                find_header_value(&headers, "User-Agent").to_string();
                            let cookie_input = cx.new(|cx| {
                                let mut st =
                                    InputState::new(window, cx).placeholder("session=...; token=...");
                                st.set_value(init_cookie, window, cx);
                                st
                            });
                            let referer_input = cx.new(|cx| {
                                let mut st =
                                    InputState::new(window, cx).placeholder("https://...");
                                st.set_value(init_referer, window, cx);
                                st
                            });
                            let user_agent_input = cx.new(|cx| {
                                let mut st =
                                    InputState::new(window, cx).placeholder("Mozilla/5.0 ...");
                                st.set_value(init_ua, window, cx);
                                st
                            });
                            let workers = if info.supports_ranges {
                                default_workers
                            } else {
                                1
                            };
                            win.state = PreflightModalState::Resolved {
                                info: Box::new(info),
                                filename_input,
                                dir_input,
                                cookie_input,
                                referer_input,
                                user_agent_input,
                                headers,
                                active_tab: PreflightTab::General,
                                extension_locked,
                                locked_extension: probed_ext,
                                workers,
                            };
                        }
                        Ok(Err(err)) => {
                            win.state = PreflightModalState::Error {
                                url,
                                headers,
                                message: err.to_string(),
                            };
                        }
                        Err(_) => {
                            win.state = PreflightModalState::Error {
                                url,
                                headers,
                                message: "Probe worker channel closed unexpectedly.".to_string(),
                            };
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn confirm_preflight(
        &mut self,
        start_immediately: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let PreflightModalState::Resolved {
            info,
            filename_input,
            dir_input,
            cookie_input,
            referer_input,
            user_agent_input,
            headers,
            extension_locked,
            locked_extension,
            workers,
            ..
        } = &self.state
        else {
            return;
        };

        let raw_name = filename_input.read(cx).value().trim().to_string();
        let filename = match (*extension_locked, locked_extension.as_deref()) {
            (true, Some(ext)) if !raw_name.is_empty() && !raw_name.ends_with(ext) => {
                format!("{raw_name}{ext}")
            }
            _ => raw_name,
        };
        let dir_str = dir_input.read(cx).value().trim().to_string();
        let cookie_val = cookie_input.read(cx).value().to_string();
        let referer_val = referer_input.read(cx).value().to_string();
        let ua_val = user_agent_input.read(cx).value().to_string();

        let mut final_headers = headers.clone();
        set_or_remove_header(&mut final_headers, "Cookie", &cookie_val);
        set_or_remove_header(&mut final_headers, "Referer", &referer_val);
        set_or_remove_header(&mut final_headers, "User-Agent", &ua_val);

        let out_dir = if dir_str.is_empty() {
            self.settings.default_output_dir.clone()
        } else {
            PathBuf::from(dir_str)
        };

        let mut req = DownloadRequest::new(info.original_url.clone(), &out_dir);
        let mut ask_overwrite_name: Option<String> = None;
        if !filename.is_empty() {
            let resolution =
                resolve_target_path(&out_dir, &filename, self.settings.collision_policy);
            if resolution.needs_overwrite_prompt {
                ask_overwrite_name = Some(filename.clone());
            }
            req.output_path = Some(resolution.path);
        }
        req.max_workers = if info.supports_ranges { *workers } else { 1 };
        req.sync_on_complete = self.settings.sync_on_complete;
        req.headers = final_headers;

        if let Some(collision_name) = ask_overwrite_name {
            self.overwrite_confirm = Some(ConfirmAction::OverwriteCollision {
                request: Box::new(req),
                start_immediately,
                filename: collision_name,
            });
            cx.notify();
            return;
        }

        if self.settings.notify_on_added {
            let allow_notify = !self.settings.notify_only_inactive || !window.is_window_active();
            if allow_notify {
                spawn_windows_toast(
                    "Posthaze — Download Added",
                    &format!(
                        "Queued {}",
                        if filename.is_empty() {
                            "download"
                        } else {
                            &filename
                        }
                    ),
                );
            }
        }

        self.dispatch_and_close(req, start_immediately, window, cx);
    }

    fn dispatch_and_close(
        &self,
        req: DownloadRequest,
        start_immediately: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cmd = if start_immediately {
            EngineCommand::Start { request: req }
        } else {
            EngineCommand::EnqueuePaused { request: req }
        };
        let rx = self.bridge.execute(cmd);
        let main_app = self.main_app.clone();
        cx.spawn(async move |_, cx| {
            if let Ok(Ok(res)) = rx.await {
                let created_id = res.created_id;
                let _ = main_app.update(cx, |app, cx| {
                    if let Some(id) = created_id
                        && (start_immediately || app.queue.selected_id.is_none())
                    {
                        app.queue.select_single(id);
                    }
                    app.status_banner = None;
                    app.refresh_from_engine(cx);
                });
            }
        })
        .detach();
        window.remove_window();
    }

    fn browse_destination_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select Download Folder".into()),
        });
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let res = rx.await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                if let Ok(Ok(Some(paths))) = res
                    && let Some(chosen) = paths.into_iter().next()
                {
                    let chosen_str = chosen.to_string_lossy().to_string();
                    let _ = this.update(cx, |win, cx| {
                        if let PreflightModalState::Resolved { dir_input, .. } = &win.state {
                            dir_input.update(cx, |st, cx| {
                                st.set_value(chosen_str, window, cx);
                            });
                            cx.notify();
                        }
                    });
                }
                window.activate_window();
            });
        })
        .detach();
    }

    fn toggle_extension_lock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let PreflightModalState::Resolved {
            filename_input,
            extension_locked,
            locked_extension,
            ..
        } = &mut self.state
        {
            let current = filename_input.read(cx).value().trim().to_string();
            if *extension_locked {
                if let Some(ext) = locked_extension.as_deref()
                    && !current.ends_with(ext)
                {
                    let combined = format!("{current}{ext}");
                    filename_input.update(cx, |st, cx| {
                        st.set_value(combined, window, cx);
                    });
                }
                *extension_locked = false;
            } else {
                let (stem, found_ext) = split_filename_extension(&current);
                if let Some(ext) = found_ext {
                    *locked_extension = Some(ext);
                    filename_input.update(cx, |st, cx| {
                        st.set_value(stem, window, cx);
                    });
                    *extension_locked = true;
                } else if let Some(ext) = locked_extension.as_deref() {
                    if let Some(stripped) = current.strip_suffix(ext) {
                        filename_input.update(cx, |st, cx| {
                            st.set_value(stripped.to_string(), window, cx);
                        });
                    }
                    *extension_locked = true;
                }
            }
            cx.notify();
        }
    }

    fn select_tab(&mut self, tab: PreflightTab, window: &mut Window, cx: &mut Context<Self>) {
        if let PreflightModalState::Resolved { active_tab, .. } = &mut self.state
            && *active_tab != tab
        {
            *active_tab = tab;
            let target_h = match tab {
                PreflightTab::General => 226.0,
                PreflightTab::Options => 372.0,
            };
            window.resize(size(px(460.0), px(target_h)));
            cx.notify();
        }
    }

    fn render_titlebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let active_tab = match &self.state {
            PreflightModalState::Resolved { active_tab, .. } => Some(*active_tab),
            _ => None,
        };
        let e_add = cx.entity().clone();
        let e_opts = cx.entity().clone();

        TitleBar::new()
            .bg(graphite_bg())
            .border_color(graphite_border())
            .child(
                div()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .h_full()
                            .pr_3()
                            .border_r_1()
                            .border_color(graphite_border())
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                svg()
                                    .path(BRAND_LOGO_SVG)
                                    .w(px(15.0))
                                    .h(px(13.0))
                                    .text_color(selenium_amber()),
                            )
                            .child(
                                div()
                                    .font_family(MONO_FONT)
                                    .text_size(TYPE_MICRO)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(silver_text())
                                    .child("POSTHAZE"),
                            ),
                    )
                    .when_some(active_tab, |el, tab| {
                        el.child(
                            div()
                                .id("pf-tb-add")
                                .h_full()
                                .px_2p5()
                                .border_r_1()
                                .border_color(graphite_border())
                                .flex()
                                .items_center()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_HEADER)
                                .font_weight(FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .when(tab == PreflightTab::General, |el| {
                                    el.bg(graphite_elevated())
                                        .text_color(silver_text())
                                        .border_b_2()
                                        .border_color(selenium_amber())
                                })
                                .when(tab != PreflightTab::General, |el| {
                                    el.text_color(silver_muted())
                                        .hover(|s| s.bg(graphite_elevated()).text_color(silver_text()))
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(move |_, window, cx| {
                                    e_add.update(cx, |win, cx| {
                                        win.select_tab(PreflightTab::General, window, cx);
                                    });
                                })
                                .child("ADD DOWNLOAD"),
                        )
                        .child(
                            div()
                                .id("pf-tb-options")
                                .h_full()
                                .px_2p5()
                                .border_r_1()
                                .border_color(graphite_border())
                                .flex()
                                .items_center()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_HEADER)
                                .font_weight(FontWeight::SEMIBOLD)
                                .cursor_pointer()
                                .when(tab == PreflightTab::Options, |el| {
                                    el.bg(graphite_elevated())
                                        .text_color(silver_text())
                                        .border_b_2()
                                        .border_color(selenium_amber())
                                })
                                .when(tab != PreflightTab::Options, |el| {
                                    el.text_color(silver_muted())
                                        .hover(|s| s.bg(graphite_elevated()).text_color(silver_text()))
                                })
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(move |_, window, cx| {
                                    e_opts.update(cx, |win, cx| {
                                        win.select_tab(PreflightTab::Options, window, cx);
                                    });
                                })
                                .child("OPTIONS"),
                        )
                    })
                    .when(active_tab.is_none(), |el| {
                        el.child(
                            div()
                                .h_full()
                                .px_2p5()
                                .border_r_1()
                                .border_color(graphite_border())
                                .flex()
                                .items_center()
                                .font_family(MONO_FONT)
                                .text_size(TYPE_HEADER)
                                .text_color(silver_muted())
                                .child("ADD DOWNLOAD"),
                        )
                    }),
            )
    }
}

impl Render for PreflightWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let show_add_paused = self.settings.ui_show_add_paused;
        let entity_retry = cx.entity().clone();
        let entity_delta = cx.entity().clone();
        let entity_confirm = cx.entity().clone();
        let entity_edit = cx.entity().clone();
        let entity_browse_dir = cx.entity().clone();
        let entity_ext_lock = cx.entity().clone();

        let card = render_preflight_card(
            &self.state,
            show_add_paused,
            |window, _| {
                window.remove_window();
            },
            move |url, headers, window, cx| {
                entity_retry.update(cx, |win, cx| {
                    win.begin_probe(url, headers, window, cx);
                });
            },
            move |delta, _, cx| {
                entity_delta.update(cx, |win, cx| {
                    if let PreflightModalState::Resolved { workers, .. } = &mut win.state {
                        let next = (*workers as isize + delta).clamp(1, 32) as usize;
                        *workers = next;
                        cx.notify();
                    }
                });
            },
            move |start_now, window, cx| {
                entity_confirm.update(cx, |win, cx| {
                    win.confirm_preflight(start_now, window, cx);
                });
            },
            move |edit_url, window, cx| {
                entity_edit.update(cx, |win, cx| {
                    let main_win = win.main_window;
                    let main_app = win.main_app.clone();
                    let _ = main_win.update(cx, |_, mw, cx| {
                        let _ = main_app.update(cx, |app, cx| {
                            app.url_input.update(cx, |st, cx| {
                                st.set_value(edit_url, mw, cx);
                            });
                            cx.notify();
                        });
                        mw.activate_window();
                    });
                });
                window.remove_window();
            },
            move |window, cx| {
                entity_browse_dir.update(cx, |win, cx| {
                    win.browse_destination_dir(window, cx);
                });
            },
            move |window, cx| {
                entity_ext_lock.update(cx, |win, cx| {
                    win.toggle_extension_lock(window, cx);
                });
            },
        );

        let overwrite_overlay = self.overwrite_confirm.as_ref().map(|action| {
            let e_cancel = cx.entity().clone();
            let e_confirm = cx.entity().clone();
            render_confirm_modal(
                action,
                move |_, cx| {
                    e_cancel.update(cx, |win, cx| {
                        win.overwrite_confirm = None;
                        cx.notify();
                    });
                },
                move |window, cx| {
                    e_confirm.update(cx, |win, cx| {
                        if let Some(ConfirmAction::OverwriteCollision {
                            request,
                            start_immediately,
                            ..
                        }) = win.overwrite_confirm.take()
                        {
                            win.dispatch_and_close(*request, start_immediately, window, cx);
                        }
                    });
                },
            )
        });

        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(graphite_bg())
            .text_color(silver_text())
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|win, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if key == "escape" {
                    if win.overwrite_confirm.take().is_some() {
                        cx.notify();
                    } else {
                        window.remove_window();
                    }
                } else if key == "enter" {
                    if let Some(ConfirmAction::OverwriteCollision {
                        request,
                        start_immediately,
                        ..
                    }) = win.overwrite_confirm.take()
                    {
                        win.dispatch_and_close(*request, start_immediately, window, cx);
                    } else if matches!(win.state, PreflightModalState::Resolved { .. }) {
                        win.confirm_preflight(true, window, cx);
                    }
                }
            }))
            .child(self.render_titlebar(cx))
            .child(card)
            .when_some(overwrite_overlay, |el, overlay| el.child(overlay))
    }
}
