use gpui_kit::component::ThemeColor;
use gpui_kit::{Hsla, Rgba};
use settings_model::{DarkTheme, LightTheme};

/// Colors the Foundations page defines on top of gpui-kit's Default theme. Built-in
/// roles (background, muted, success, list-active…) are read from `cx.theme()`.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct AppTheme {
    /// Merged pull requests.
    pub merged: Hsla,
    /// Informational emphasis: review-requested state, progress, the viewer's pending review.
    pub accent: Hsla,
    /// Background of an added line.
    pub diff_add: Hsla,
    /// Background of an added line's gutter and of word-level additions.
    pub diff_add_strong: Hsla,
    /// Background of a removed line.
    pub diff_del: Hsla,
    /// Background of a removed line's gutter and of word-level removals.
    pub diff_del_strong: Hsla,
    /// Background of hunk headers.
    pub diff_hunk: Hsla,
    pub syntax_keyword: Hsla,
    pub syntax_string: Hsla,
    pub syntax_function: Hsla,
    pub syntax_type: Hsla,
    pub syntax_comment: Hsla,
    pub syntax_number: Hsla,
    /// The scrim behind dialogs.
    pub overlay: Hsla,
    /// Dialog and popover shadow.
    pub shadow: Hsla,
    /// The Approve button's border and text once a pull request has approvals.
    pub approve: Hsla,
    pub approve_hover: Hsla,
    pub approve_active: Hsla,
    /// Agents: their avatars, @-mentions and the regions they are writing.
    pub agent: Hsla,
    /// Fill behind agent-authored regions and agent tags.
    pub agent_subtle: Hsla,
    /// Fill of success badges and callouts.
    pub success_subtle: Hsla,
    /// Fill of danger badges and callouts.
    pub danger_subtle: Hsla,
    /// Fill of warning badges and callouts.
    pub warning_subtle: Hsla,
    /// Unread-count badges.
    pub badge: Hsla,
    /// Participant colors for avatars, cursors and selections, assigned in order.
    pub presence: [Hsla; 8],
    /// Colors a person marks items with, Finder-tag style: macOS 27's twelve named system
    /// colors, in [`TAG_NAMES`] order.
    pub tags: [Hsla; 12],
}

/// The names of [`AppTheme::tags`], in order.
pub const TAG_NAMES: [&str; 12] = [
    "Red", "Orange", "Yellow", "Green", "Mint", "Teal", "Cyan", "Blue", "Indigo", "Purple", "Pink", "Brown",
];

