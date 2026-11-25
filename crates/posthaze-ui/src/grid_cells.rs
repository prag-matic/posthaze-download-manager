//! Shared Table-Grid Cell UI primitives for Posthaze.
//!
//! Every interactive cell, section header, and key-value row across `table`, `sidebar`,
//! `inspector`, and modals composes from these primitives to maintain uniform `1px`
//! construction-grid rules, tabular monospace typography, and `selenium_amber()` hover fills.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::assets::BRAND_LOGO_SVG;
use crate::theme::*;

/// Surface polarity & semantic emphasis for a grid cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GridTone {
    /// Table/toolbar surface (`paper_bg` / `table_header_bg`, `paper_border`).
    Paper,
    /// Subtle row-inline surface (`table_header_bg().opacity(0.45)`, `paper_border`).
    PaperSubtle,
    /// Dark rail, inspector, and modal surface (`graphite_elevated`, `graphite_border`).
    Graphite,
    /// High-emphasis primary action cell (`selenium_amber` fill at rest).
    Primary,
    /// Destructive action cell (`errata_vermilion` on hover).
    Danger,
}

/// Which vertical `1px` cell borders to draw on a [`grid_cell_button`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellBorder {
    None,
    Left,
    Right,
    Both,
}

impl GridTone {
    pub(crate) fn border_color(self) -> Hsla {
        match self {
            Self::Paper | Self::PaperSubtle => paper_border(),
            Self::Graphite | Self::Primary | Self::Danger => graphite_border(),
        }
    }

    pub(crate) fn rest_bg(self) -> Hsla {
        match self {
            Self::Paper => table_header_bg(),
            Self::PaperSubtle => table_header_bg().opacity(0.45),
            Self::Graphite => graphite_elevated(),
            Self::Primary => selenium_amber(),
            Self::Danger => graphite_elevated(),
        }
    }

    pub(crate) fn rest_fg(self) -> Hsla {
        match self {
            Self::Paper | Self::PaperSubtle => carbon_text(),
            Self::Graphite => silver_text(),
            Self::Primary => active_theme().accent_foreground.hsla(),
            Self::Danger => errata_vermilion(),
        }
    }

    pub(crate) fn hover_bg(self) -> Hsla {
        match self {
            Self::Paper | Self::PaperSubtle | Self::Graphite => selenium_amber(),
            Self::Primary => active_theme().accent_hover.hsla(),
            Self::Danger => errata_vermilion(),
        }
    }

    pub(crate) fn hover_fg(self) -> Hsla {
        match self {
            Self::Paper | Self::PaperSubtle | Self::Graphite | Self::Primary => {
                active_theme().accent_foreground.hsla()
            }
            Self::Danger => danger_foreground(),
        }
    }
}

/// Full-height rectangular table-cell button with `1px` vertical dividers and accent hover fill.
#[allow(clippy::too_many_arguments)]
pub(crate) fn grid_cell_button(
    id: impl Into<ElementId>,
    icon: Option<IconName>,
    label: Option<&'static str>,
    tooltip_text: Option<&'static str>,
    tone: GridTone,
    border: CellBorder,
    compact: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    grid_cell_button_dyn(
        id,
        icon,
        label.map(SharedString::from),
        tooltip_text,
        tone,
        border,
        compact,
        on_click,
    )
}

/// Same as [`grid_cell_button`] but accepts a runtime-built label (for example `PAUSE (3)`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn grid_cell_button_dyn(
    id: impl Into<ElementId>,
    icon: Option<IconName>,
    label: Option<SharedString>,
    tooltip_text: Option<&'static str>,
    tone: GridTone,
    border: CellBorder,
    compact: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let border_col = tone.border_color();
    let has_left = matches!(border, CellBorder::Left | CellBorder::Both);
    let has_right = matches!(border, CellBorder::Right | CellBorder::Both);

    div()
        .id(id)
        .h_full()
        .when(compact, |el| el.px_2p5())
        .when(!compact, |el| el.px_3())
        .when(has_left, |el| el.border_l_1().border_color(border_col))
        .when(has_right, |el| el.border_r_1().border_color(border_col))
        .bg(tone.rest_bg())
        .hover(move |s| s.bg(tone.hover_bg()).text_color(tone.hover_fg()))
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .gap_1p5()
        .font_family(MONO_FONT)
        .text_size(TYPE_MICRO)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(tone.rest_fg())
        .when_some(tooltip_text, |el, tip| {
            el.tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        })
        .on_click(on_click)
        .when_some(icon, |el, ic| el.child(Icon::new(ic).size(px(14.0))))
        .when_some(label, |el, text| el.child(text))
        .into_any_element()
}

