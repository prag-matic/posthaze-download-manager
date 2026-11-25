//! Independent, auto-activating GPUI window for pre-flight download probing.

use std::path::PathBuf;

use gpui_kit::component::TitleBar;
use gpui_kit::component::input::InputState;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{
    CollisionPolicy, DownloadCategory, DownloadId, DownloadRequest, DownloadState,
    GENERAL_CATEGORY_ID, PreflightInfo, Settings, disambiguate_output_path_with_reserved,
    effective_category_dir, resolve_category, resolve_target_path,
    resolve_target_path_with_reserved,
};

use crate::PosthazeApp;
use crate::bridge::{EngineBridge, EngineCommand};
use crate::curl_parser::{find_header_value, set_or_remove_header};
use crate::grid_cells::grid_brand_plate;
use crate::modals::{
    ConfirmAction, DuplicateBannerAction, DuplicateMatch, PreflightModalState, PreflightTab,
    category_dropdown_extra_height, folder_dropdown_extra_height, render_confirm_modal,
    render_preflight_card, split_filename_extension,
};
use crate::notifications::ToastRequest;
use crate::platform_hooks::open_file_in_shell;
use crate::theme::*;

/// Optional browser-capture metadata and transfer hints for [`PreflightWindow`].
#[derive(Default, Clone)]
pub(crate) struct PreflightHints {
    pub(crate) filename: Option<String>,
    pub(crate) total_bytes: Option<u64>,
    pub(crate) single_stream: bool,
    pub(crate) skip_probe: bool,
    #[allow(dead_code)]
    pub(crate) original_url: Option<String>,
    #[allow(dead_code)]
    pub(crate) source_page_url: Option<String>,
}

/// Standalone pre-flight probe & confirmation window for a single `Download` URL.
pub(crate) struct PreflightWindow {
    bridge: EngineBridge,
    settings: Settings,
    categories: Vec<DownloadCategory>,
    hints: PreflightHints,
    state: PreflightModalState,
    overwrite_confirm: Option<ConfirmAction>,
    focus_handle: FocusHandle,
    main_window: AnyWindowHandle,
    main_app: WeakEntity<PosthazeApp>,
    _appearance_subscription: Subscription,
    probe_generation: u64,
    confirm_in_flight: bool,
}

