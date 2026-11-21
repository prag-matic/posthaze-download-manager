//! Right-click context menu for download rows.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use posthaze_engine::{DownloadId, DownloadState};

use crate::theme::*;
use crate::view_model::DownloadRowView;

const MENU_WIDTH: f32 = 224.0;
const MENU_ITEM_HEIGHT: f32 = 28.0;
const MENU_EDGE_MARGIN: f32 = 8.0;

/// Active right-click context menu anchored to a download row.
#[derive(Debug, Clone)]
pub(crate) struct ContextMenuState {
    pub download_id: DownloadId,
    pub anchor: Point<Pixels>,
}

/// Actions exposed in the download row context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextMenuAction {
    Pause,
    Resume,
    RestartFresh,
    Cancel,
    OpenFile,
    ShowInFolder,
    CopyUrl,
    InspectByteMap,
    FileIntegrity,
    RenameFile,
    MoveTo,
    RemoveFromList,
    DeleteFileAndEntry,
}

/// A single item or group divider in the context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContextMenuEntry {
    Item {
        action: ContextMenuAction,
        label: &'static str,
        icon: IconName,
        enabled: bool,
        danger: bool,
    },
    Separator,
}

/// Builds the state-aware context menu entries for a download row.
pub(crate) fn build_row_menu_items(row: &DownloadRowView) -> Vec<ContextMenuEntry> {
    let mut items = Vec::new();

    match row.state {
        DownloadState::Active | DownloadState::Queued | DownloadState::Probing => {
            items.push(ContextMenuEntry::Item {
                action: ContextMenuAction::Pause,
                label: "Pause",
                icon: IconName::Pause,
                enabled: true,
                danger: false,
            });
            items.push(ContextMenuEntry::Item {
                action: ContextMenuAction::Cancel,
                label: "Cancel Transfer",
                icon: IconName::X,
                enabled: true,
                danger: false,
            });
            items.push(ContextMenuEntry::Separator);
        }
        DownloadState::Paused | DownloadState::Failed => {
            if !row.is_resource_changed {
                items.push(ContextMenuEntry::Item {
                    action: ContextMenuAction::Resume,
                    label: "Resume",
                    icon: IconName::Play,
                    enabled: true,
                    danger: false,
                });
            }
            items.push(ContextMenuEntry::Item {
                action: ContextMenuAction::RestartFresh,
                label: "Restart Fresh",
                icon: IconName::RotateCcw,
                enabled: true,
                danger: false,
            });
            items.push(ContextMenuEntry::Separator);
        }
        DownloadState::Cancelled => {
            items.push(ContextMenuEntry::Item {
                action: ContextMenuAction::RestartFresh,
                label: "Download Again",
                icon: IconName::RotateCcw,
                enabled: true,
                danger: false,
            });
            items.push(ContextMenuEntry::Separator);
        }
        DownloadState::Completed => {}
    }

    let can_open_file = row.state == DownloadState::Completed && row.output_path.is_some();
    let has_path = row.output_path.is_some();

    items.push(ContextMenuEntry::Item {
        action: ContextMenuAction::OpenFile,
        label: "Open File",
        icon: IconName::File,
        enabled: can_open_file,
        danger: false,
    });
    items.push(ContextMenuEntry::Item {
        action: ContextMenuAction::ShowInFolder,
        label: "Show in Folder",
        icon: IconName::FolderOpen,
        enabled: has_path,
        danger: false,
    });
    items.push(ContextMenuEntry::Item {
        action: ContextMenuAction::CopyUrl,
        label: "Copy URL",
        icon: IconName::Copy,
        enabled: true,
        danger: false,
    });
    items.push(ContextMenuEntry::Item {
        action: ContextMenuAction::InspectByteMap,
        label: "Inspect Byte-Map",
        icon: IconName::PanelBottomOpen,
        enabled: true,
        danger: false,
    });
    if row.state == DownloadState::Completed {
        items.push(ContextMenuEntry::Item {
            action: ContextMenuAction::FileIntegrity,
            label: "File Integrity (BLAKE3)",
            icon: IconName::ShieldCheck,
            enabled: has_path,
            danger: false,
        });
        items.push(ContextMenuEntry::Item {
            action: ContextMenuAction::RenameFile,
            label: "Rename File...",
            icon: IconName::File,
            enabled: has_path,
            danger: false,
        });
        items.push(ContextMenuEntry::Item {
            action: ContextMenuAction::MoveTo,
            label: "Move To...",
            icon: IconName::Folder,
            enabled: has_path,
            danger: false,
        });
    }

    items.push(ContextMenuEntry::Separator);

    items.push(ContextMenuEntry::Item {
        action: ContextMenuAction::RemoveFromList,
        label: "Remove from List",
        icon: IconName::Trash,
        enabled: true,
        danger: false,
    });
    items.push(ContextMenuEntry::Item {
        action: ContextMenuAction::DeleteFileAndEntry,
        label: "Delete File & Entry",
        icon: IconName::Trash,
        enabled: has_path,
        danger: true,
    });

    items
}

