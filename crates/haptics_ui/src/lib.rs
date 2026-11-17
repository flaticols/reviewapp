//! Haptic feedback on a Force Touch trackpad. The app installs the system performer at
//! startup; until one is installed a haptic does nothing, which is what tests and
//! offscreen rendering want.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{App, Global};

/// A haptic the trackpad can play, named as AppKit names its feedback patterns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    /// A plain tick, for feedback that fits neither of the others.
    Generic,
    /// Something the pointer drags snapped into place.
    Alignment,
    /// A value moved to its next discrete level.
    LevelChange,
}

struct Performer(Rc<dyn Fn(Haptic)>);

impl Global for Performer {}

/// Sends haptics to the trackpad from here on. Call once at startup, on the main thread.
/// Without a Force Touch trackpad, or with haptic feedback turned off in System Settings,
/// macOS drops them. Does nothing on other platforms.
pub fn install_system(cx: &mut App) {
    cx.set_global(Performer(Rc::new(system)));
}

/// Collects haptics instead of playing them, and returns the list they are added to.
pub fn record(cx: &mut App) -> Rc<RefCell<Vec<Haptic>>> {
    let played = Rc::new(RefCell::new(Vec::new()));
    let sink = played.clone();
    cx.set_global(Performer(Rc::new(move |haptic| {
        sink.borrow_mut().push(haptic)
    })));
    played
}

/// Plays `haptic` through the installed performer.
pub fn perform(haptic: Haptic, cx: &App) {
    if let Some(performer) = cx.try_global::<Performer>() {
        (performer.0)(haptic)
    }
}

#[cfg(target_os = "macos")]
fn system(haptic: Haptic) {
    use objc2_app_kit::{
        NSHapticFeedbackManager, NSHapticFeedbackPattern, NSHapticFeedbackPerformanceTime,
        NSHapticFeedbackPerformer as _,
    };
    let pattern = match haptic {
        Haptic::Generic => NSHapticFeedbackPattern::Generic,
        Haptic::Alignment => NSHapticFeedbackPattern::Alignment,
        Haptic::LevelChange => NSHapticFeedbackPattern::LevelChange,
    };
    NSHapticFeedbackManager::defaultPerformer()
        .performFeedbackPattern_performanceTime(pattern, NSHapticFeedbackPerformanceTime::Now);
}

#[cfg(not(target_os = "macos"))]
fn system(_: Haptic) {}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn nothing_plays_until_a_performer_is_installed(cx: &mut TestAppContext) {
        cx.update(|cx| {
            perform(Haptic::LevelChange, cx);
            let played = record(cx);
            perform(Haptic::LevelChange, cx);
            perform(Haptic::Alignment, cx);
            assert_eq!(
                played.borrow().as_slice(),
                [Haptic::LevelChange, Haptic::Alignment]
            );
        });
    }
}