impl AppTheme {
    /// Tokens for dark mode in `theme`.
    pub fn dark(theme: DarkTheme) -> Self {
        match theme {
            DarkTheme::Switchfold => Self {
                merged: hex(0xa99bffff),
                accent: hex(0x419cffff),
                diff_add: hex(0x182a1fff),
                diff_add_strong: hex(0x1f3d29ff),
                diff_del: hex(0x301c1dff),
                diff_del_strong: hex(0x4a2427ff),
                diff_hunk: hex(0x1a2638ff),
                syntax_keyword: hex(0xff7ab2ff),
                syntax_string: hex(0xff8f70ff),
                syntax_function: hex(0x7ab6ffff),
                syntax_type: hex(0x6fd9c9ff),
                syntax_comment: hex(0x858791ff),
                syntax_number: hex(0xd9c97cff),
                overlay: hex(0x00000080),
                shadow: hex(0x00000080),
                approve: hex(0x23864aff),
                approve_hover: hex(0x2a9655ff),
                approve_active: hex(0x1e7440ff),
                agent: hex(0xa99bffff),
                agent_subtle: hex(0x2b2643ff),
                success_subtle: hex(0x19301fff),
                danger_subtle: hex(0x3a1f20ff),
                warning_subtle: hex(0x3a2a17ff),
                badge: hex(0xff4245ff),
                presence: presence_dark(),
                tags: tags_dark(),
            },
            DarkTheme::IslandsDark => Self {
                merged: hex(0xc092faff),
                accent: hex(0x5594faff),
                diff_add: hex(0x6bcc621f),
                diff_add_strong: hex(0x6bcc6247),
                diff_del: hex(0xf274811f),
                diff_del_strong: hex(0xf2748147),
                diff_hunk: hex(0x5594fa14),
                syntax_keyword: hex(0xc092faff),
                syntax_string: hex(0x6bcc62ff),
                syntax_function: hex(0x5594faff),
                syntax_type: hex(0xe0ce70ff),
                syntax_comment: hex(0x6f737aff),
                syntax_number: hex(0x47ccbdff),
                overlay: hex(0x00000099),
                shadow: hex(0x000000b3),
                approve: hex(0x6bcc62ff),
                approve_hover: hex(0x85d97dff),
                approve_active: hex(0x56b04eff),
                agent: hex(0xc092faff),
                agent_subtle: hex(0xc092fa26),
                success_subtle: hex(0x6bcc6226),
                danger_subtle: hex(0xf2748126),
                warning_subtle: hex(0xe0ce7026),
                badge: hex(0xe5484dff),
                presence: presence_dark(),
                tags: tags_dark(),
            },
            DarkTheme::OneDark => Self {
                merged: hex(0xc678ddff),
                accent: hex(0x61afefff),
                diff_add: hex(0x98c3791f),
                diff_add_strong: hex(0x98c37947),
                diff_del: hex(0xe06c751f),
                diff_del_strong: hex(0xe06c7547),
                diff_hunk: hex(0x61afef14),
                syntax_keyword: hex(0xc678ddff),
                syntax_string: hex(0x98c379ff),
                syntax_function: hex(0x61afefff),
                syntax_type: hex(0xe5c07bff),
                syntax_comment: hex(0x7f848eff),
                syntax_number: hex(0xd19a66ff),
                overlay: hex(0x00000080),
                shadow: hex(0x00000099),
                approve: hex(0x98c379ff),
                approve_hover: hex(0xb0d494ff),
                approve_active: hex(0x7fa862ff),
                agent: hex(0xc678ddff),
                agent_subtle: hex(0xc678dd26),
                success_subtle: hex(0x98c37926),
                danger_subtle: hex(0xe06c7526),
                warning_subtle: hex(0xe5c07b26),
                badge: hex(0xe5484dff),
                presence: presence_dark(),
                tags: tags_dark(),
            },
            DarkTheme::Espresso => Self {
                merged: hex(0xd197d9ff),
                accent: hex(0x8ab7d9ff),
                diff_add: hex(0xa5c2611f),
                diff_add_strong: hex(0xa5c26147),
                diff_del: hex(0xd252521f),
                diff_del_strong: hex(0xd2525247),
                diff_hunk: hex(0x6c99bb1a),
                syntax_keyword: hex(0xd197d9ff),
                syntax_string: hex(0xa5c261ff),
                syntax_function: hex(0x8ab7d9ff),
                syntax_type: hex(0xffc66dff),
                syntax_comment: hex(0x888888ff),
                syntax_number: hex(0xbed6ffff),
                overlay: hex(0x00000080),
                shadow: hex(0x00000099),
                approve: hex(0xa5c261ff),
                approve_hover: hex(0xc2e075ff),
                approve_active: hex(0x8aa84dff),
                agent: hex(0xd197d9ff),
                agent_subtle: hex(0xd197d926),
                success_subtle: hex(0xa5c26126),
                danger_subtle: hex(0xd2525226),
                warning_subtle: hex(0xffc66d26),
                badge: hex(0xe5484dff),
                presence: presence_dark(),
                tags: tags_dark(),
            },
            DarkTheme::GitHub => Self {
                merged: hex(0xc084fcff),
                accent: hex(0x4493f8ff),
                diff_add: hex(0x4ade801a),
                diff_add_strong: hex(0x4ade8040),
                diff_del: hex(0xf871711a),
                diff_del_strong: hex(0xf8717140),
                diff_hunk: hex(0x60a5fa14),
                syntax_keyword: hex(0xc084fcff),
                syntax_string: hex(0x86efacff),
                syntax_function: hex(0x93c5fdff),
                syntax_type: hex(0xfde047ff),
                syntax_comment: hex(0x737373ff),
                syntax_number: hex(0x67e8f9ff),
                overlay: hex(0x00000080),
                shadow: hex(0x00000099),
                approve: hex(0x3fb950ff),
                approve_hover: hex(0x56d364ff),
                approve_active: hex(0x2ea043ff),
                agent: hex(0xc084fcff),
                agent_subtle: hex(0xc084fc26),
                success_subtle: hex(0x3fb95026),
                danger_subtle: hex(0xf8514926),
                warning_subtle: hex(0xd2992226),
                badge: hex(0xe5484dff),
                presence: presence_dark(),
                tags: tags_dark(),
            },
        }
    }