/// Clamps the floating menu origin inside the current viewport bounds.
pub(crate) fn clamp_menu_position(
    anchor: Point<Pixels>,
    viewport: Size<Pixels>,
    menu_width: f32,
    menu_height: f32,
) -> Point<Pixels> {
    let max_x = (f32::from(viewport.width) - menu_width - MENU_EDGE_MARGIN).max(MENU_EDGE_MARGIN);
    let max_y = (f32::from(viewport.height) - menu_height - MENU_EDGE_MARGIN).max(MENU_EDGE_MARGIN);
    Point {
        x: px(f32::from(anchor.x).clamp(MENU_EDGE_MARGIN, max_x)),
        y: px(f32::from(anchor.y).clamp(MENU_EDGE_MARGIN, max_y)),
    }
}

/// Renders the floating download row context menu and its click-away backdrop.
pub(crate) fn render_context_menu<F1, F2>(
    row: &DownloadRowView,
    anchor: Point<Pixels>,
    viewport: Size<Pixels>,
    on_close: F1,
    on_action: F2,
) -> AnyElement
where
    F1: Fn(&mut Window, &mut App) + Clone + 'static,
    F2: Fn(DownloadId, ContextMenuAction, &mut Window, &mut App) + Clone + 'static,
{
    let entries = build_row_menu_items(row);
    let estimated_height = entries.iter().fold(10.0_f32, |acc, entry| {
        acc + match entry {
            ContextMenuEntry::Item { .. } => MENU_ITEM_HEIGHT,
            ContextMenuEntry::Separator => 9.0,
        }
    });
    let pos = clamp_menu_position(anchor, viewport, MENU_WIDTH, estimated_height);
    let on_close_right = on_close.clone();
    let download_id = row.id.clone();

    div()
        .id("context-menu-backdrop")
        .absolute()
        .inset_0()
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            on_close(window, cx);
        })
        .on_mouse_down(MouseButton::Right, move |_, window, cx| {
            on_close_right(window, cx);
        })
        .child(
            div()
                .id("context-menu-surface")
                .absolute()
                .left(pos.x)
                .top(pos.y)
                .w(px(MENU_WIDTH))
                .py_1()
                .bg(context_menu_bg())
                .border_1()
                .border_color(graphite_border())
                .rounded(px(4.0))
                .shadow_md()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                .children(entries.into_iter().enumerate().map(|(idx, entry)| match entry {
                    ContextMenuEntry::Separator => div()
                        .my_1()
                        .h(px(1.0))
                        .w_full()
                        .bg(graphite_border())
                        .into_any_element(),
                    ContextMenuEntry::Item {
                        action,
                        label,
                        icon,
                        enabled,
                        danger,
                    } => {
                        let text_color = if !enabled {
                            silver_muted().opacity(0.45)
                        } else if danger {
                            errata_vermilion()
                        } else {
                            silver_text()
                        };
                        let icon_color = if !enabled {
                            silver_muted().opacity(0.45)
                        } else if danger {
                            errata_vermilion()
                        } else {
                            silver_muted()
                        };
                        let id = download_id.clone();
                        let on_act = on_action.clone();

                        div()
                            .id(SharedString::from(format!("ctx-item-{idx}")))
                            .h(px(MENU_ITEM_HEIGHT))
                            .px_2p5()
                            .mx_1()
                            .rounded(px(3.0))
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(TYPE_BODY)
                            .line_height(px(18.0))
                            .text_color(text_color)
                            .when(enabled, |el| {
                                el.cursor_pointer()
                                    .hover(|s| {
                                        s.bg(if danger {
                                            errata_vermilion().opacity(0.14)
                                        } else {
                                            paper_hover()
                                        })
                                    })
                                    .on_click(move |_, window, cx| {
                                        on_act(id.clone(), action, window, cx);
                                    })
                            })
                            .child(Icon::new(icon).size(px(13.0)).text_color(icon_color))
                            .child(label)
                            .into_any_element()
                    }
                }))
                .with_animation(
                    "context-menu-enter",
                    Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
                    |el, delta| el.mt(px((1.0 - delta) * 4.0)).opacity(0.7 + 0.3 * delta),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use posthaze_engine::DownloadRecord;

    use super::*;

    fn sample_row(state: DownloadState, has_path: bool, resource_changed: bool) -> DownloadRowView {
        let record = DownloadRecord {
            id: DownloadId::from("dl-ctx-1".to_string()),
            url: "https://example.com/archive.tar.zst".to_string(),
            final_url: None,
            output_dir: Some(PathBuf::from("C:/Downloads")),
            output_path: has_path.then(|| PathBuf::from("C:/Downloads/archive.tar.zst")),
            part_path: None,
            state,
            content_length: Some(1024),
            bytes_downloaded: if state == DownloadState::Completed { 1024 } else { 256 },
            etag: None,
            last_modified: None,
            supports_ranges: true,
            max_workers: 4,
            sync_on_complete: false,
            headers: Vec::new(),
            error_message: resource_changed.then(|| "remote resource changed".to_string()),
            created_at: 1,
            updated_at: 1,
        };
        DownloadRowView::from_record(&record, None, 0, None)
    }

    #[::core::prelude::v1::test]
    fn active_row_menu_enables_pause_and_cancel_and_disables_open_file() {
        let row = sample_row(DownloadState::Active, true, false);
        let items = build_row_menu_items(&row);
        assert!(items.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::Pause,
                enabled: true,
                ..
            }
        )));
        assert!(items.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::Cancel,
                enabled: true,
                ..
            }
        )));
        assert!(items.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::OpenFile,
                enabled: false,
                ..
            }
        )));
    }

    #[::core::prelude::v1::test]
    fn completed_row_menu_enables_open_file_and_delete_file_danger() {
        let row = sample_row(DownloadState::Completed, true, false);
        let items = build_row_menu_items(&row);
        assert!(!items.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::RestartFresh,
                ..
            }
        )));
        assert!(items.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::OpenFile,
                enabled: true,
                ..
            }
        )));
        assert!(items.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::DeleteFileAndEntry,
                enabled: true,
                danger: true,
                ..
            }
        )));
    }

    #[::core::prelude::v1::test]
    fn resource_changed_failed_row_omits_resume_and_offers_restart_fresh() {
        let row = sample_row(DownloadState::Failed, true, true);
        let items = build_row_menu_items(&row);
        assert!(!items.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::Resume,
                ..
            }
        )));
        assert!(items.iter().any(|e| matches!(
            e,
            ContextMenuEntry::Item {
                action: ContextMenuAction::RestartFresh,
                enabled: true,
                ..
            }
        )));
    }

    #[::core::prelude::v1::test]
    fn clamp_menu_position_keeps_menu_inside_viewport() {
        let clamped = clamp_menu_position(
            Point {
                x: px(1390.0),
                y: px(880.0),
            },
            Size {
                width: px(1440.0),
                height: px(900.0),
            },
            224.0,
            260.0,
        );
        assert_eq!(f32::from(clamped.x), 1440.0 - 224.0 - 8.0);
        assert_eq!(f32::from(clamped.y), 900.0 - 260.0 - 8.0);
    }
}
