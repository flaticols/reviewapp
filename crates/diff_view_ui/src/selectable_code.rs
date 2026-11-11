use gpui_kit::base::{
    TextSelection, TextSelectionHandle, TextSelectionRegistration, TextSelectionRun,
};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    App, BorderStyle, Bounds, Corners, Edges, Element, ElementId, GlobalElementId, HighlightStyle,
    Hitbox, HitboxBehavior, InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels, Point,
    SharedString, StyledText, TextLayout, Window, transparent_black,
};

/// A line of highlighted code that takes part in the window's text selection, so a drag
/// across diff lines selects them like any other text and ⌘C copies it.
///
/// gpui-kit's `SelectableText` lays out plain text only; this is the same participant
/// around a `StyledText` that keeps the syntax colors.
pub(crate) struct SelectableCode {
    id: ElementId,
    text: SharedString,
    styled_text: StyledText,
    document_order: u64,
}

impl SelectableCode {
    /// `text` painted with `highlights`; `id` must be stable across frames.
    pub(crate) fn new(
        id: impl Into<ElementId>,
        text: impl Into<SharedString>,
        highlights: Vec<(std::ops::Range<usize>, HighlightStyle)>,
    ) -> Self {
        let text = text.into();
        Self {
            id: id.into(),
            styled_text: StyledText::new(text.clone()).with_highlights(highlights),
            text,
            document_order: 0,
        }
    }

    /// Places the line in reading order among the other selectable text of the window.
    pub(crate) fn document_order(mut self, order: u64) -> Self {
        self.document_order = order;
        self
    }
}

impl IntoElement for SelectableCode {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableCode {
    type RequestLayoutState = TextSelectionHandle;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let handle = window.with_element_state(
            global_id.expect("SelectableCode has an id"),
            |retained: Option<TextSelectionHandle>, _| {
                let handle =
                    retained.unwrap_or_else(|| TextSelectionHandle::new(self.text.clone(), cx));
                (handle.clone(), handle)
            },
        );
        let (layout_id, ()) = self
            .styled_text
            .request_layout(global_id, inspector_id, window, cx);
        (layout_id, handle)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled_text
            .prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
            .with_document_order(self.document_order)
            .with_text_bounds(vec![bounds])
            .with_rendered_element(handle, window, cx);
        handle.register(registration, window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        handle: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled_text.layout().clone();
        // Resolving the selected text walks every participant in the window, so a frame
        // with nothing selected, which is nearly every scroll frame, skips it.
        let selected_before = TextSelection::has_selection(window, cx)
            .then(|| TextSelection::selected_text(window, cx));
        let projection = handle.update_runs(
            &[
                TextSelectionRun::new(self.text.clone(), layout.clone(), bounds)
                    .with_document_order(self.document_order),
            ],
            cx,
        );
        // Reporting this line's runs can change what the selection covers elsewhere,
        // which only the next frame paints.
        if let Some(before) = selected_before
            && before != TextSelection::selected_text(window, cx)
        {
            window.refresh();
        }
        let color = cx.theme().selection;
        for range in projection.ranges().iter().flatten() {
            paint_selection(&layout, range.clone(), color, window);
        }
        self.styled_text.paint(
            global_id,
            inspector_id,
            bounds,
            &mut (),
            &mut (),
            window,
            cx,
        );
    }
}

/// Paints the selection over `range`. Diff lines never wrap, so it is one quad.
fn paint_selection(
    layout: &TextLayout,
    range: std::ops::Range<usize>,
    color: gpui_kit::Hsla,
    window: &mut Window,
) {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return;
    };
    window.paint_quad(PaintQuad {
        bounds: Bounds::from_corners(start, Point::new(end.x, end.y + layout.line_height())),
        background: color.into(),
        corner_radii: Corners::default(),
        border_widths: Edges::default(),
        border_color: transparent_black(),
        border_style: BorderStyle::default(),
    });
}