    /// Tokens for light mode in `theme`.
    pub fn light(theme: LightTheme) -> Self {
        match theme {
            LightTheme::Switchfold => Self {
                merged: hex(0x6a4fd6ff),
                accent: hex(0x0068daff),
                diff_add: hex(0xeaf6eeff),
                diff_add_strong: hex(0xccebd6ff),
                diff_del: hex(0xfdedeeff),
                diff_del_strong: hex(0xf7d3d5ff),
                diff_hunk: hex(0xebf3ffff),
                syntax_keyword: hex(0x9b2393ff),
                syntax_string: hex(0xb4441eff),
                syntax_function: hex(0x2f5fc7ff),
                syntax_type: hex(0x0d6e86ff),
                syntax_comment: hex(0x75757eff),
                syntax_number: hex(0x8a5a00ff),
                overlay: hex(0x00000029),
                shadow: hex(0x00000024),
                approve: hex(0x1a7f39ff),
                approve_hover: hex(0x176f32ff),
                approve_active: hex(0x13602bff),
                agent: hex(0x6a4fd6ff),
                agent_subtle: hex(0xf0edfcff),
                success_subtle: hex(0xe7f4eaff),
                danger_subtle: hex(0xfcebecff),
                warning_subtle: hex(0xfcf0e1ff),
                badge: hex(0xff383cff),
                presence: presence_light(),
                tags: tags_light(),
            },
            LightTheme::GitHub => Self {
                merged: hex(0x9333eaff),
                accent: hex(0x0969daff),
                diff_add: hex(0x22c55e1f),
                diff_add_strong: hex(0x22c55e47),
                diff_del: hex(0xef44441f),
                diff_del_strong: hex(0xef444447),
                diff_hunk: hex(0x3b82f614),
                syntax_keyword: hex(0x9333eaff),
                syntax_string: hex(0x15803dff),
                syntax_function: hex(0x2563ebff),
                syntax_type: hex(0xa16207ff),
                syntax_comment: hex(0xa3a3a3ff),
                syntax_number: hex(0x0e7490ff),
                overlay: hex(0x00000033),
                shadow: hex(0x0000001f),
                approve: hex(0x1f883dff),
                approve_hover: hex(0x1c8139ff),
                approve_active: hex(0x197935ff),
                agent: hex(0x9333eaff),
                agent_subtle: hex(0x9333ea1f),
                success_subtle: hex(0x1f883d1f),
                danger_subtle: hex(0xd1242f1f),
                warning_subtle: hex(0x9a67001f),
                badge: hex(0xe5484dff),
                presence: presence_light(),
                tags: tags_light(),
            },
            LightTheme::IslandsLight => Self {
                merged: hex(0x871094ff),
                accent: hex(0x3574f0ff),
                diff_add: hex(0x3696501f),
                diff_add_strong: hex(0x36965040),
                diff_del: hex(0xe55b5b1f),
                diff_del_strong: hex(0xe55b5b40),
                diff_hunk: hex(0x3574f014),
                syntax_keyword: hex(0x0033b3ff),
                syntax_string: hex(0x067d17ff),
                syntax_function: hex(0x00627aff),
                syntax_type: hex(0x008080ff),
                syntax_comment: hex(0x8c8c8cff),
                syntax_number: hex(0x1750ebff),
                overlay: hex(0x00000033),
                shadow: hex(0x0000001f),
                approve: hex(0x369650ff),
                approve_hover: hex(0x2f8746ff),
                approve_active: hex(0x29783dff),
                agent: hex(0x871094ff),
                agent_subtle: hex(0x8710941f),
                success_subtle: hex(0x3696501f),
                danger_subtle: hex(0xe55b5b1f),
                warning_subtle: hex(0xc27d041f),
                badge: hex(0xe5484dff),
                presence: presence_light(),
                tags: tags_light(),
            },
            LightTheme::OneLight => Self {
                merged: hex(0xa626a4ff),
                accent: hex(0x4078f2ff),
                diff_add: hex(0x50a14f1f),
                diff_add_strong: hex(0x50a14f40),
                diff_del: hex(0xe456491f),
                diff_del_strong: hex(0xe4564940),
                diff_hunk: hex(0x4078f214),
                syntax_keyword: hex(0xa626a4ff),
                syntax_string: hex(0x50a14fff),
                syntax_function: hex(0x4078f2ff),
                syntax_type: hex(0xc18401ff),
                syntax_comment: hex(0xa0a1a7ff),
                syntax_number: hex(0x986801ff),
                overlay: hex(0x00000033),
                shadow: hex(0x0000001f),
                approve: hex(0x50a14fff),
                approve_hover: hex(0x468f45ff),
                approve_active: hex(0x3d7d3cff),
                agent: hex(0xa626a4ff),
                agent_subtle: hex(0xa626a41f),
                success_subtle: hex(0x50a14f1f),
                danger_subtle: hex(0xe456491f),
                warning_subtle: hex(0xc184011f),
                badge: hex(0xe5484dff),
                presence: presence_light(),
                tags: tags_light(),
            },
            LightTheme::SolarizedLight => Self {
                merged: hex(0x6c71c4ff),
                accent: hex(0x268bd2ff),
                diff_add: hex(0x8599001f),
                diff_add_strong: hex(0x85990040),
                diff_del: hex(0xdc322f1f),
                diff_del_strong: hex(0xdc322f40),
                diff_hunk: hex(0x268bd214),
                syntax_keyword: hex(0x859900ff),
                syntax_string: hex(0x2aa198ff),
                syntax_function: hex(0x268bd2ff),
                syntax_type: hex(0xb58900ff),
                syntax_comment: hex(0x93a1a1ff),
                syntax_number: hex(0xd33682ff),
                overlay: hex(0x00000033),
                shadow: hex(0x0000001f),
                approve: hex(0x859900ff),
                approve_hover: hex(0x748500ff),
                approve_active: hex(0x637200ff),
                agent: hex(0x6c71c4ff),
                agent_subtle: hex(0x6c71c41f),
                success_subtle: hex(0x8599001f),
                danger_subtle: hex(0xdc322f1f),
                warning_subtle: hex(0xb589001f),
                badge: hex(0xe5484dff),
                presence: presence_light(),
                tags: tags_light(),
            },
        }
    }
}

