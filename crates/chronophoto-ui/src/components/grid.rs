//! Table-Grid Cell `RenderOnce` primitives for Chronophoto.

use gpui_kit::assets::{IconName, IconNamed};
use gpui_kit::component::Icon;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::components::resolve_mono_font;
use crate::components::tones::{BadgeTone, CellBorder, GridTone, NavRowTone};
use crate::theme::ChronoAppExt;
use crate::tokens::{
    INDICATOR_BAR_WIDTH, MODAL_FOOTER_HEIGHT, RADIUS_BADGE, RADIUS_CONTROL, SECTION_HEADER_HEIGHT,
    TYPE_BODY, TYPE_HEADER, TYPE_MICRO,
};

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Full-height rectangular table-cell button with `1px` vertical dividers and accent hover fill.
#[derive(IntoElement)]
pub struct GridCellButton {
    id: ElementId,
    icon_path: Option<SharedString>,
    label: Option<SharedString>,
    tooltip: Option<SharedString>,
    tone: GridTone,
    border: CellBorder,
    compact: bool,
    on_click: Option<ClickHandler>,
}

impl GridCellButton {
    /// Creates a new [`GridCellButton`] with the given element `id`.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            icon_path: None,
            label: None,
            tooltip: None,
            tone: GridTone::Chrome,
            border: CellBorder::None,
            compact: false,
            on_click: None,
        }
    }

    /// Sets the button icon from any [`IconNamed`] value (Lucide [`IconName`] or custom SVG enum).
    pub fn icon(mut self, icon: impl IconNamed) -> Self {
        self.icon_path = Some(icon.path());
        self
    }

    /// Sets an optional icon from any [`IconNamed`] value.
    pub fn maybe_icon(mut self, icon: Option<impl IconNamed>) -> Self {
        self.icon_path = icon.map(IconNamed::path);
        self
    }

    /// Sets the button icon from a raw asset path.
    pub fn icon_path(mut self, path: impl Into<SharedString>) -> Self {
        self.icon_path = Some(path.into());
        self
    }

    /// Sets the button label text.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets an optional button label text.
    pub fn maybe_label(mut self, label: Option<impl Into<SharedString>>) -> Self {
        self.label = label.map(Into::into);
        self
    }

    /// Sets a hover tooltip string.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Sets an optional hover tooltip string.
    pub fn maybe_tooltip(mut self, tooltip: Option<impl Into<SharedString>>) -> Self {
        self.tooltip = tooltip.map(Into::into);
        self
    }

    /// Sets the surface polarity and emphasis tone.
    pub fn tone(mut self, tone: GridTone) -> Self {
        self.tone = tone;
        self
    }

    /// Sets which vertical `1px` cell dividers to render.
    pub fn border(mut self, border: CellBorder) -> Self {
        self.border = border;
        self
    }

    /// Enables tighter horizontal padding (`px_2p5` instead of `px_3`).
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Registers a click callback.
    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for GridCellButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let text_size = chrono.typography.scale(TYPE_MICRO);
        let mono = resolve_mono_font(cx);

        let border_col = self.tone.border_color(palette);
        let rest_bg = self.tone.rest_bg(palette);
        let rest_fg = self.tone.rest_fg(palette);
        let hover_bg = self.tone.hover_bg(palette);
        let hover_fg = self.tone.hover_fg(palette);

        div()
            .id(self.id)
            .h_full()
            .when(self.compact, |el| el.px_2p5())
            .when(!self.compact, |el| el.px_3())
            .when(self.border.has_left(), |el| el.border_l_1().border_color(border_col))
            .when(self.border.has_right(), |el| el.border_r_1().border_color(border_col))
            .bg(rest_bg)
            .hover(move |s| s.bg(hover_bg).text_color(hover_fg))
            .cursor_pointer()
            .flex()
            .items_center()
            .justify_center()
            .gap_1p5()
            .font_family(mono)
            .text_size(text_size)
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rest_fg)
            .when_some(self.tooltip, |el, tip| {
                el.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            })
            .when_some(self.on_click, |el, handler| {
                el.on_click(move |ev, window, cx| handler(ev, window, cx))
            })
            .when_some(self.icon_path, |el, path| {
                el.child(Icon::default().path(path).size(px(14.0)))
            })
            .when_some(self.label, |el, text| el.child(text))
    }
}

