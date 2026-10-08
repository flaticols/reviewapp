//! Settings › Appearance: light, dark or system theme, the dark palette, interface
//! size, and the font and size used for code.

use app_state_ui::StoresExt as _;
use gpui_kit::component::{
    IndexPath, Sizable as _,
    radio::RadioGroup,
    select::{Select, SelectEvent, SelectState},
};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Window, div, rems,
};
use settings_model::{
    Appearance, CODE_FONT_SIZES, DEFAULT_FONT_SIZE, DarkTheme, FONT_SIZES, Preferences,
};
use widgets_ui::{SettingsPage, SettingsRow, SettingsSection};

/// The Appearance page.
pub struct AppearancePage {
    size: Entity<SelectState<Vec<SharedString>>>,
    code_font: Entity<SelectState<Vec<SharedString>>>,
    code_size: Entity<SelectState<Vec<SharedString>>>,
    _subscriptions: Vec<Subscription>,
}

/// Name fragments of common monospace families, listed first in the code font menu.
const MONOSPACE_HINTS: [&str; 14] = [
    "Mono",
    "Code",
    "Menlo",
    "Monaco",
    "Courier",
    "Consol",
    "Iosevka",
    "Hack",
    "Fira",
    "Inconsolata",
    "Cascadia",
    "Berkeley",
    "Hasklig",
    "Terminal",
];

/// The code font menu: the default first, then installed families that look
/// monospace, then every other family.
fn code_font_labels(default_family: &str, installed: &[String]) -> Vec<SharedString> {
    let visible = installed.iter().filter(|name| !name.starts_with('.'));
    let (mono, other): (Vec<&String>, Vec<&String>) =
        visible.partition(|name| MONOSPACE_HINTS.iter().any(|hint| name.contains(hint)));
    std::iter::once(SharedString::from(format!("Default ({default_family})")))
        .chain(
            mono.into_iter()
                .chain(other)
                .map(|name| name.clone().into()),
        )
        .collect()
}

/// `Match interface` for the proportional size, the bare size otherwise.
fn code_size_label(size: Option<f32>) -> SharedString {
    match size {
        None => "Match interface".into(),
        Some(size) => format!("{size} pt").into(),
    }
}

/// Saves `edit` to the preferences and reapplies the theme with the result.
fn update_look(window: &mut Window, cx: &mut gpui_kit::App, edit: impl FnOnce(&mut Preferences)) {
    let settings = cx.stores().settings().clone();
    settings.update(cx, |settings, cx| settings.update_preferences(cx, edit));
    let preferences = settings.read(cx).preferences().clone();
    theme_ui::apply(&preferences, Some(window), cx);
}

/// `Default (16 pt)` for the design size, the bare size otherwise.
fn size_label(size: f32) -> SharedString {
    if size == DEFAULT_FONT_SIZE {
        format!("Default ({size} pt)").into()
    } else if size < DEFAULT_FONT_SIZE {
        format!("Smaller ({size} pt)").into()
    } else {
        format!("Larger ({size} pt)").into()
    }
}