/// The palette one appearance uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Palette {
    Light(LightTheme),
    Dark(DarkTheme),
}

/// Switchfold's chrome on top of gpui-kit's neutral Default theme: the sidebar and bars
/// a step off the background, and one accent for primary actions, focus, links,
/// selection and progress.
pub(crate) fn apply_chrome(colors: &mut ThemeColor, palette: Palette) {
    let c = match palette {
        Palette::Light(theme) => Chrome::light(theme),
        Palette::Dark(theme) => Chrome::dark(theme),
    };
    if let Some([success, danger, warning, info]) = c.status {
        colors.success = success;
        colors.danger = danger;
        colors.warning = warning;
        colors.info = info;
    }
    colors.background = c.background;
    colors.foreground = c.foreground;
    colors.muted = c.subtle;
    colors.muted_foreground = c.muted_foreground;
    colors.border = c.border;
    colors.input = c.border;
    colors.accent = c.hover;
    colors.accent_foreground = c.foreground;
    colors.secondary = c.subtle;
    colors.secondary_hover = c.hover;
    colors.secondary_active = c.pressed;
    colors.secondary_foreground = c.foreground;
    colors.button = c.raised;
    colors.button_hover = c.hover;
    colors.button_active = c.pressed;
    colors.button_foreground = c.foreground;
    colors.popover = c.raised;
    colors.popover_foreground = c.foreground;
    colors.sidebar = c.chrome;
    colors.sidebar_border = c.border;
    colors.sidebar_foreground = c.foreground;
    colors.sidebar_accent = c.hover;
    colors.sidebar_accent_foreground = c.foreground;
    colors.sidebar_primary = c.primary;
    colors.sidebar_primary_foreground = c.on_primary;
    colors.title_bar = c.chrome;
    colors.title_bar_border = c.border;
    colors.status_bar = c.chrome;
    colors.status_bar_border = c.border;
    colors.tab_bar = c.chrome;
    colors.primary = c.primary;
    colors.primary_hover = c.primary_hover;
    colors.primary_active = c.primary_active;
    colors.primary_foreground = c.on_primary;
    colors.button_primary = c.primary;
    colors.button_primary_hover = c.primary_hover;
    colors.button_primary_active = c.primary_active;
    colors.button_primary_foreground = c.on_primary;
    colors.ring = c.ring.unwrap_or(c.primary);
    colors.caret = c.primary;
    colors.link = c.link;
    colors.link_hover = c.primary_hover;
    colors.link_active = c.primary_active;
    colors.selection = c.selection;
    colors.list_active = c.selected;
    colors.list_active_border = c.primary;
    colors.list_hover = c.subtle;
    colors.table_active = c.selected;
    colors.table_active_border = c.primary;
    colors.table_hover = c.subtle;
    colors.progress_bar = c.primary;
    // The off track; the on track takes the primary color on its own.
    colors.switch = c.switch.unwrap_or(c.pressed);
    colors.slider_bar = c.primary;
    colors.slider_thumb = c.on_primary;
    colors.skeleton = c.hover;
    colors.scrollbar_thumb = c.scrollbar;
    colors.window_border = c.border;
}

