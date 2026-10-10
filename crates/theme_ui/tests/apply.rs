//! Applying preferences reaches the components: they read gpui-kit's semantic tokens,
//! not the raw colors the theme sets.

use gpui_kit::TestAppContext;
use gpui_kit::component::ActiveTheme as _;
use settings_model::{Appearance, Preferences};

#[gpui_kit::test]
fn primary_reaches_the_tokens_in_both_modes(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        for appearance in [Appearance::Light, Appearance::Dark, Appearance::Light] {
            let mut preferences = Preferences::default();
            preferences.appearance = appearance;
            theme_ui::apply(&preferences, None, cx);
            let theme = cx.theme();
            assert_eq!(
                theme.tokens.primary.color, theme.primary,
                "{appearance:?}: switches and primary buttons read the token"
            );
            assert_ne!(
                theme.tokens.switch.color, theme.primary,
                "{appearance:?}: off track"
            );
        }
    });
}
