//! Overlay, brand chrome, split-sash, and alert-banner `RenderOnce` components.

use std::time::Duration;

use gpui_kit::assets::IconNamed;
use gpui_kit::base::InteractiveElementExt;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::components::resolve_mono_font;
use crate::components::tones::AlertTone;
use crate::theme::ChronoAppExt;
use crate::tokens::{RADIUS_BADGE, RADIUS_MODAL, TYPE_BODY, TYPE_HEADER, TYPE_MICRO};

type CloseHandler = Box<dyn Fn(&mut Window, &mut App)>;
type MouseDownHandler = Box<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;
type DoubleClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Canonical titlebar brand cell with optional SVG mark, wordmark, and optional subtitle,
/// bounded by a `1px` `chrome_border` right rule.
#[derive(IntoElement)]
pub struct BrandPlate {
    wordmark: SharedString,
    logo_svg: Option<SharedString>,
    subtitle: Option<SharedString>,
}

impl BrandPlate {
    /// Creates a new [`BrandPlate`] displaying `wordmark`.
    pub fn new(wordmark: impl Into<SharedString>) -> Self {
        Self {
            wordmark: wordmark.into(),
            logo_svg: None,
            subtitle: None,
        }
    }

    /// Sets an SVG asset path for the brand mark (`22px`).
    pub fn logo_svg(mut self, path: impl Into<SharedString>) -> Self {
        self.logo_svg = Some(path.into());
        self
    }

    /// Sets the brand mark from an [`IconNamed`] value.
    pub fn logo_icon(mut self, icon: impl IconNamed) -> Self {
        self.logo_svg = Some(icon.path());
        self
    }

    /// Sets an optional muted subtitle displayed beside the wordmark.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

impl RenderOnce for BrandPlate {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let text_size = chrono.typography.scale(TYPE_MICRO);
        let mono = resolve_mono_font(cx);

        div()
            .h_full()
            .pr_3()
            .border_r_1()
            .border_color(palette.chrome_border.hsla())
            .flex()
            .items_center()
            .gap_2()
            .when_some(self.logo_svg, |el, path| {
                el.child(
                    svg()
                        .path(path)
                        .size(px(22.0))
                        .text_color(palette.accent.hsla()),
                )
            })
            .child(
                div()
                    .font_family(mono.clone())
                    .text_size(text_size)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.chrome_text.hsla())
                    .child(self.wordmark),
            )
            .when_some(self.subtitle, |el, sub| {
                el.child(
                    div()
                        .font_family(mono)
                        .text_size(text_size)
                        .text_color(palette.chrome_muted.hsla())
                        .child(sub),
                )
            })
    }
}

/// Full-window modal scrim overlay (`modal_scrim`) that occludes pointer/scroll events behind
/// a centered [`ModalPlate`] card with an entrance animation.
#[derive(IntoElement)]
pub struct ModalBackdrop {
    id: ElementId,
    card: AnyElement,
    on_close: Option<CloseHandler>,
}

impl ModalBackdrop {
    /// Creates a new [`ModalBackdrop`] wrapping `card`.
    pub fn new(card: impl IntoElement) -> Self {
        Self {
            id: "modal-backdrop".into(),
            card: card.into_any_element(),
            on_close: None,
        }
    }

    /// Overrides the backdrop element ID (defaults to `"modal-backdrop"`).
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Registers a callback invoked when the user clicks the scrim outside the modal card.
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for ModalBackdrop {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let scrim = cx.chrono().palette.modal_scrim.hsla();
        div()
            .id(self.id)
            .absolute()
            .inset_0()
            .occlude()
            .p_4()
            .bg(scrim)
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll(|_, _, cx| cx.stop_propagation())
            .when_some(self.on_close, |el, handler| {
                el.on_click(move |_, window, cx| handler(window, cx))
            })
            .child(
                div()
                    .id("modal-card-stop")
                    .max_w_full()
                    .max_h_full()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(self.card)
                    .with_animation(
                        "modal-card-enter",
                        Animation::new(Duration::from_millis(140)).with_easing(ease_out_quint()),
                        |el, delta| el.mt(px((1.0 - delta) * 8.0)).opacity(0.65 + 0.35 * delta),
                    ),
            )
    }
}

/// Outer plate shell for in-window modals with padded body content and an optional flush
/// full-bleed footer.
#[derive(IntoElement)]
pub struct ModalPlate {
    width: Pixels,
    border_color: Option<Hsla>,
    body: Vec<AnyElement>,
    footer: Option<AnyElement>,
}