/// Standalone `1px`-bordered cell button with explicit `height` (for inline/empty-state CTAs).
#[allow(clippy::too_many_arguments)]
pub(crate) fn grid_boxed_button(
    id: impl Into<ElementId>,
    height: Pixels,
    icon: Option<IconName>,
    label: Option<&'static str>,
    tooltip_text: Option<&'static str>,
    tone: GridTone,
    compact: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    div()
        .h(height)
        .border_1()
        .border_color(tone.border_color())
        .child(grid_cell_button(
            id,
            icon,
            label,
            tooltip_text,
            tone,
            CellBorder::None,
            compact,
            on_click,
        ))
        .into_any_element()
}

/// Flush `34px` modal footer bar with `1px` top border and right-aligned action cells.
pub(crate) fn grid_modal_footer(tone: GridTone) -> Div {
    div()
        .h(MODAL_FOOTER_HEIGHT)
        .w_full()
        .border_t_1()
        .border_color(tone.border_color())
        .flex()
        .justify_end()
}

/// `28px` modular grid section header bar with `1px` bottom rule.
pub(crate) fn grid_section_header(title: impl Into<SharedString>, tone: GridTone) -> Div {
    let (bg, border_col, fg) = match tone {
        GridTone::Paper | GridTone::PaperSubtle => (table_header_bg(), paper_border(), carbon_muted()),
        GridTone::Graphite | GridTone::Primary | GridTone::Danger => {
            (graphite_elevated(), graphite_border(), silver_muted())
        }
    };
    div()
        .h(SECTION_HEADER_HEIGHT)
        .w_full()
        .bg(bg)
        .border_b_1()
        .border_color(border_col)
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .px_3()
                .font_family(MONO_FONT)
                .text_size(TYPE_HEADER)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .child(title.into()),
        )
}

/// Two-column `LABEL | VALUE` grid row with `1px` horizontal and vertical rules.
pub(crate) fn grid_kv_row(
    label: &'static str,
    label_width: f32,
    value: impl IntoElement,
    tone: GridTone,
) -> AnyElement {
    grid_kv_row_with_bg(label, label_width, value, tone, None)
}

/// Two-column `LABEL | VALUE` grid row with an optional value-cell background override
/// (for example `input_bg()` on recessed modal form fields).
pub(crate) fn grid_kv_row_with_bg(
    label: &'static str,
    label_width: f32,
    value: impl IntoElement,
    tone: GridTone,
    value_bg_override: Option<Hsla>,
) -> AnyElement {
    let (border_col, label_bg, label_fg, default_value_bg) = match tone {
        GridTone::Paper | GridTone::PaperSubtle => {
            (paper_border(), table_header_bg(), carbon_muted(), paper_bg())
        }
        GridTone::Graphite | GridTone::Primary | GridTone::Danger => (
            graphite_border(),
            graphite_elevated(),
            silver_muted(),
            Hsla::transparent_black(),
        ),
    };
    let value_bg = value_bg_override.unwrap_or(default_value_bg);
    div()
        .min_h(px(32.0))
        .w_full()
        .border_b_1()
        .border_color(border_col)
        .flex()
        .items_stretch()
        .child(
            div()
                .w(px(label_width))
                .flex_shrink_0()
                .px_3()
                .py_1p5()
                .bg(label_bg)
                .border_r_1()
                .border_color(border_col)
                .flex()
                .items_center()
                .font_family(MONO_FONT)
                .text_size(TYPE_MICRO)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(label_fg)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .px_3()
                .py_1p5()
                .bg(value_bg)
                .flex()
                .items_center()
                .child(value),
        )
        .into_any_element()
}

