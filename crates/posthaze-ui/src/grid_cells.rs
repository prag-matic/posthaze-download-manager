//! Shared Table-Grid Cell UI primitives for Posthaze.
//!
//! Every interactive cell, section header, and key-value row across `table`, `sidebar`,
//! `inspector`, and modals composes from these primitives to maintain uniform `1px`
//! construction-grid rules, tabular monospace typography, and `selenium_amber()` hover fills.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

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
            Self::Danger => rgb(0xFFFFFF).into(),
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
        .when_some(icon, |el, ic| el.child(Icon::new(ic).size(px(12.0))))
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
        .h(px(34.0))
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
        .h(px(28.0))
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
    let (border_col, label_bg, label_fg, value_bg) = match tone {
        GridTone::Paper | GridTone::PaperSubtle => {
            (paper_border(), table_header_bg(), carbon_muted(), paper_bg())
        }
        GridTone::Graphite | GridTone::Primary | GridTone::Danger => (
            graphite_border(),
            graphite_elevated(),
            silver_muted(),
            graphite_bg(),
        ),
    };
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