impl ModalPlate {
    /// Creates a new [`ModalPlate`] with the given `width`.
    pub fn new(width: Pixels) -> Self {
        Self {
            width,
            border_color: None,
            body: Vec::new(),
            footer: None,
        }
    }

    /// Overrides the `1px` outer border color (defaults to `chrome_border`).
    pub fn border_color(mut self, color: Hsla) -> Self {
        self.border_color = Some(color);
        self
    }

    /// Sets the flush footer element rendered below the padded body.
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }
}

impl ParentElement for ModalPlate {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.body.extend(elements);
    }
}

impl RenderOnce for ModalPlate {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = &cx.chrono().palette;
        let border_col = self.border_color.unwrap_or_else(|| palette.chrome_border.hsla());
        div()
            .w(self.width)
            .bg(palette.modal_bg.hsla())
            .border_1()
            .border_color(border_col)
            .rounded(RADIUS_MODAL)
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .child(div().w_full().p_5().flex().flex_col().gap_3().children(self.body))
            .when_some(self.footer, |el, footer| {
                el.child(div().bg(palette.chrome_bg.hsla()).child(footer))
            })
    }
}

/// Orientation of a [`SplitSash`] resize handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SashAxis {
    /// Vertical divider between left/right panes (`cursor_col_resize`).
    Vertical,
    /// Horizontal divider between top/bottom panes (`cursor_row_resize`).
    Horizontal,
}

/// Unified vertical or horizontal resize sash with magnetic detent flash and optional
/// floating pixel readout pill while dragging.
#[derive(IntoElement)]
pub struct SplitSash {
    id: ElementId,
    axis: SashAxis,
    active: bool,
    snapped: bool,
    detent_flash: f32,
    readout: Option<SharedString>,
    on_mouse_down: Option<MouseDownHandler>,
    on_double_click: Option<DoubleClickHandler>,
}

impl SplitSash {
    /// Creates a new [`SplitSash`] with `id` and `axis`.
    pub fn new(id: impl Into<ElementId>, axis: SashAxis) -> Self {
        Self {
            id: id.into(),
            axis,
            active: false,
            snapped: false,
            detent_flash: 0.0,
            readout: None,
            on_mouse_down: None,
            on_double_click: None,
        }
    }

    /// Marks the sash as actively dragged.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Marks the sash as snapped to its default detent.
    pub fn snapped(mut self, snapped: bool) -> Self {
        self.snapped = snapped;
        self
    }

    /// Sets the normalized `[0.0..=1.0]` magnetic detent flash intensity.
    pub fn detent_flash(mut self, intensity: f32) -> Self {
        self.detent_flash = intensity;
        self
    }

    /// Sets the floating readout pill label shown while `active` is `true`.
    pub fn readout(mut self, label: impl Into<SharedString>) -> Self {
        self.readout = Some(label.into());
        self
    }

    /// Sets an optional floating readout pill label.
    pub fn maybe_readout(mut self, label: Option<impl Into<SharedString>>) -> Self {
        self.readout = label.map(Into::into);
        self
    }

    /// Registers a left-button mouse-down handler to begin dragging.
    pub fn on_mouse_down(
        mut self,
        handler: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_mouse_down = Some(Box::new(handler));
        self
    }