/// Standalone `1px`-bordered cell button with explicit `height` (for inline/empty-state CTAs).
#[derive(IntoElement)]
pub struct GridBoxedButton {
    height: Pixels,
    inner: GridCellButton,
}

impl GridBoxedButton {
    /// Creates a new [`GridBoxedButton`] with the given element `id` and default height (`28px`).
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            height: px(28.0),
            inner: GridCellButton::new(id),
        }
    }

    /// Sets the explicit outer height of the boxed button.
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Sets the button icon from any [`IconNamed`] value.
    pub fn icon(mut self, icon: impl IconNamed) -> Self {
        self.inner = self.inner.icon(icon);
        self
    }

    /// Sets an optional icon from any [`IconNamed`] value.
    pub fn maybe_icon(mut self, icon: Option<impl IconNamed>) -> Self {
        self.inner = self.inner.maybe_icon(icon);
        self
    }

    /// Sets the button icon from a raw asset path.
    pub fn icon_path(mut self, path: impl Into<SharedString>) -> Self {
        self.inner = self.inner.icon_path(path);
        self
    }

    /// Sets the button label text.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.inner = self.inner.label(label);
        self
    }

    /// Sets an optional button label text.
    pub fn maybe_label(mut self, label: Option<impl Into<SharedString>>) -> Self {
        self.inner = self.inner.maybe_label(label);
        self
    }

    /// Sets a hover tooltip string.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.inner = self.inner.tooltip(tooltip);
        self
    }

    /// Sets the surface polarity and emphasis tone.
    pub fn tone(mut self, tone: GridTone) -> Self {
        self.inner = self.inner.tone(tone);
        self
    }

    /// Enables tighter horizontal padding.
    pub fn compact(mut self, compact: bool) -> Self {
        self.inner = self.inner.compact(compact);
        self
    }

    /// Registers a click callback.
    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.inner = self.inner.on_click(handler);
        self
    }
}

impl RenderOnce for GridBoxedButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let border_col = self.inner.tone.border_color(&cx.chrono().palette);
        div()
            .h(self.height)
            .border_1()
            .border_color(border_col)
            .child(self.inner)
    }
}

/// `28px` modular grid section header bar with `1px` bottom rule.
#[derive(IntoElement)]
pub struct GridSectionHeader {
    title: SharedString,
    tone: GridTone,
    trailing: Option<AnyElement>,
}

impl GridSectionHeader {
    /// Creates a new [`GridSectionHeader`] with the given `title`.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            tone: GridTone::Chrome,
            trailing: None,
        }
    }

    /// Sets the surface polarity tone.
    pub fn tone(mut self, tone: GridTone) -> Self {
        self.tone = tone;
        self
    }

    /// Sets an optional right-aligned trailing element.
    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing = Some(trailing.into_any_element());
        self
    }
}

impl RenderOnce for GridSectionHeader {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let text_size = chrono.typography.scale(TYPE_HEADER);
        let mono = resolve_mono_font(cx);

        let bg = self.tone.header_bg(palette);
        let border_col = self.tone.border_color(palette);
        let fg = self.tone.muted_fg(palette);

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
                    .font_family(mono)
                    .text_size(text_size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(fg)
                    .child(self.title),
            )
            .when_some(self.trailing, |el, right| el.child(right))
    }
}

/// Two-column `LABEL | VALUE` grid row with `1px` horizontal and vertical rules.
#[derive(IntoElement)]
pub struct GridKvRow {
    label: SharedString,
    label_width: f32,
    value: AnyElement,
    tone: GridTone,
    value_bg: Option<Hsla>,
}

