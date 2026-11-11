use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, rems,
};
use theme_ui::TypographyExt as _;

/// A settings page: title, one-sentence description, then sections, in a readable
/// column that scrolls on its own.
#[derive(IntoElement)]
pub struct SettingsPage {
    id: SharedString,
    title: SharedString,
    description: SharedString,
    children: Vec<AnyElement>,
}

impl SettingsPage {
    /// A page titled `title`.
    pub fn new(
        id: impl Into<SharedString>,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: description.into(),
            children: Vec::new(),
        }
    }

    /// Adds a section or control below the description.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    /// Adds several children.
    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl RenderOnce for SettingsPage {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        div().id(self.id).size_full().overflow_y_scroll().child(
            v_flex()
                .w_full()
                // The design's 680 px column plus the page's leading and trailing padding.
                .max_w(rems(48.5))
                .pt_12()
                .pb_12()
                .pl_16()
                .pr_8()
                .gap_6()
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_title().child(self.title))
                        .child(
                            div()
                                .text_body()
                                .text_color(cx.theme().muted_foreground)
                                .child(self.description),
                        ),
                )
                .children(self.children),
        )
    }
}

/// A titled group of settings rows inside a page.
#[derive(IntoElement)]
pub struct SettingsSection {
    title: Option<SharedString>,
    rows: Vec<AnyElement>,
    after: Vec<AnyElement>,
}

impl SettingsSection {
    /// A section without a heading.
    pub fn new() -> Self {
        Self {
            title: None,
            rows: Vec::new(),
            after: Vec::new(),
        }
    }

    /// A section headed `title`.
    pub fn titled(title: impl Into<SharedString>) -> Self {
        Self {
            title: Some(title.into()),
            rows: Vec::new(),
            after: Vec::new(),
        }
    }

    /// Adds a row inside the card.
    pub fn row(mut self, row: impl IntoElement) -> Self {
        self.rows.push(row.into_any_element());
        self
    }

    /// Adds several rows.
    pub fn rows(mut self, rows: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.rows
            .extend(rows.into_iter().map(IntoElement::into_any_element));
        self
    }

    /// Adds an element under the card, such as an Add button.
    pub fn after(mut self, element: impl IntoElement) -> Self {
        self.after.push(element.into_any_element());
        self
    }
}

impl Default for SettingsSection {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderOnce for SettingsSection {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let count = self.rows.len();
        v_flex()
            .gap_3()
            .when_some(self.title, |this, title| {
                this.child(
                    div()
                        .text_body()
                        .font_weight(FontWeight::MEDIUM)
                        .child(title),
                )
            })
            .when(count > 0, |this| {
                this.child(
                    v_flex()
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded(cx.theme().radius_lg)
                        .overflow_hidden()
                        .children(self.rows.into_iter().enumerate().map(|(ix, row)| {
                            div()
                                .when(ix + 1 < count, |this| {
                                    this.border_b_1().border_color(cx.theme().border)
                                })
                                .child(row)
                        })),
                )
            })
            .when(!self.after.is_empty(), |this| {
                this.child(h_flex().gap_3().children(self.after))
            })
    }
}

/// One setting: an optional leading visual, a title with a description, and the
/// control at the trailing edge.
#[derive(IntoElement)]
pub struct SettingsRow {
    leading: Option<AnyElement>,
    title: AnyElement,
    description: Option<AnyElement>,
    trailing: Vec<AnyElement>,
}

impl SettingsRow {
    /// A row titled `title`.
    pub fn new(title: impl Into<SharedString>) -> Self {
        let title: SharedString = title.into();
        Self {
            leading: None,
            title: div().child(title).into_any_element(),
            description: None,
            trailing: Vec::new(),
        }
    }

    /// A row whose title is an arbitrary element.
    pub fn with_title_element(title: impl IntoElement) -> Self {
        Self {
            leading: None,
            title: title.into_any_element(),
            description: None,
            trailing: Vec::new(),
        }
    }

    /// Adds the explanation under the title.
    pub fn description(mut self, description: impl IntoElement) -> Self {
        self.description = Some(description.into_any_element());
        self
    }

    /// Adds a visual before the title, such as an avatar or icon.
    pub fn leading(mut self, leading: impl IntoElement) -> Self {
        self.leading = Some(leading.into_any_element());
        self
    }

    /// Adds a control at the trailing edge; several keep their order.
    pub fn trailing(mut self, trailing: impl IntoElement) -> Self {
        self.trailing.push(trailing.into_any_element());
        self
    }
}

impl RenderOnce for SettingsRow {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        h_flex()
            .min_h(rems(3.5))
            .px_3()
            .py_2()
            .gap_3()
            .when_some(self.leading, |this, leading| this.child(leading))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(div().text_body().child(self.title))
                    .when_some(self.description, |this, description| {
                        this.child(
                            div()
                                .text_ui()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        )
                    }),
            )
            .children(self.trailing)
    }
}
