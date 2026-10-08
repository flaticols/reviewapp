use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{AbsoluteLength, App, FontWeight, Pixels, Styled, px, rems};

/// The type roles the design uses, on the rem scale so they follow interface zoom.
/// Sizes are in design pixels at the default 16 px base.
pub trait TypographyExt: Styled + Sized {
    /// 20 px semibold: the object a screen is about, such as a pull request title.
    fn text_title(self) -> Self {
        self.text_size(rems(1.25)).font_weight(FontWeight::SEMIBOLD)
    }

    /// 16 px semibold: dialog and settings page headings.
    fn text_heading(self) -> Self {
        self.text_size(rems(1.)).font_weight(FontWeight::SEMIBOLD)
    }

    /// 14 px: prose such as descriptions and comments.
    fn text_body(self) -> Self {
        self.text_size(rems(0.875))
    }

    /// 13 px: window chrome, rows and labels.
    fn text_ui(self) -> Self {
        self.text_size(rems(0.8125))
    }

    /// 12 px: metadata under a primary label.
    fn text_meta(self) -> Self {
        self.text_size(rems(0.75))
    }

    /// Monospace for code, paths and hashes: 12 px at the default interface size, or
    /// the code size the reviewer picked.
    fn text_code(self, cx: &App) -> Self {
        let size: AbsoluteLength = match code_size(cx) {
            Some(size) => size.into(),
            None => rems(0.75).into(),
        };
        self.font_family(cx.theme().mono_font_family.clone())
            .text_size(size)
    }

    /// Monospace one step below [`TypographyExt::text_code`]: diff gutters and log
    /// line numbers.
    fn text_gutter(self, cx: &App) -> Self {
        let size: AbsoluteLength = match code_size(cx) {
            Some(size) => (size - px(1.)).into(),
            None => rems(0.6875).into(),
        };
        self.font_family(cx.theme().mono_font_family.clone())
            .text_size(size)
    }
}

impl<T: Styled> TypographyExt for T {}

fn code_size(cx: &App) -> Option<Pixels> {
    cx.try_global::<crate::CodeFont>()
        .and_then(|font| font.size)
}

/// The height of one line of code in a diff or log: 20 px at the default interface
/// size, or 1.6 times the picked code size.
pub fn code_line_height(cx: &App) -> AbsoluteLength {
    match code_size(cx) {
        Some(size) => (size * 1.6).round().into(),
        None => rems(1.25).into(),
    }
}