/// Semantic emphasis for a stamped monospace [`grid_badge`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BadgeTone {
    /// Standard neutral badge (`silver_text()` on `graphite_elevated()`, `graphite_border()`).
    Neutral,
    /// Muted metadata badge (`silver_muted()` on `graphite_elevated()`, `graphite_border()`).
    Muted,
    /// Active / highlighted badge (`selenium_amber()` ink on `graphite_elevated()`).
    Accent,
    /// Verified / completed badge (`halide_emerald()` ink and border).
    Success,
    /// Error / mismatch badge (`errata_vermilion()` ink and border).
    Danger,
}

/// Stamped monospace badge (`3px` radius, `1px` border, `Consolas` `11px`) for validators,
/// protocol tags, FIFO ranks, and verification verdicts.
pub(crate) fn grid_badge(label: impl Into<SharedString>, tone: BadgeTone) -> Div {
    let (border_col, text_col, weight) = match tone {
        BadgeTone::Neutral => (graphite_border(), silver_text(), FontWeight::MEDIUM),
        BadgeTone::Muted => (graphite_border(), silver_muted(), FontWeight::MEDIUM),
        BadgeTone::Accent => (graphite_border(), selenium_amber(), FontWeight::SEMIBOLD),
        BadgeTone::Success => (halide_emerald(), halide_emerald(), FontWeight::SEMIBOLD),
        BadgeTone::Danger => (errata_vermilion(), errata_vermilion(), FontWeight::SEMIBOLD),
    };
    div()
        .px_2()
        .py_0p5()
        .rounded(RADIUS_BADGE)
        .bg(graphite_elevated())
        .border_1()
        .border_color(border_col)
        .font_family(MONO_FONT)
        .text_size(TYPE_MICRO)
        .line_height(px(14.0))
        .font_weight(weight)
        .text_color(text_col)
        .child(label.into())
}

/// Full-height `3px` vertical selection/active indicator bar (`selenium_amber()` when active).
pub(crate) fn grid_selection_bar(selected: bool) -> Div {
    div()
        .w(INDICATOR_BAR_WIDTH)
        .flex_shrink_0()
        .h_full()
        .bg(if selected {
            selenium_amber()
        } else {
            Hsla::transparent_black()
        })
}

/// Surface polarity for a vertical navigation row in [`grid_nav_row`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavRowTone {
    /// Sidebar rail (`graphite_elevated()` selected and hovered).
    Graphite,
    /// Settings section rail (`paper_selected()` selected, `paper_hover()` hovered).
    PaperSelection,
}

/// Vertical navigation item (`34px`) with [`grid_selection_bar`], `14px` icon, uppercase monospace
/// label, and optional right-bordered count cell.
pub(crate) fn grid_nav_row(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    selected: bool,
    trailing_count: Option<usize>,
    tone: NavRowTone,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> AnyElement {
    let (selected_bg, hover_bg) = match tone {
        NavRowTone::Graphite => (graphite_elevated(), graphite_elevated()),
        NavRowTone::PaperSelection => (paper_selected(), paper_hover()),
    };
    let icon_color = if selected {
        selenium_amber()
    } else {
        silver_muted()
    };
    let text_color = if selected {
        silver_text()
    } else {
        silver_muted()
    };

    div()
        .id(id)
        .h(px(34.0))
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .bg(if selected {
            selected_bg
        } else {
            Hsla::transparent_black()
        })
        .border_b_1()
        .border_color(graphite_border())
        .hover(move |s| s.bg(hover_bg))
        .on_click(on_click)
        .child(
            div()
                .h_full()
                .flex()
                .items_center()
                .child(grid_selection_bar(selected).mr_2p5())
                .child(Icon::new(icon).size(px(14.0)).text_color(icon_color))
                .child(
                    div()
                        .ml_2()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text_color)
                        .child(label.into()),
                ),
        )
        .when_some(trailing_count, |el, count| {
            el.child(
                div()
                    .w(px(44.0))
                    .h_full()
                    .border_l_1()
                    .border_color(graphite_border())
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family(MONO_FONT)
                    .text_size(TYPE_MICRO)
                    .text_color(text_color)
                    .child(count.to_string()),
            )
        })
        .into_any_element()
}