/// The chrome palette for one appearance.
struct Chrome {
    background: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    /// The sidebar, title bar and status bar.
    chrome: Hsla,
    /// Inline fills: code, hovered rows, muted surfaces.
    subtle: Hsla,
    /// Buttons and popovers that sit above the background.
    raised: Hsla,
    hover: Hsla,
    pressed: Hsla,
    border: Hsla,
    primary: Hsla,
    primary_hover: Hsla,
    primary_active: Hsla,
    on_primary: Hsla,
    link: Hsla,
    selection: Hsla,
    selected: Hsla,
    scrollbar: Hsla,
    /// Success, danger, warning and info, when the palette has its own.
    status: Option<[Hsla; 4]>,
    /// The focus ring, when it isn't the primary color.
    ring: Option<Hsla>,
    /// A switch's off track, when it isn't the pressed fill. The thumb is always the
    /// background, so a dark palette needs a lighter track for the thumb to show.
    switch: Option<Hsla>,
}

impl Chrome {
    fn light(theme: LightTheme) -> Self {
        match theme {
            // Built on macOS 27's own values: white content, the under-page grey for the
            // sidebar and bars, and separators at the system's 10% black. The primary stays
            // neutral, as in the product design; system blue is kept for links, focus and
            // selection. Text runs darker than AppKit's 85% label, and muted text darker
            // than its 50% secondary label, to keep both above 4.5:1 on the sidebar.
            LightTheme::Switchfold => Self {
                background: hex(0xffffffff),
                foreground: hex(0x1d1d1fff),
                muted_foreground: hex(0x6b6b72ff),
                chrome: hex(0xf7f7f8ff),
                subtle: hex(0xf2f2f4ff),
                raised: hex(0xffffffff),
                hover: hex(0xebebeeff),
                pressed: hex(0xe2e2e6ff),
                border: hex(0xe5e5e8ff),
                primary: hex(0x1d1d1fff),
                primary_hover: hex(0x3a3a3fff),
                primary_active: hex(0x000000ff),
                on_primary: hex(0xffffffff),
                link: hex(0x0068daff),
                selection: hex(0x0088ff33),
                selected: hex(0xe1e1e6ff),
                scrollbar: hex(0x6b6b7266),
                status: Some([
                    hex(0x1a7f39ff),
                    hex(0xd92b2fff),
                    hex(0xb85b00ff),
                    hex(0x0068daff),
                ]),
                ring: Some(hex(0x0088ffff)),
                switch: Some(hex(0xd4d4d9ff)),
            },
            LightTheme::GitHub => Self {
                background: hex(0xffffffff),
                foreground: hex(0x1f2328ff),
                muted_foreground: hex(0x59636eff),
                chrome: hex(0xf6f8faff),
                subtle: hex(0xf6f8faff),
                raised: hex(0xffffffff),
                hover: hex(0xeef1f4ff),
                pressed: hex(0xe1e6ebff),
                border: hex(0xd1d9e0ff),
                primary: hex(0x0969daff),
                primary_hover: hex(0x0860caff),
                primary_active: hex(0x0757baff),
                on_primary: hex(0xffffffff),
                link: hex(0x0969daff),
                selection: hex(0x0969da33),
                selected: hex(0xddf4ffff),
                scrollbar: hex(0x8c959f66),
                status: None,
                ring: None,
                switch: None,
            },
            // Content stays white; the window frame, sidebar and bars take the panel
            // grey, as JetBrains' islands do.
            LightTheme::IslandsLight => Self {
                background: hex(0xffffffff),
                foreground: hex(0x000000ff),
                muted_foreground: hex(0x6c707eff),
                chrome: hex(0xf7f8faff),
                subtle: hex(0xf7f8faff),
                raised: hex(0xffffffff),
                hover: hex(0xdfe1e5ff),
                pressed: hex(0xd3d5dbff),
                border: hex(0xebecf0ff),
                primary: hex(0x3574f0ff),
                primary_hover: hex(0x3369d6ff),
                primary_active: hex(0x315fbdff),
                on_primary: hex(0xffffffff),
                link: hex(0x2e55a3ff),
                selection: hex(0xa6d2ff99),
                selected: hex(0xd5e1ffff),
                scrollbar: hex(0x8c8c8c66),
                status: Some([
                    hex(0x369650ff),
                    hex(0xe55b5bff),
                    hex(0xc27d04ff),
                    hex(0x3574f0ff),
                ]),
                ring: None,
                switch: None,
            },
            LightTheme::OneLight => Self {
                background: hex(0xfafafaff),
                foreground: hex(0x383a42ff),
                muted_foreground: hex(0x696c77ff),
                chrome: hex(0xeaeaebff),
                subtle: hex(0xf0f0f1ff),
                raised: hex(0xffffffff),
                hover: hex(0xe5e5e6ff),
                pressed: hex(0xdbdbdcff),
                border: hex(0xdbdbdcff),
                primary: hex(0x4078f2ff),
                primary_hover: hex(0x3a6cdaff),
                primary_active: hex(0x3360c2ff),
                on_primary: hex(0xffffffff),
                link: hex(0x4078f2ff),
                selection: hex(0x4078f233),
                selected: hex(0xdfe6f8ff),
                scrollbar: hex(0x696c7766),
                status: Some([
                    hex(0x50a14fff),
                    hex(0xe45649ff),
                    hex(0xc18401ff),
                    hex(0x0184bcff),
                ]),
                ring: None,
                switch: None,
            },
            LightTheme::SolarizedLight => Self {
                background: hex(0xfdf6e3ff),
                foreground: hex(0x586e75ff),
                muted_foreground: hex(0x839496ff),
                chrome: hex(0xeee8d5ff),
                subtle: hex(0xf5efdcff),
                raised: hex(0xfdf6e3ff),
                hover: hex(0xe8e1cbff),
                pressed: hex(0xddd6c1ff),
                border: hex(0xddd6c1ff),
                primary: hex(0x268bd2ff),
                primary_hover: hex(0x227dbdff),
                primary_active: hex(0x1e6fa8ff),
                on_primary: hex(0xfdf6e3ff),
                link: hex(0x268bd2ff),
                selection: hex(0x268bd233),
                selected: hex(0xe4e4d4ff),
                scrollbar: hex(0x93a1a166),
                status: Some([
                    hex(0x859900ff),
                    hex(0xdc322fff),
                    hex(0xb58900ff),
                    hex(0x268bd2ff),
                ]),
                ring: None,
                switch: None,
            },
        }
    }