impl GridKvRow {
    /// Creates a new [`GridKvRow`] with `label` and `value` element.
    pub fn new(label: impl Into<SharedString>, value: impl IntoElement) -> Self {
        Self {
            label: label.into(),
            label_width: 140.0,
            value: value.into_any_element(),
            tone: GridTone::Chrome,
            value_bg: None,
        }
    }

    /// Sets the fixed width in logical pixels for the left label column (default `140.0`).
    pub fn label_width(mut self, width_px: f32) -> Self {
        self.label_width = width_px;
        self
    }

    /// Sets the surface polarity tone.
    pub fn tone(mut self, tone: GridTone) -> Self {
        self.tone = tone;
        self
    }

    /// Overrides the right-hand value cell background fill.
    pub fn value_bg(mut self, bg: Hsla) -> Self {
        self.value_bg = Some(bg);
        self
    }
}

impl RenderOnce for GridKvRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let text_size = chrono.typography.scale(TYPE_MICRO);
        let mono = resolve_mono_font(cx);

        let border_col = self.tone.border_color(palette);
        let label_bg = self.tone.header_bg(palette);
        let label_fg = self.tone.muted_fg(palette);
        let value_bg = self
            .value_bg
            .unwrap_or_else(|| self.tone.kv_value_bg(palette));

        div()
            .min_h(px(32.0))
            .w_full()
            .border_b_1()
            .border_color(border_col)
            .flex()
            .items_stretch()
            .child(
                div()
                    .w(px(self.label_width))
                    .flex_shrink_0()
                    .px_3()
                    .py_1p5()
                    .bg(label_bg)
                    .border_r_1()
                    .border_color(border_col)
                    .flex()
                    .items_center()
                    .font_family(mono)
                    .text_size(text_size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(label_fg)
                    .child(self.label),
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
                    .child(self.value),
            )
    }
}

/// Flush `34px` modal footer bar with `1px` top border and right-aligned action cells.
#[derive(IntoElement)]
pub struct GridModalFooter {
    tone: GridTone,
    children: Vec<AnyElement>,
}

impl Default for GridModalFooter {
    fn default() -> Self {
        Self::new()
    }
}

impl GridModalFooter {
    /// Creates an empty [`GridModalFooter`] defaulting to [`GridTone::Chrome`].
    pub fn new() -> Self {
        Self {
            tone: GridTone::Chrome,
            children: Vec::new(),
        }
    }

    /// Sets the surface polarity tone for the top border rule.
    pub fn tone(mut self, tone: GridTone) -> Self {
        self.tone = tone;
        self
    }
}

impl ParentElement for GridModalFooter {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for GridModalFooter {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let border_col = self.tone.border_color(&cx.chrono().palette);
        div()
            .h(MODAL_FOOTER_HEIGHT)
            .w_full()
            .border_t_1()
            .border_color(border_col)
            .flex()
            .justify_end()
            .children(self.children)
    }
}

/// Stamped monospace badge (`3px` radius, `1px` border, `11px` monospace) for status tags,
/// protocol labels, and verification verdicts.
#[derive(IntoElement)]
pub struct GridBadge {
    label: SharedString,
    tone: BadgeTone,
}

impl GridBadge {
    /// Creates a new [`GridBadge`] with the given `label` and [`BadgeTone::Neutral`].
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            tone: BadgeTone::Neutral,
        }
    }

    /// Sets the semantic badge tone.
    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self
    }
}

impl RenderOnce for GridBadge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let text_size = chrono.typography.scale(TYPE_MICRO);
        let mono = resolve_mono_font(cx);

        div()
            .px_2()
            .py_0p5()
            .rounded(RADIUS_BADGE)
            .bg(palette.chrome_elevated.hsla())
            .border_1()
            .border_color(self.tone.border_color(palette))
            .font_family(mono)
            .text_size(text_size)
            .line_height(px(14.0))
            .font_weight(self.tone.font_weight())
            .text_color(self.tone.text_color(palette))
            .child(self.label)
    }
}