/// Canonical `TitleBar` brand cell (`22px` Posthaze mark + `POSTHAZE` monospace plate)
/// bounded by a `1px` `graphite_border()` right rule.
pub(crate) fn grid_brand_plate() -> Div {
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
                .size(px(22.0))
                .text_color(selenium_amber()),
        )
        .child(
            div()
                .font_family(MONO_FONT)
                .text_size(TYPE_MICRO)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(silver_text())
                .child("POSTHAZE"),
        )
}

/// Monospace borderless `Input` container for recessed toolbar and modal fields.
pub(crate) fn grid_input_box(state: &Entity<InputState>) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .font_family(MONO_FONT)
        .text_size(TYPE_BODY)
        .child(Input::new(state).appearance(false))
}

/// Compact `30px` `LABEL | INPUT | TRAILING` form row with `1px` `graphite_border()` framing.
pub(crate) fn grid_compact_input_row(
    label: &'static str,
    label_width: f32,
    state: &Entity<InputState>,
    trailing_action: Option<AnyElement>,
) -> Div {
    div()
        .w_full()
        .h(px(30.0))
        .bg(input_bg())
        .border_1()
        .border_color(graphite_border())
        .flex()
        .items_center()
        .child(
            div()
                .w(px(label_width))
                .flex_shrink_0()
                .h_full()
                .px_2()
                .bg(graphite_elevated())
                .border_r_1()
                .border_color(graphite_border())
                .flex()
                .items_center()
                .font_family(MONO_FONT)
                .text_size(TYPE_MICRO)
                .text_color(silver_muted())
                .child(label),
        )
        .child(grid_input_box(state).px_1p5())
        .when_some(trailing_action, |el, action| el.child(action))
}

/// Small square or circular status indicator mark.
pub(crate) fn grid_status_dot(color: Hsla, size_px: f32, round: bool) -> Div {
    let dot = div().w(px(size_px)).h(px(size_px)).flex_shrink_0().bg(color);
    if round {
        dot.rounded(px(size_px / 2.0))
    } else {
        dot
    }
}

/// Floating popover / menu plate using `context_menu_bg()`, `1px` `graphite_border()`,
/// `RADIUS_CONTROL`, and `shadow_md()`.
pub(crate) fn grid_dropdown_surface(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .bg(context_menu_bg())
        .border_1()
        .border_color(graphite_border())
        .rounded(RADIUS_CONTROL)
        .shadow_md()
}

/// Checkable option row (`paper_selected()` / `paper_hover()`, `12px` checkmark slot,
/// monospace label, and optional right-aligned metadata) shared across dropdown panels.
pub(crate) fn grid_check_option_row(
    id: impl Into<ElementId>,
    height: Pixels,
    label: impl Into<SharedString>,
    selected: bool,
    trailing: Option<AnyElement>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(height)
        .px_2p5()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .cursor_pointer()
        .bg(if selected {
            paper_selected()
        } else {
            Hsla::transparent_black()
        })
        .hover(|s| s.bg(paper_hover()))
        .on_click(on_click)
        .child(
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .min_w_0()
                .child(
                    div()
                        .w(px(12.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(selected, |el| {
                            el.child(
                                Icon::new(IconName::Check)
                                    .size(px(12.0))
                                    .text_color(selenium_amber()),
                            )
                        }),
                )
                .child(
                    div()
                        .font_family(MONO_FONT)
                        .text_size(TYPE_MICRO)
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(if selected {
                            silver_text()
                        } else {
                            silver_muted()
                        })
                        .truncate()
                        .child(label.into()),
                ),
        )
        .when_some(trailing, |el, right| el.child(right))
}

/// Recessed `0px`-radius progress / byte-map track (`byte_track_bg()`) with optional `1px` frame.
pub(crate) fn grid_progress_track(height: Pixels, border_color: Option<Hsla>) -> Div {
    div()
        .h(height)
        .w_full()
        .bg(byte_track_bg())
        .when_some(border_color, |el, col| el.border_1().border_color(col))
        .flex()
        .overflow_hidden()
}
