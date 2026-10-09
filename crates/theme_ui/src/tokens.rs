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
}

impl AppTheme {
    /// Tokens for dark mode in `theme`.
    pub fn dark(theme: DarkTheme) -> Self {
        match theme {
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
            },
        }
    }

    /// Tokens for light mode in `theme`.
    pub fn light(theme: LightTheme) -> Self {
        match theme {
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
    colors.ring = c.primary;
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
    colors.switch = c.primary;
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
}

impl Chrome {
    fn light(theme: LightTheme) -> Self {
        match theme {
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
            },
        }
    }

    fn dark(theme: DarkTheme) -> Self {
        match theme {
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
            },
        }
    }
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