/// Full-height `3px` vertical selection/active indicator bar (`accent` when active).
#[derive(IntoElement)]
pub struct GridSelectionBar {
    selected: bool,
}

impl GridSelectionBar {
    /// Creates a new [`GridSelectionBar`] reflecting `selected`.
    pub fn new(selected: bool) -> Self {
        Self { selected }
    }
}

impl RenderOnce for GridSelectionBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let bg = if self.selected {
            cx.chrono().palette.accent.hsla()
        } else {
            Hsla::transparent_black()
        };
        div()
            .w(INDICATOR_BAR_WIDTH)
            .flex_shrink_0()
            .h_full()
            .bg(bg)
    }
}

/// Vertical navigation item (`34px`) with [`GridSelectionBar`], `14px` icon, uppercase monospace
/// label, and optional right-bordered count cell.
#[derive(IntoElement)]
pub struct GridNavRow {
    id: ElementId,
    icon_path: SharedString,
    label: SharedString,
    selected: bool,
    trailing_count: Option<usize>,
    tone: NavRowTone,
    on_click: Option<ClickHandler>,
}

impl GridNavRow {
    /// Creates a new [`GridNavRow`] with `id`, an [`IconNamed`] icon, and `label`.
    pub fn new(
        id: impl Into<ElementId>,
        icon: impl IconNamed,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            icon_path: icon.path(),
            label: label.into(),
            selected: false,
            trailing_count: None,
            tone: NavRowTone::Chrome,
            on_click: None,
        }
    }

    /// Creates a new [`GridNavRow`] using a raw icon asset path.
    pub fn with_icon_path(
        id: impl Into<ElementId>,
        icon_path: impl Into<SharedString>,
        label: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            icon_path: icon_path.into(),
            label: label.into(),
            selected: false,
            trailing_count: None,
            tone: NavRowTone::Chrome,
            on_click: None,
        }
    }

    /// Marks the navigation row as active/selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Sets an optional right-bordered count cell.
    pub fn trailing_count(mut self, count: Option<usize>) -> Self {
        self.trailing_count = count;
        self
    }

    /// Sets the surface polarity tone.
    pub fn tone(mut self, tone: NavRowTone) -> Self {
        self.tone = tone;
        self
    }

    /// Registers a click callback.
    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for GridNavRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let text_size = chrono.typography.scale(TYPE_MICRO);
        let mono = resolve_mono_font(cx);

        let selected_bg = self.tone.selected_bg(palette);
        let hover_bg = self.tone.hover_bg(palette);
        let icon_color = if self.selected {
            palette.accent.hsla()
        } else {
            palette.chrome_muted.hsla()
        };
        let text_color = if self.selected {
            palette.chrome_text.hsla()
        } else {
            palette.chrome_muted.hsla()
        };
        let border_col = palette.chrome_border.hsla();

        div()
            .id(self.id)
            .h(px(34.0))
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .cursor_pointer()
            .bg(if self.selected {
                selected_bg
            } else {
                Hsla::transparent_black()
            })
            .border_b_1()
            .border_color(border_col)
            .hover(move |s| s.bg(hover_bg))
            .when_some(self.on_click, |el, handler| {
                el.on_click(move |ev, window, cx| handler(ev, window, cx))
            })
            .child(
                div()
                    .h_full()
                    .flex()
                    .items_center()
                    .child(div().mr_2p5().h_full().child(GridSelectionBar::new(self.selected)))
                    .child(
                        Icon::default()
                            .path(self.icon_path)
                            .size(px(14.0))
                            .text_color(icon_color),
                    )
                    .child(
                        div()
                            .ml_2()
                            .font_family(mono.clone())
                            .text_size(text_size)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(text_color)
                            .child(self.label),
                    ),
            )
            .when_some(self.trailing_count, |el, count| {
                el.child(
                    div()
                        .w(px(44.0))
                        .h_full()
                        .border_l_1()
                        .border_color(border_col)
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_family(mono)
                        .text_size(text_size)
                        .text_color(text_color)
                        .child(count.to_string()),
                )
            })
    }
}