    /// Registers a double-click handler to snap/reset the pane size.
    pub fn on_double_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_double_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for SplitSash {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let header_size = chrono.typography.scale(TYPE_HEADER);
        let mono = resolve_mono_font(cx);

        let hover_bg = palette.surface_hover.hsla();
        let is_detent = self.detent_flash > 0.05 || (self.active && self.snapped);
        let rule_thickness = if self.active || self.detent_flash > 0.05 {
            2.0
        } else {
            1.0
        };
        let rule_color = if is_detent {
            palette.info.hsla()
        } else if self.active {
            palette.accent.hsla()
        } else {
            palette.chrome_border.hsla()
        };

        let base = div()
            .id(self.id)
            .flex_shrink_0()
            .flex()
            .relative()
            .hover(move |s| s.bg(hover_bg))
            .when_some(self.on_mouse_down, |el, handler| {
                el.on_mouse_down(MouseButton::Left, move |ev, window, cx| {
                    handler(ev, window, cx)
                })
            })
            .when_some(self.on_double_click, |el, handler| {
                el.on_double_click(move |ev, window, cx| handler(ev, window, cx))
            });

        let positioned = match self.axis {
            SashAxis::Vertical => base
                .w(px(7.0))
                .h_full()
                .justify_center()
                .cursor_col_resize()
                .child(div().w(px(rule_thickness)).h_full().bg(rule_color)),
            SashAxis::Horizontal => base
                .h(px(7.0))
                .w_full()
                .items_center()
                .cursor_row_resize()
                .child(div().h(px(rule_thickness)).w_full().bg(rule_color)),
        };

        let pill_border = if self.snapped {
            palette.info.hsla()
        } else {
            palette.accent.hsla()
        };
        let pill_bg = palette.popover_bg.hsla();
        let pill_fg = palette.chrome_text.hsla();

        positioned.when(self.active, |el| {
            el.when_some(self.readout, |el, label| {
                let pill = div()
                    .absolute()
                    .px_2()
                    .py_0p5()
                    .rounded(RADIUS_BADGE)
                    .bg(pill_bg)
                    .border_1()
                    .border_color(pill_border)
                    .shadow_md()
                    .font_family(mono)
                    .text_size(header_size)
                    .text_color(pill_fg)
                    .whitespace_nowrap()
                    .child(label);
                let pill = match self.axis {
                    SashAxis::Vertical => pill.left(px(10.0)).top(px(14.0)),
                    SashAxis::Horizontal => pill.right(px(16.0)).top(px(-24.0)),
                };
                el.child(deferred(pill).with_priority(10))
            })
        })
    }
}

/// Full-width semantic alert strip (`Danger`, `Success`, or `Accent`) with leading icon,
/// truncated monospace message, optional tooltip, and trailing action cells.
#[derive(IntoElement)]
pub struct AlertBanner {
    id: ElementId,
    tone: AlertTone,
    icon_path: Option<SharedString>,
    message: SharedString,
    tooltip: Option<SharedString>,
    actions: Vec<AnyElement>,
}

impl AlertBanner {
    /// Creates a new [`AlertBanner`] displaying `message` (defaults to [`AlertTone::Danger`]).
    pub fn new(message: impl Into<SharedString>) -> Self {
        Self {
            id: "alert-banner".into(),
            tone: AlertTone::Danger,
            icon_path: None,
            message: message.into(),
            tooltip: None,
            actions: Vec::new(),
        }
    }

    /// Overrides the banner element ID.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = id.into();
        self
    }

    /// Sets the semantic alert tone.
    pub fn tone(mut self, tone: AlertTone) -> Self {
        self.tone = tone;
        self
    }

    /// Overrides the leading icon using any [`IconNamed`] value.
    pub fn icon(mut self, icon: impl IconNamed) -> Self {
        self.icon_path = Some(icon.path());
        self
    }

    /// Overrides the leading icon using a raw asset path.
    pub fn icon_path(mut self, path: impl Into<SharedString>) -> Self {
        self.icon_path = Some(path.into());
        self
    }

    /// Sets an optional hover tooltip over the message region.
    pub fn tooltip(mut self, tooltip: impl Into<SharedString>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Appends a trailing action element (for example a [`crate::components::GridCellButton`]).
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl ParentElement for AlertBanner {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.actions.extend(elements);
    }
}

impl RenderOnce for AlertBanner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let chrono = cx.chrono();
        let palette = &chrono.palette;
        let body_size = chrono.typography.scale(TYPE_BODY);
        let mono = resolve_mono_font(cx);

        let bg = self.tone.surface_bg(palette);
        let accent = self.tone.accent_color(palette);
        let icon_path = self
            .icon_path
            .unwrap_or_else(|| self.tone.default_icon().path());

        div()
            .min_h(px(32.0))
            .pl_3()
            .bg(bg)
            .border_1()
            .border_color(accent)
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                div()
                    .id(self.id)
                    .flex_1()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .when_some(self.tooltip, |el, tip| {
                        el.tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    })
                    .child(Icon::default().path(icon_path).size(px(14.0)).text_color(accent))
                    .child(
                        div()
                            .font_family(mono)
                            .text_size(body_size)
                            .line_height(px(18.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(palette.chrome_text.hsla())
                            .truncate()
                            .child(self.message),
                    ),
            )
            .when(!self.actions.is_empty(), |el| {
                el.child(
                    div()
                        .h(px(30.0))
                        .flex()
                        .items_center()
                        .children(self.actions),
                )
            })
    }
}