impl AppearancePage {
    /// Creates the page.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = cx.stores().settings().clone();
        let current = settings.read(cx).preferences().font_size;
        let labels: Vec<SharedString> = FONT_SIZES.iter().map(|size| size_label(*size)).collect();
        let selected = FONT_SIZES.iter().position(|size| *size == current);
        let size = cx.new(|cx| SelectState::new(labels, selected.map(IndexPath::new), window, cx));
        let preferences = settings.read(cx).preferences().clone();
        let fonts = code_font_labels(
            &theme_ui::default_code_font_family(cx),
            theme_ui::installed_font_families(cx),
        );
        let font_ix = match &preferences.code_font_family {
            None => Some(0),
            Some(family) => fonts.iter().position(|label| label.as_ref() == family),
        };
        let code_font = cx.new(|cx| {
            SelectState::new(fonts, font_ix.map(IndexPath::new), window, cx).searchable(true)
        });
        let sizes: Vec<Option<f32>> = std::iter::once(None)
            .chain(CODE_FONT_SIZES.iter().copied().map(Some))
            .collect();
        let size_ix = sizes
            .iter()
            .position(|size| *size == preferences.code_font_size);
        let code_size = cx.new(|cx| {
            SelectState::new(
                sizes.iter().map(|size| code_size_label(*size)).collect(),
                size_ix.map(IndexPath::new),
                window,
                cx,
            )
        });
        let subscriptions = vec![
            cx.observe(&settings, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &size,
                window,
                |_, _, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                    let SelectEvent::Confirm(Some(label)) = event else {
                        return;
                    };
                    if let Some(size) = FONT_SIZES
                        .iter()
                        .copied()
                        .find(|size| &size_label(*size) == label)
                    {
                        update_look(window, cx, |p| p.font_size = size);
                    }
                },
            ),
            cx.subscribe_in(
                &code_font,
                window,
                |_, _, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                    let SelectEvent::Confirm(Some(label)) = event else {
                        return;
                    };
                    let family = (!label.starts_with("Default (")).then(|| label.to_string());
                    update_look(window, cx, |p| p.code_font_family = family);
                },
            ),
            cx.subscribe_in(
                &code_size,
                window,
                move |_, _, event: &SelectEvent<Vec<SharedString>>, window, cx| {
                    let SelectEvent::Confirm(Some(label)) = event else {
                        return;
                    };
                    if let Some(size) = sizes.iter().find(|size| &code_size_label(**size) == label)
                    {
                        let size = *size;
                        update_look(window, cx, |p| p.code_font_size = size);
                    }
                },
            ),
        ];
        Self {
            size,
            code_font,
            code_size,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for AppearancePage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preferences = cx.stores().settings().read(cx).preferences().clone();
        let selected = Appearance::ALL
            .iter()
            .position(|a| *a == preferences.appearance);
        SettingsPage::new(
            "appearance-page",
            "Appearance",
            "How Switchfold looks. Changes apply right away.",
        )
        .child(
            SettingsSection::new()
                .row(
                    SettingsRow::new("Theme")
                        .description("System follows macOS light and dark mode.")
                        .trailing(
                            RadioGroup::horizontal("theme")
                                .children(Appearance::ALL.map(|appearance| appearance.label()))
                                .selected_index(selected)
                                .on_change(|ix, window, cx| {
                                    let appearance = Appearance::ALL[*ix];
                                    update_look(window, cx, |p| p.appearance = appearance);
                                }),
                        ),
                )
                .row(
                    SettingsRow::new("Dark theme")
                        .description("The palette dark mode uses, in the window and in code.")
                        .trailing(
                            RadioGroup::horizontal("dark-theme")
                                .children(DarkTheme::ALL.map(|theme| theme.label()))
                                .selected_index(
                                    DarkTheme::ALL
                                        .iter()
                                        .position(|theme| *theme == preferences.dark_theme),
                                )
                                .on_change(|ix, window, cx| {
                                    let theme = DarkTheme::ALL[*ix];
                                    update_look(window, cx, |p| p.dark_theme = theme);
                                }),
                        ),
                )
                .row(
                    SettingsRow::new("Interface size")
                        .description(
                            "Text, controls and spacing scale together. ⌘+ and ⌘− change it too.",
                        )
                        .trailing(div().w(rems(11.25)).child(Select::new(&self.size).small())),
                ),
        )
        .child(
            SettingsSection::titled("Code")
                .row(
                    SettingsRow::new("Font")
                        .description("Diffs, file paths, hashes and logs.")
                        .trailing(
                            div()
                                .w(rems(13.75))
                                .child(Select::new(&self.code_font).small()),
                        ),
                )
                .row(
                    SettingsRow::new("Size")
                        .description("Match interface keeps code in step with ⌘+ and ⌘−.")
                        .trailing(
                            div()
                                .w(rems(11.25))
                                .child(Select::new(&self.code_size).small()),
                        ),
                ),
        )
    }
}
