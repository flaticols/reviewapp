//! Switchfold's theme layer on top of gpui-kit's Default theme: the application tokens
//! the Foundations page adds (merged, accent, diff and syntax colors), the typography
//! roles the screens use, and switching between light, dark and system appearance.
//!
//! Hex values live only in this crate. Everything else reads `cx.app_theme()` or
//! `cx.theme()`.

mod tokens;
mod typography;

use std::sync::OnceLock;

use gpui_kit::component::{ActiveTheme as _, Theme, ThemeMode};
use gpui_kit::{App, Global, Pixels, Point, SharedString, Window, WindowAppearance, point, px};
use settings_model::{Appearance, Preferences};

pub use settings_model::{DarkTheme, LightTheme};
pub use tokens::{AppTheme, TAG_NAMES};
pub use typography::{TypographyExt, code_line_height};

/// The title bar's height, the compact height of macOS unified toolbars; overlays that
/// sit below it use the same value. Fixed rather than in rem: the traffic lights don't
/// scale with the interface size, so a bar that did would stop lining up with them.
pub const TITLE_BAR_HEIGHT: Pixels = px(38.);

/// Where the traffic lights go, in points from the window's top-left corner. A traffic
/// light is 15 pt tall as measured in a real window, so 11.5 centers it in the
/// [`TITLE_BAR_HEIGHT`] bar.
pub const TRAFFIC_LIGHT_POSITION: Point<Pixels> = point(px(12.), px(11.5));

impl Global for AppTheme {}

/// Access to the application tokens for the active appearance.
pub trait ActiveAppTheme {
    /// The application tokens matching the current light or dark mode.
    fn app_theme(&self) -> &AppTheme;
}

impl ActiveAppTheme for App {
    fn app_theme(&self) -> &AppTheme {
        self.global::<AppTheme>()
    }
}

/// Installs the theme for the preferred appearance, the interface zoom given by the
/// font size, and the code font. `System` follows the window's appearance, or the
/// app's when no window exists yet.
pub fn apply(preferences: &Preferences, window: Option<&mut Window>, cx: &mut App) {
    let system = window
        .as_ref()
        .map(|window| window.appearance())
        .unwrap_or_else(|| cx.window_appearance());
    let mode = resolve_mode(preferences.appearance, system);
    Theme::change(mode, None, cx);
    let palette = if mode.is_dark() {
        tokens::Palette::Dark(preferences.dark_theme)
    } else {
        tokens::Palette::Light(preferences.light_theme)
    };
    let tokens = match palette {
        tokens::Palette::Dark(theme) => AppTheme::dark(theme),
        tokens::Palette::Light(theme) => AppTheme::light(theme),
    };
    let font_size = preferences.font_size;
    let default_family = cx.theme().mono_font_family.clone();
    // GPUI panics on the first line laid out in a family it can't find, so a saved
    // font that has since been uninstalled falls back to the default.
    let code_family = preferences
        .code_font_family
        .clone()
        .filter(|family| installed_font_families(cx).contains(family));
    // `Theme::change` reloads the registered theme, so the overrides go in a second
    // update afterwards, as `Theme::update` documents.
    Theme::update(cx, |theme| {
        tokens::apply_chrome(&mut theme.colors, palette);
        // Success buttons are the green Approve: solid, like GitHub's.
        theme.button_success = tokens.approve;
        theme.button_success_hover = tokens.approve_hover;
        theme.button_success_active = tokens.approve_active;
        theme.button_success_foreground = gpui_kit::white();
        theme.overlay = tokens.overlay;
        theme.font_size = px(font_size);
        // Sheets open below the title bar.
        theme.sheet.margin_top = TITLE_BAR_HEIGHT;
        if let Some(family) = code_family {
            theme.mono_font_family = family.into();
        }
    });
    cx.set_global(tokens);
    cx.set_global(CodeFont {
        default_family,
        size: preferences.code_font_size.map(px),
    });
}

/// The theme color for a repository color.
pub fn repository_color(color: settings_model::RepositoryColor, cx: &App) -> gpui_kit::Hsla {
    use settings_model::RepositoryColor;
    let theme = cx.theme();
    match color {
        RepositoryColor::Red => theme.red,
        RepositoryColor::Yellow => theme.yellow,
        RepositoryColor::Green => theme.green,
        RepositoryColor::Cyan => theme.cyan,
        RepositoryColor::Blue => theme.blue,
        RepositoryColor::Purple => theme.magenta,
    }
}

/// The code font beyond the theme's family: the theme's own monospace family, for
/// naming the default in settings, and the size the reviewer picked, if any.
#[derive(Clone, Debug, Default)]
pub struct CodeFont {
    pub default_family: SharedString,
    pub size: Option<Pixels>,
}

impl Global for CodeFont {}

/// Every font family installed on the machine, listed once per process: enumerating
/// them takes about 100 ms on macOS.
pub fn installed_font_families(cx: &App) -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        let mut names = cx.text_system().all_font_names();
        names.sort();
        names.dedup();
        names
    })
}

/// The theme's monospace family before the reviewer's choice replaced it.
pub fn default_code_font_family(cx: &App) -> SharedString {
    cx.try_global::<CodeFont>()
        .map(|font| font.default_family.clone())
        .unwrap_or_default()
}

/// The light or dark mode `appearance` resolves to under `system`.
pub fn resolve_mode(appearance: Appearance, system: WindowAppearance) -> ThemeMode {
    match appearance {
        Appearance::Light => ThemeMode::Light,
        Appearance::Dark => ThemeMode::Dark,
        Appearance::System => match system {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ThemeMode::Dark,
            WindowAppearance::Light | WindowAppearance::VibrantLight => ThemeMode::Light,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_appearance_ignores_the_system() {
        assert_eq!(
            resolve_mode(Appearance::Light, WindowAppearance::Dark),
            ThemeMode::Light
        );
        assert_eq!(
            resolve_mode(Appearance::System, WindowAppearance::VibrantDark),
            ThemeMode::Dark
        );
    }
}