    fn dark(theme: DarkTheme) -> Self {
        match theme {
            // macOS 27's dark window grey for content, with the sidebar and bars a step
            // lighter, as Tahoe's floating sidebar sits above the content. The primary
            // stays neutral, as in the product design; system blue is kept for links, focus
            // and selection, and danger is softened from system red so it reads as text.
            DarkTheme::Switchfold => Self {
                background: hex(0x1e1e1fff),
                foreground: hex(0xe6e6e8ff),
                muted_foreground: hex(0x9d9da4ff),
                chrome: hex(0x252527ff),
                subtle: hex(0x2b2b2eff),
                raised: hex(0x2c2c2fff),
                hover: hex(0x323236ff),
                pressed: hex(0x3a3a3eff),
                border: hex(0x363639ff),
                primary: hex(0xe6e6e8ff),
                primary_hover: hex(0xffffffff),
                primary_active: hex(0xbdbdc3ff),
                on_primary: hex(0x1e1e1fff),
                link: hex(0x419cffff),
                selection: hex(0x0091ff4d),
                selected: hex(0x3d3d42ff),
                scrollbar: hex(0x8e8e9566),
                status: Some([
                    hex(0x30d158ff),
                    hex(0xff6961ff),
                    hex(0xff9f3dff),
                    hex(0x419cffff),
                ]),
                ring: Some(hex(0x0091ffff)),
                switch: Some(hex(0x5a5a60ff)),
            },
            // The window frame, sidebar and bars take the Ghostty background; content
            // sits a step lighter, as JetBrains' islands do.
            DarkTheme::IslandsDark => Self {
                background: hex(0x1e1f22ff),
                foreground: hex(0xced0d6ff),
                muted_foreground: hex(0x868a91ff),
                chrome: hex(0x191a1cff),
                subtle: hex(0x26282bff),
                raised: hex(0x2b2d30ff),
                hover: hex(0x313337ff),
                pressed: hex(0x393b40ff),
                border: hex(0x2f3135ff),
                primary: hex(0x3574f0ff),
                primary_hover: hex(0x4682faff),
                primary_active: hex(0x2e64d0ff),
                on_primary: hex(0xffffffff),
                link: hex(0x5594faff),
                selection: hex(0x214283ff),
                selected: hex(0x2e436eff),
                scrollbar: hex(0x4e515780),
                status: Some([
                    hex(0x6bcc62ff),
                    hex(0xf27481ff),
                    hex(0xe0ce70ff),
                    hex(0x5594faff),
                ]),
                ring: None,
                switch: None,
            },
            // One Dark's hues on darker grounds, as One Dark Pro Darker has them.
            DarkTheme::OneDark => Self {
                background: hex(0x1f2329ff),
                foreground: hex(0xd7dae0ff),
                muted_foreground: hex(0x7f848eff),
                chrome: hex(0x181b20ff),
                subtle: hex(0x252930ff),
                raised: hex(0x272b32ff),
                hover: hex(0x2d323aff),
                pressed: hex(0x363c46ff),
                border: hex(0x30353dff),
                primary: hex(0x4d78ccff),
                primary_hover: hex(0x5a86deff),
                primary_active: hex(0x3e65b0ff),
                on_primary: hex(0xffffffff),
                link: hex(0x61afefff),
                selection: hex(0x4d78cc55),
                selected: hex(0x4d78cc33),
                scrollbar: hex(0x4e566680),
                status: Some([
                    hex(0x98c379ff),
                    hex(0xe06c75ff),
                    hex(0xe5c07bff),
                    hex(0x61afefff),
                ]),
                ring: None,
                switch: None,
            },
            DarkTheme::Espresso => Self {
                background: hex(0x323232ff),
                foreground: hex(0xeeeeecff),
                muted_foreground: hex(0x9a9a96ff),
                chrome: hex(0x2a2a2aff),
                subtle: hex(0x3a3a3aff),
                raised: hex(0x3c3c3cff),
                hover: hex(0x454545ff),
                pressed: hex(0x505050ff),
                border: hex(0x4a4a4aff),
                primary: hex(0x6c99bbff),
                primary_hover: hex(0x8ab7d9ff),
                primary_active: hex(0x5a87a8ff),
                on_primary: hex(0xffffffff),
                link: hex(0x8ab7d9ff),
                selection: hex(0x6c99bb55),
                selected: hex(0x6c99bb33),
                scrollbar: hex(0x66666680),
                status: Some([
                    hex(0xa5c261ff),
                    hex(0xd25252ff),
                    hex(0xffc66dff),
                    hex(0x6c99bbff),
                ]),
                ring: None,
                switch: None,
            },
            DarkTheme::GitHub => Self {
                background: hex(0x0d1117ff),
                foreground: hex(0xe6edf3ff),
                muted_foreground: hex(0x9198a1ff),
                chrome: hex(0x151b23ff),
                subtle: hex(0x151b23ff),
                raised: hex(0x212830ff),
                hover: hex(0x262c36ff),
                pressed: hex(0x2f3742ff),
                border: hex(0x3d444dff),
                primary: hex(0x1f6febff),
                primary_hover: hex(0x388bfdff),
                primary_active: hex(0x1158c7ff),
                on_primary: hex(0xffffffff),
                link: hex(0x4493f8ff),
                selection: hex(0x388bfd40),
                selected: hex(0x388bfd26),
                scrollbar: hex(0x6e768166),
                status: None,
                ring: None,
                switch: None,
            },
        }
    }
}

