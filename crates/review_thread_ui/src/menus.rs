use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::{App, Entity};
use pr_actions_ui::FileTarget;

use crate::ReviewThreadView;

impl ReviewThreadView {
    /// The menu for `path`, shared by its diff header and its row in the file tree:
    /// the GitHub actions, then Open in Zed, Mark viewed and Collapse.
    pub(crate) fn file_menu(
        view: &Entity<Self>,
        path: &str,
        menu: PopupMenu,
        cx: &App,
    ) -> PopupMenu {
        let this = view.read(cx);
        let head = this
            .detail
            .as_ref()
            .and_then(|detail| detail.commits.last().map(|commit| commit.sha.clone()));
        let viewed = this
            .detail
            .as_ref()
            .is_some_and(|detail| detail.is_viewed(path));
        let collapsed = this.collapsed.contains(path);
        let target = FileTarget::new(this.key.repo().clone(), this.key.number(), head, path);
        let (zed_view, zed_path) = (view.clone(), path.to_string());
        let (viewed_view, viewed_path) = (view.clone(), path.to_string());
        let (collapse_view, collapse_path) = (view.clone(), path.to_string());
        pr_actions_ui::file_menu(menu, &target)
            .separator()
            .item(
                PopupMenuItem::new("Open in Zed")
                    .icon(Icon::new(IconName::ExternalLink))
                    .on_click(move |_, window, cx| {
                        zed_view.update(cx, |this, cx| {
                            this.open_in_zed(Some(zed_path.clone()), None, window, cx)
                        })
                    }),
            )
            .item(
                PopupMenuItem::new(if viewed {
                    "Mark as not viewed"
                } else {
                    "Mark as viewed"
                })
                .icon(Icon::new(IconName::SquareCheck))
                .on_click(move |_, _, cx| {
                    viewed_view.update(cx, |this, cx| this.set_viewed(&viewed_path, !viewed, cx))
                }),
            )
            .item(
                PopupMenuItem::new(if collapsed { "Expand" } else { "Collapse" })
                    .icon(Icon::new(if collapsed {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    }))
                    .on_click(move |_, _, cx| {
                        collapse_view.update(cx, |this, cx| {
                            if !this.collapsed.remove(&collapse_path) {
                                this.collapsed.insert(collapse_path.clone());
                            }
                            cx.notify();
                        })
                    }),
            )
    }
}