/// Monospace borderless `Input` container for recessed toolbar and modal fields.
#[derive(IntoElement)]
pub struct GridInputBox {
    state: Entity<InputState>,
}

impl GridInputBox {
    /// Creates a new [`GridInputBox`] bound to `state`.
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
        }
    }
}

impl RenderOnce for GridInputBox {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let text_size = cx.chrono().typography.scale(TYPE_BODY);
        let mono = resolve_mono_font(cx);

        div()
            .flex_1()
            .min_w_0()
            .font_family(mono)
            .text_size(text_size)
            .child(Input::new(&self.state).appearance(false))
    }
}

/// Compact `30px` `LABEL | INPUT | TRAILING` form row with `1px` `chrome_border` framing.
#[derive(IntoElement)]
pub struct GridCompactInputRow {
    label: SharedString,
    label_width: f32,
    state: Entity<InputState>,
    trailing_action: Option<AnyElement>,
}

impl GridCompactInputRow {
    /// Creates a new [`GridCompactInputRow`] with `label` and `state`.
    pub fn new(label: impl Into<SharedString>, state: &Entity<InputState>) -> Self {
        Self {
            label: label.into(),
            label_width: 68.0,
            state: state.clone(),
            trailing_action: None,
        }
    }

    /// Sets the fixed width in logical pixels for the left label cell (default `68.0`).
    pub fn label_width(mut self, width_px: f32) -> Self {
        self.label_width = width_px;
        self
    }

    /// Sets an optional trailing action element (for example a browse button).
    pub fn trailing_action(mut self, action: impl IntoElement) -> Self {
        self.trailing_action = Some(action.into_any_element());
        self
    }
}

impl RenderOnce for GridCompactInputRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let text_size = chrono.typography.scale(TYPE_MICRO);
        let mono = resolve_mono_font(cx);
        let border_col = palette.chrome_border.hsla();

        div()
            .w_full()
            .h(px(30.0))
            .bg(palette.input_bg.hsla())
            .border_1()
            .border_color(border_col)
            .flex()
            .items_center()
            .child(
                div()
                    .w(px(self.label_width))
                    .flex_shrink_0()
                    .h_full()
                    .px_2()
                    .bg(palette.chrome_elevated.hsla())
                    .border_r_1()
                    .border_color(border_col)
                    .flex()
                    .items_center()
                    .font_family(mono)
                    .text_size(text_size)
                    .text_color(palette.chrome_muted.hsla())
                    .child(self.label),
            )
            .child(div().flex_1().min_w_0().px_1p5().child(GridInputBox::new(&self.state)))
            .when_some(self.trailing_action, |el, action| el.child(action))
    }
}

/// Small square or circular status indicator mark.
#[derive(IntoElement)]
pub struct GridStatusDot {
    color: Hsla,
    size_px: f32,
    round: bool,
}

impl GridStatusDot {
    /// Creates a new [`GridStatusDot`] with the given `color` (default `8px`, circular).
    pub fn new(color: Hsla) -> Self {
        Self {
            color,
            size_px: 8.0,
            round: true,
        }
    }

    /// Sets the dot width and height in logical pixels (default `8.0`).
    pub fn size(mut self, size_px: f32) -> Self {
        self.size_px = size_px;
        self
    }

    /// Sets whether the dot is circular (`true`) or square (`false`).
    pub fn round(mut self, round: bool) -> Self {
        self.round = round;
        self
    }
}

impl RenderOnce for GridStatusDot {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let dot = div()
            .w(px(self.size_px))
            .h(px(self.size_px))
            .flex_shrink_0()
            .bg(self.color);
        if self.round {
            dot.rounded(px(self.size_px / 2.0))
        } else {
            dot
        }
    }
}