impl PreflightWindow {
    /// Opens a new foreground `PreflightWindow` for `url` and starts probing immediately.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn open(
        bridge: EngineBridge,
        settings: Settings,
        categories: Vec<DownloadCategory>,
        url: String,
        headers: Vec<(String, String)>,
        hints: PreflightHints,
        main_window: AnyWindowHandle,
        main_app: WeakEntity<PosthazeApp>,
        cascade_idx: usize,
        cx: &mut App,
    ) {
        let mut bounds = Bounds::centered(None, size(px(460.0), px(252.0)), cx);
        let cascade = ((cascade_idx % 6) as f32) * 28.0;
        bounds.origin.x += px(cascade);
        bounds.origin.y += px(cascade);

        let active_palette = active_theme();
        let main_app_for_fallback = main_app.clone();
        let fallback_theme = settings.ui_theme.clone();
        let fallback_accent = settings.ui_accent_override.clone();

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background: active_palette.backdrop.to_window_background(),
            focus: true,
            show: true,
            kind: WindowKind::Normal,
            window_min_size: Some(size(px(420.0), px(190.0))),
            ..TitleBar::window_options()
        };

        if let Ok((handle, _)) = gpui_kit::open_window(options, cx, move |window, cx| {
            let entity = cx.new(|cx| {
                Self::new(
                    bridge,
                    settings,
                    categories,
                    url,
                    headers,
                    hints,
                    main_window,
                    main_app,
                    window,
                    cx,
                )
            });
            window.activate_window();
            entity
        }) {
            let backdrop_failed = handle
                .update(cx, |_, window, _| {
                    let res = crate::platform_hooks::apply_window_backdrop(
                        window,
                        active_palette.backdrop,
                        active_palette.tone,
                    );
                    window.activate_window();
                    res.is_err()
                })
                .unwrap_or(false);
            if backdrop_failed && active_palette.backdrop != ThemeBackdrop::Opaque {
                let (active_theme_id, active_accent) = main_app_for_fallback
                    .update(cx, |app, _| app.active_theme_selection())
                    .unwrap_or((fallback_theme, fallback_accent));
                crate::sync_theme_and_backdrops(
                    &active_theme_id,
                    active_accent.as_deref(),
                    cx,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        bridge: EngineBridge,
        settings: Settings,
        categories: Vec<DownloadCategory>,
        url: String,
        headers: Vec<(String, String)>,
        hints: PreflightHints,
        main_window: AnyWindowHandle,
        main_app: WeakEntity<PosthazeApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let main_app_for_appearance = main_app.clone();
        let _appearance_subscription = window.observe_window_appearance(move |window, cx| {
            let Ok((theme_id, _)) =
                main_app_for_appearance.update(cx, |app, _| app.active_theme_selection())
            else {
                return;
            };
            if crate::is_forced_mica_polarity(&theme_id) {
                let is_dark = active_theme().tone == PaletteTone::Dark;
                window.defer(cx, move |window, _| {
                    crate::platform_hooks::assert_window_dark_mode(window, is_dark);
                });
            }
        });
        let mut this = Self {
            bridge,
            settings,
            categories,
            hints,
            state: PreflightModalState::Probing {
                url: url.clone(),
                headers: headers.clone(),
            },
            overwrite_confirm: None,
            focus_handle,
            main_window,
            main_app,
            _appearance_subscription,
            probe_generation: 0,
            confirm_in_flight: false,
        };
        if this.hints.skip_probe {
            let suggested_filename = this
                .hints
                .filename
                .as_deref()
                .and_then(|s| std::path::Path::new(s).file_name())
                .and_then(|n| n.to_str())
                .filter(|s| !s.trim().is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| fallback_filename_from_url_str(&url));
            let disambiguated_output_path = resolve_target_path(
                &this.settings.default_output_dir,
                &suggested_filename,
                this.settings.collision_policy,
            )
            .path;
            let info = PreflightInfo {
                original_url: url.clone(),
                final_url: url,
                suggested_filename,
                disambiguated_output_path,
                content_length: this.hints.total_bytes,
                supports_ranges: false,
                etag: None,
                last_modified: None,
                server_digest: None,
            };
            this.apply_probe_info(info, headers, window, cx);
        } else {
            this.begin_probe(url, headers, window, cx);
        }
        this
    }

    fn apply_probe_info(
        &mut self,
        info: PreflightInfo,
        headers: Vec<(String, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let default_workers = self.settings.max_workers.clamp(1, 32);
        let matched_category = resolve_category(
            &self.categories,
            &info.suggested_filename,
            Some(&info.final_url),
            self.settings.category_url_rule_precedence,
        )
        .cloned();
        let matched_cat_id = matched_category
            .as_ref()
            .map(|c| c.id.clone())
            .unwrap_or_else(|| GENERAL_CATEGORY_ID.to_string());
        let base_dir = if self.settings.auto_categorize_folders
            && let Some(cat) = matched_category.as_ref()
        {
            effective_category_dir(cat, &self.settings.default_output_dir)
        } else {
            info.disambiguated_output_path
                .parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| self.settings.default_output_dir.clone())
        };
        let initial_name = if self.settings.collision_policy == CollisionPolicy::Rename {
            resolve_target_path(&base_dir, &info.suggested_filename, CollisionPolicy::Rename)
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&info.suggested_filename)
                .to_string()
        } else {
            info.suggested_filename.clone()
        };
        let (stem, probed_ext) = split_filename_extension(&initial_name);
        let extension_locked = self.settings.ui_lock_file_extension && probed_ext.is_some();
        let display_name = if extension_locked { stem } else { initial_name };
        let filename_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("Filename");
            st.set_value(display_name, window, cx);
            st
        });
        let dir_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("Output directory");
            st.set_value(base_dir.to_string_lossy().to_string(), window, cx);
            st
        });
        let init_cookie = find_header_value(&headers, "Cookie").to_string();
        let init_referer = find_header_value(&headers, "Referer").to_string();
        let init_ua = find_header_value(&headers, "User-Agent").to_string();
        let cookie_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("session=...; token=...");
            st.set_value(init_cookie, window, cx);
            st
        });
        let referer_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("https://...");
            st.set_value(init_referer, window, cx);
            st
        });
        let user_agent_input = cx.new(|cx| {
            let mut st = InputState::new(window, cx).placeholder("Mozilla/5.0 ...");
            st.set_value(init_ua, window, cx);
            st
        });
        let workers = if info.supports_ranges && !self.hints.single_stream && !self.hints.skip_probe {
            default_workers
        } else {
            1
        };
        let original_url = info.original_url.clone();
        let final_url = info.final_url.clone();
        let suggested_name = info.suggested_filename.clone();
        let probed_length = info.content_length;
        self.state = PreflightModalState::Resolved {
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
            default_dir: Box::new(self.settings.default_output_dir.clone()),
            dir_dropdown_open: false,
            categories: self.categories.clone(),
            selected_category_id: matched_cat_id.clone(),
            auto_matched_category_id: matched_cat_id,
            category_dropdown_open: false,
            duplicate_match: None,
            recent_folders: Vec::new(),
        };
        self.sync_height(window);

        let generation = self.probe_generation;
        let window_handle = window.window_handle();
        let ctx_rx = self.bridge.preflight_context(
            vec![final_url, original_url],
            Some(suggested_name),
            probed_length,
            Some(base_dir),
            6,
        );

        cx.spawn(async move |this, cx| {
            let ctx_res = ctx_rx.await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |win, cx| {
                    if win.probe_generation != generation {
                        return;
                    }
                    let Ok(Ok(ctx_data)) = ctx_res else {
                        return;
                    };
                    if let PreflightModalState::Resolved {
                        duplicate_match,
                        recent_folders,
                        categories,
                        ..
                    } = &mut win.state
                    {
                        *duplicate_match = ctx_data.duplicate.map(|rec| {
                            Box::new(DuplicateMatch {
                                filename: crate::view_model::derive_display_filename(&rec),
                                id: rec.id,
                                state: rec.state,
                                output_path: rec.output_path,
                                file_exists: ctx_data.duplicate_file_exists,
                                bytes_downloaded: rec.bytes_downloaded,
                                content_length: rec.content_length,
                            })
                        });
                        *recent_folders = ctx_data.recent_folders;
                        if !ctx_data.categories.is_empty() {
                            *categories = ctx_data.categories.clone();
                            win.categories = ctx_data.categories;
                        }
                        win.sync_height(window);
                        cx.notify();
                    }
                });
            });
        })
        .detach();
    }

    fn sync_height(&self, window: &mut Window) {
        let target_h = match &self.state {
            PreflightModalState::Probing { .. } => 216.0,
            PreflightModalState::Error { .. } => 244.0,
            PreflightModalState::Resolved {
                active_tab,
                default_dir,
                dir_dropdown_open,
                categories,
                category_dropdown_open,
                duplicate_match,
                recent_folders,
                ..
            } => match active_tab {
                PreflightTab::General => {
                    let mut h = 252.0;
                    if *category_dropdown_open {
                        h += category_dropdown_extra_height(categories);
                    }
                    if *dir_dropdown_open {
                        h += folder_dropdown_extra_height(default_dir.as_path(), recent_folders);
                    }
                    if duplicate_match.is_some() {
                        h += 46.0;
                    }
                    h
                }
                PreflightTab::Options => 272.0,
            },
        };
        window.resize(size(px(460.0), px(target_h)));
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
        self.sync_height(window);
        cx.notify();

        let out_dir = self.settings.default_output_dir.clone();
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
                            win.apply_probe_info(info, headers, window, cx);
                        }
                        Ok(Err(err)) => {
                            win.state = PreflightModalState::Error {
                                url,
                                headers,
                                message: err.to_string(),
                            };
                            win.sync_height(window);
                        }
                        Err(_) => {
                            win.state = PreflightModalState::Error {
                                url,
                                headers,
                                message: "Probe worker channel closed unexpectedly.".to_string(),
                            };
                            win.sync_height(window);
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
        if self.confirm_in_flight {
            return;
        }
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
            selected_category_id,
            ..
        } = &self.state
        else {
            return;
        };

        let selected_category_id = selected_category_id.clone();

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
        req.max_workers = if info.supports_ranges && !self.hints.single_stream && !self.hints.skip_probe {
            *workers
        } else {
            1
        };
        req.sync_on_complete = self.settings.sync_on_complete;
        req.headers = final_headers;
        req.known_content_length = info.content_length;
        req.skip_probe = self.hints.skip_probe;
        req.category_id = Some(selected_category_id);
        if !self.hints.skip_probe {
            req.preflight_probe = Some(info.as_ref().clone());
        }

        let target_name = if filename.is_empty() {
            info.suggested_filename.clone()
        } else {
            filename.clone()
        };
        let urls = vec![info.final_url.clone(), info.original_url.clone()];
        let generation = self.probe_generation;
        let window_handle = window.window_handle();
        self.confirm_in_flight = true;
        let conflict_rx = self.bridge.check_confirm_conflicts(
            urls,
            Some(target_name.clone()),
            info.content_length,
            Some(out_dir.clone()),
        );

        cx.spawn(async move |this, cx| {
            let conflict = conflict_rx.await.ok().and_then(Result::ok);
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let _ = this.update(cx, |win, cx| {
                    win.confirm_in_flight = false;
                    if win.probe_generation != generation {
                        return;
                    }
                    let reserved = conflict
                        .as_ref()
                        .map(|c| c.reserved_paths.as_slice())
                        .unwrap_or(&[]);
                    let dup_ongoing = conflict
                        .as_ref()
                        .and_then(|c| c.duplicate.as_ref())
                        .filter(|rec| {
                            matches!(
                                rec.state,
                                DownloadState::Active
                                    | DownloadState::Probing
                                    | DownloadState::Queued
                                    | DownloadState::Paused
                            )
                        });

                    let resolution = resolve_target_path_with_reserved(
                        &out_dir,
                        &target_name,
                        win.settings.collision_policy,
                        reserved,
                    );
                    if !filename.is_empty() {
                        req.output_path = Some(resolution.path.clone());
                    }

                    if let Some(dup) = dup_ongoing {
                        let disambiguated_path = disambiguate_output_path_with_reserved(
                            &out_dir,
                            &target_name,
                            reserved,
                        );
                        win.overwrite_confirm = Some(ConfirmAction::DuplicateOngoing {
                            existing_id: dup.id.clone(),
                            existing_state: dup.state,
                            existing_filename: crate::view_model::derive_display_filename(dup),
                            pending_request: Box::new(req),
                            start_immediately,
                            disambiguated_path,
                        });
                        cx.notify();
                        return;
                    }

                    if resolution.needs_overwrite_prompt && !filename.is_empty() {
                        win.overwrite_confirm = Some(ConfirmAction::OverwriteCollision {
                            request: Box::new(req),
                            start_immediately,
                            filename,
                        });
                        cx.notify();
                        return;
                    }

                    win.dispatch_and_close(
                        req,
                        start_immediately,
                        Some(filename),
                        window,
                        cx,
                    );
                });
            });
        })
        .detach();
    }

    fn handle_duplicate_action(
        &self,
        action: DuplicateBannerAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            DuplicateBannerAction::OpenFile(path) => {
                let bridge = self.bridge.clone();
                bridge.runtime().spawn_blocking(move || {
                    let _ = open_file_in_shell(&path);
                });
                window.remove_window();
            }
            DuplicateBannerAction::ShowExisting(id) => {
                self.focus_existing_in_main(id, false, cx);
                window.remove_window();
            }
            DuplicateBannerAction::ResumeExisting(id) => {
                self.focus_existing_in_main(id, true, cx);
                window.remove_window();
            }
        }
    }

    fn focus_existing_in_main(&self, id: DownloadId, resume: bool, cx: &mut App) {
        let main_win = self.main_window;
        let main_app = self.main_app.clone();
        let _ = main_win.update(cx, |_, mw, cx| {
            let _ = main_app.update(cx, |app, cx| {
                app.select_and_reveal_download(id.clone(), cx);
                if resume {
                    app.resume_download(id.clone(), cx);
                }
                cx.notify();
            });
            mw.activate_window();
        });
    }

    fn dispatch_and_close(
        &self,
        req: DownloadRequest,
        start_immediately: bool,
        filename_hint: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let allow_added_toast =
            self.settings.notify_on_added && (!self.settings.notify_only_inactive || !window.is_window_active());
        let resolved_name = filename_hint
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                req.output_path
                    .as_deref()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "Download".to_string());
        let output_dir = req
            .output_path
            .as_deref()
            .and_then(|p| p.parent())
            .map(PathBuf::from)
            .unwrap_or_else(|| req.output_dir.clone());
        let total_bytes = req.known_content_length;

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
                    if allow_added_toast {
                        app.notify_toast(ToastRequest::Queued {
                            id: created_id.clone(),
                            filename: resolved_name,
                            output_dir: Some(output_dir),
                            total_bytes,
                        });
                    }
                    if let Some(id) = created_id
                        && (start_immediately || app.queue.selected_id.is_none())
                    {
                        app.select_and_reveal_download(id, cx);
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
                        if let PreflightModalState::Resolved {
                            dir_input,
                            dir_dropdown_open,
                            ..
                        } = &mut win.state
                        {
                            *dir_dropdown_open = false;
                            dir_input.update(cx, |st, cx| {
                                st.set_value(chosen_str, window, cx);
                            });
                            win.sync_height(window);
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
        if let PreflightModalState::Resolved {
            active_tab,
            dir_dropdown_open,
            category_dropdown_open,
            ..
        } = &mut self.state
            && *active_tab != tab
        {
            *active_tab = tab;
            *dir_dropdown_open = false;
            *category_dropdown_open = false;
            self.sync_height(window);
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
            .bg(titlebar_bg())
            .border_color(graphite_border())
            .child(
                div()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(grid_brand_plate())
                    .when_some(active_tab, |el, tab| {
                        el.child(
                            div()
                                .id("pf-tb-add")
                                .relative()
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
                                        .child(
                                            div()
                                                .absolute()
                                                .bottom_0()
                                                .left_0()
                                                .right_0()
                                                .h(px(2.0))
                                                .bg(selenium_amber()),
                                        )
                                })
                                .when(tab != PreflightTab::General, |el| {
                                    el.text_color(silver_muted())
                                        .hover(|s| s.bg(paper_hover()).text_color(silver_text()))
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
                                .relative()
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
                                        .child(
                                            div()
                                                .absolute()
                                                .bottom_0()
                                                .left_0()
                                                .right_0()
                                                .h(px(2.0))
                                                .bg(selenium_amber()),
                                        )
                                })
                                .when(tab != PreflightTab::Options, |el| {
                                    el.text_color(silver_muted())
                                        .hover(|s| s.bg(paper_hover()).text_color(silver_text()))
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
        let entity_toggle_dropdown = cx.entity().clone();
        let entity_recent_dir = cx.entity().clone();
        let entity_open_dup = cx.entity().clone();
        let entity_toggle_cat = cx.entity().clone();
        let entity_pick_cat = cx.entity().clone();

        let card = render_preflight_card(
            &self.state,
            cx,
            show_add_paused,
            self.settings.auto_categorize_folders,
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
                    if let PreflightModalState::Resolved {
                        dir_dropdown_open, ..
                    } = &mut win.state
                    {
                        *dir_dropdown_open = false;
                        win.sync_height(window);
                    }
                    win.browse_destination_dir(window, cx);
                });
            },
            move |window, cx| {
                entity_ext_lock.update(cx, |win, cx| {
                    win.toggle_extension_lock(window, cx);
                });
            },
            move |window, cx| {
                entity_toggle_dropdown.update(cx, |win, cx| {
                    if let PreflightModalState::Resolved {
                        dir_dropdown_open,
                        category_dropdown_open,
                        ..
                    } = &mut win.state
                    {
                        *dir_dropdown_open = !*dir_dropdown_open;
                        if *dir_dropdown_open {
                            *category_dropdown_open = false;
                        }
                        win.sync_height(window);
                        cx.notify();
                    }
                });
            },
            move |folder, window, cx| {
                entity_recent_dir.update(cx, |win, cx| {
                    if let PreflightModalState::Resolved {
                        dir_input,
                        dir_dropdown_open,
                        ..
                    } = &mut win.state
                    {
                        let value = folder.to_string_lossy().to_string();
                        *dir_dropdown_open = false;
                        dir_input.update(cx, |st, cx| {
                            st.set_value(value, window, cx);
                        });
                        win.sync_height(window);
                        cx.notify();
                    }
                });
            },
            move |action, window, cx| {
                entity_open_dup.update(cx, |win, cx| {
                    win.handle_duplicate_action(action, window, cx);
                });
            },
            move |window, cx| {
                entity_toggle_cat.update(cx, |win, cx| {
                    if let PreflightModalState::Resolved {
                        category_dropdown_open,
                        dir_dropdown_open,
                        ..
                    } = &mut win.state
                    {
                        *category_dropdown_open = !*category_dropdown_open;
                        if *category_dropdown_open {
                            *dir_dropdown_open = false;
                        }
                        win.sync_height(window);
                        cx.notify();
                    }
                });
            },
            move |cat_id, window, cx| {
                entity_pick_cat.update(cx, |win, cx| {
                    if let PreflightModalState::Resolved {
                        categories,
                        selected_category_id,
                        category_dropdown_open,
                        dir_input,
                        default_dir,
                        ..
                    } = &mut win.state
                    {
                        *selected_category_id = cat_id.clone();
                        *category_dropdown_open = false;
                        if let Some(cat) = categories.iter().find(|c| c.id == cat_id) {
                            let next_dir = effective_category_dir(cat, default_dir.as_path());
                            let value = next_dir.to_string_lossy().to_string();
                            dir_input.update(cx, |st, cx| {
                                st.set_value(value, window, cx);
                            });
                        }
                        win.sync_height(window);
                        cx.notify();
                    }
                });
            },
        );

        let overwrite_overlay = self.overwrite_confirm.as_ref().map(|action| {
            let e_cancel = cx.entity().clone();
            let e_confirm = cx.entity().clone();
            let e_alt = cx.entity().clone();
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
                        match win.overwrite_confirm.take() {
                            Some(ConfirmAction::OverwriteCollision {
                                request,
                                start_immediately,
                                filename,
                            }) => {
                                win.dispatch_and_close(
                                    *request,
                                    start_immediately,
                                    Some(filename),
                                    window,
                                    cx,
                                );
                            }
                            Some(ConfirmAction::DuplicateOngoing {
                                existing_id,
                                existing_state,
                                ..
                            }) => {
                                win.focus_existing_in_main(
                                    existing_id,
                                    existing_state == DownloadState::Paused,
                                    cx,
                                );
                                window.remove_window();
                            }
                            _ => {}
                        }
                    });
                },
                |_, _, _| {},
                move |window, cx| {
                    e_alt.update(cx, |win, cx| {
                        if let Some(ConfirmAction::DuplicateOngoing {
                            mut pending_request,
                            start_immediately,
                            disambiguated_path,
                            ..
                        }) = win.overwrite_confirm.take()
                        {
                            let fname = disambiguated_path
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("download")
                                .to_string();
                            pending_request.output_path = Some(disambiguated_path);
                            win.dispatch_and_close(
                                *pending_request,
                                start_immediately,
                                Some(fname),
                                window,
                                cx,
                            );
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
            .text_color(silver_text())
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|win, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                if key == "escape" {
                    if win.overwrite_confirm.take().is_some() {
                        cx.notify();
                    } else if let PreflightModalState::Resolved {
                        dir_dropdown_open, ..
                    } = &mut win.state
                        && *dir_dropdown_open
                    {
                        *dir_dropdown_open = false;
                        win.sync_height(window);
                        cx.notify();
                    } else {
                        window.remove_window();
                    }
                } else if key == "enter" {
                    match win.overwrite_confirm.take() {
                        Some(ConfirmAction::OverwriteCollision {
                            request,
                            start_immediately,
                            ..
                        }) => {
                            let filename_hint = request
                                .output_path
                                .as_ref()
                                .and_then(|p| p.file_name())
                                .map(|n| n.to_string_lossy().into_owned());
                            win.dispatch_and_close(
                                *request,
                                start_immediately,
                                filename_hint,
                                window,
                                cx,
                            );
                        }
                        Some(ConfirmAction::DuplicateOngoing {
                            existing_id,
                            existing_state,
                            ..
                        }) => {
                            win.focus_existing_in_main(
                                existing_id,
                                existing_state == DownloadState::Paused,
                                cx,
                            );
                            window.remove_window();
                        }
                        _ if matches!(win.state, PreflightModalState::Resolved { .. }) => {
                            win.confirm_preflight(true, window, cx);
                        }
                        _ => {}
                    }
                }
            }))
            .child(self.render_titlebar(cx))
            .child(card)
            .when_some(overwrite_overlay, |el, overlay| el.child(overlay))
    }
}

fn fallback_filename_from_url_str(raw_url: &str) -> String {
    raw_url
        .split('?')
        .next()
        .and_then(|path| path.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("download.bin")
        .to_string()
}