/// macOS 27's system colors in dark mode, as AppKit reports them.
fn tags_dark() -> [Hsla; 12] {
    [
        hex(0xff4245ff),
        hex(0xff9230ff),
        hex(0xffd600ff),
        hex(0x30d158ff),
        hex(0x00dac3ff),
        hex(0x00d2e0ff),
        hex(0x3cd3feff),
        hex(0x0091ffff),
        hex(0x6d7cffff),
        hex(0xdb34f2ff),
        hex(0xff375fff),
        hex(0xb78a66ff),
    ]
}

/// macOS 27's system colors in light mode, as AppKit reports them.
fn tags_light() -> [Hsla; 12] {
    [
        hex(0xff383cff),
        hex(0xff8d28ff),
        hex(0xffcc00ff),
        hex(0x34c759ff),
        hex(0x00c8b3ff),
        hex(0x00c3d0ff),
        hex(0x00c0e8ff),
        hex(0x0088ffff),
        hex(0x6155f5ff),
        hex(0xcb30e0ff),
        hex(0xff2d55ff),
        hex(0xac7f5eff),
    ]
}

/// Participant colors on dark grounds.
fn presence_dark() -> [Hsla; 8] {
    [
        hex(0xf97316ff),
        hex(0xa78bfaff),
        hex(0x22d3eeff),
        hex(0xf472b6ff),
        hex(0x84cc16ff),
        hex(0xfbbf24ff),
        hex(0x60a5faff),
        hex(0xf87171ff),
    ]
}

/// The same hues as [`presence_dark`], a step darker so they read on white.
fn presence_light() -> [Hsla; 8] {
    [
        hex(0xea580cff),
        hex(0x7c3aedff),
        hex(0x0891b2ff),
        hex(0xdb2777ff),
        hex(0x65a30dff),
        hex(0xd97706ff),
        hex(0x2563ebff),
        hex(0xdc2626ff),
    ]
}

fn hex(rgba: u32) -> Hsla {
    Rgba {
        r: ((rgba >> 24) & 0xff) as f32 / 255.,
        g: ((rgba >> 16) & 0xff) as f32 / 255.,
        b: ((rgba >> 8) & 0xff) as f32 / 255.,
        a: (rgba & 0xff) as f32 / 255.,
    }
    .into()
}