/// Floating popover / menu plate using `popover_bg`, `1px` `chrome_border`, `RADIUS_CONTROL`,
/// and `shadow_md()`.
#[derive(IntoElement)]
pub struct GridDropdownSurface {
    id: ElementId,
    children: Vec<AnyElement>,
}

impl GridDropdownSurface {
    /// Creates a new [`GridDropdownSurface`] with the given `id`.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            children: Vec::new(),
        }
    }
}

impl ParentElement for GridDropdownSurface {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for GridDropdownSurface {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = &cx.chrono().palette;
        div()
            .id(self.id)
            .bg(palette.popover_bg.hsla())
            .border_1()
            .border_color(palette.chrome_border.hsla())
            .rounded(RADIUS_CONTROL)
            .shadow_md()
            .children(self.children)
    }
}

/// Checkable option row (`surface_selected` / `surface_hover`, `12px` checkmark slot,
/// monospace label, and optional right-aligned metadata) shared across dropdown panels.
#[derive(IntoElement)]
pub struct GridCheckOptionRow {
    id: ElementId,
    height: Pixels,
    label: SharedString,
    selected: bool,
    trailing: Option<AnyElement>,
    on_click: Option<ClickHandler>,
}

impl GridCheckOptionRow {
    /// Creates a new [`GridCheckOptionRow`] with `id` and `label` (default height `28px`).
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            height: px(28.0),
            label: label.into(),
            selected: false,
            trailing: None,
            on_click: None,
        }
    }

    /// Sets the row height (default `28px`).
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Marks this option row as selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Sets an optional right-aligned metadata element.
    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing = Some(trailing.into_any_element());
        self
    }

    /// Registers a click callback.
    pub fn on_click(mut self, handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for GridCheckOptionRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let text_size = chrono.typography.scale(TYPE_MICRO);
        let mono = resolve_mono_font(cx);
        let hover_bg = palette.surface_hover.hsla();

        div()
            .id(self.id)
            .h(self.height)
            .px_2p5()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .cursor_pointer()
            .bg(if self.selected {
                palette.surface_selected.hsla()
            } else {
                Hsla::transparent_black()
            })
            .hover(move |s| s.bg(hover_bg))
            .when_some(self.on_click, |el, handler| {
                el.on_click(move |ev, window, cx| handler(ev, window, cx))
            })
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
                            .when(self.selected, |el| {
                                el.child(
                                    Icon::new(IconName::Check)
                                        .size(px(12.0))
                                        .text_color(palette.accent.hsla()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .font_family(mono)
                            .text_size(text_size)
                            .font_weight(if self.selected {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if self.selected {
                                palette.chrome_text.hsla()
                            } else {
                                palette.chrome_muted.hsla()
                            })
                            .truncate()
                            .child(self.label),
                    ),
            )
            .when_some(self.trailing, |el, right| el.child(right))
    }
}

/// Recessed `0px`-radius progress / meter track (`track_bg`) with optional `1px` frame.
#[derive(IntoElement)]
pub struct GridProgressTrack {
    height: Pixels,
    border_color: Option<Hsla>,
    children: Vec<AnyElement>,
}

impl Default for GridProgressTrack {
    fn default() -> Self {
        Self::new()
    }
}

impl GridProgressTrack {
    /// Creates a new [`GridProgressTrack`] with default `6px` height.
    pub fn new() -> Self {
        Self {
            height: px(6.0),
            border_color: None,
            children: Vec::new(),
        }
    }

    /// Sets the track height.
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Sets an optional `1px` border color around the track.
    pub fn border_color(mut self, color: Hsla) -> Self {
        self.border_color = Some(color);
        self
    }
}

impl ParentElement for GridProgressTrack {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for GridProgressTrack {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .h(self.height)
            .w_full()
            .bg(cx.chrono().palette.track_bg.hsla())
            .when_some(self.border_color, |el, col| el.border_1().border_color(col))
            .flex()
            .overflow_hidden()
            .children(self.children)
    }
}
