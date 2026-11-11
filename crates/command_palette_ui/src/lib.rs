//! The command palette behind ⌘K and the title bar's search field: pull requests, files
//! of the open review, and commands, in one filterable list.

mod fuzzy;

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::component::{
    Icon, WindowExt as _,
    command::{Command, CommandGroup, CommandItem, CommandState},
};
use gpui_kit::{
    Action, App, AppContext as _, InteractiveElement as _, ParentElement as _, SharedString,
    Styled as _, TestSupportExt as _, Window, div,
};

pub use fuzzy::rank;
pub use gpui_kit::assets::IconName;
pub use gpui_kit::component::IndexPath;

/// What a palette entry does when confirmed.
pub enum PaletteTarget {
    /// Dispatch a command.
    Action(Box<dyn Action>),
    /// Call back with the entry's key, such as a pull request or file path.
    Key(SharedString),
}

/// One row of the palette.
pub struct PaletteEntry {
    label: SharedString,
    keywords: Vec<SharedString>,
    icon: IconName,
    target: PaletteTarget,
}

impl PaletteEntry {
    /// A row that dispatches `action`.
    pub fn action(label: impl Into<SharedString>, icon: IconName, action: Box<dyn Action>) -> Self {
        Self {
            label: label.into(),
            keywords: Vec::new(),
            icon,
            target: PaletteTarget::Action(action),
        }
    }

    /// A row that reports `key` to the palette's owner.
    pub fn key(
        label: impl Into<SharedString>,
        icon: IconName,
        key: impl Into<SharedString>,
    ) -> Self {
        Self {
            label: label.into(),
            keywords: Vec::new(),
            icon,
            target: PaletteTarget::Key(key.into()),
        }
    }

    /// Extra words the row matches, such as an author or a number.
    pub fn keywords(mut self, keywords: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }
}

/// A titled group of rows.
pub struct PaletteGroup {
    title: SharedString,
    entries: Vec<PaletteEntry>,
}

impl PaletteGroup {
    /// A group titled `title`.
    pub fn new(title: impl Into<SharedString>, entries: Vec<PaletteEntry>) -> Self {
        Self {
            title: title.into(),
            entries,
        }
    }
}

/// A rendered row: label, keywords, icon, and the action it dispatches, if any.
type Row = (
    SharedString,
    Vec<SharedString>,
    IconName,
    Option<Box<dyn Action>>,
);

type KeyHandler = Rc<dyn Fn(SharedString, &mut Window, &mut App)>;

/// Opens the palette over the window. Rows with a [`PaletteTarget::Key`] call
/// `on_key`; the palette closes after any confirmation.
pub fn open(
    groups: Vec<PaletteGroup>,
    on_key: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let groups: Vec<PaletteGroup> = groups
        .into_iter()
        .filter(|g| !g.entries.is_empty())
        .collect();
    let keys: Rc<Vec<Vec<Option<SharedString>>>> = Rc::new(
        groups
            .iter()
            .map(|group| {
                group
                    .entries
                    .iter()
                    .map(|entry| match &entry.target {
                        PaletteTarget::Key(key) => Some(key.clone()),
                        PaletteTarget::Action(_) => None,
                    })
                    .collect()
            })
            .collect(),
    );
    let rows: Rc<Vec<(SharedString, Vec<Row>)>> = Rc::new(
        groups
            .into_iter()
            .map(|group| {
                let entries = group
                    .entries
                    .into_iter()
                    .map(|entry| {
                        let action = match entry.target {
                            PaletteTarget::Action(action) => Some(action),
                            PaletteTarget::Key(_) => None,
                        };
                        (entry.label, entry.keywords, entry.icon, action)
                    })
                    .collect();
                (group.title, entries)
            })
            .collect(),
    );
    let state = cx.new(|cx| CommandState::new(window, cx));
    state.update(cx, |state, cx| state.focus(window, cx));
    let on_key: KeyHandler = Rc::new(on_key);
    let width = window.rem_size() * 40.;
    let top = window.viewport_size().height / 6.;
    window.open_dialog(cx, move |dialog, _, _| {
        let keys = keys.clone();
        let on_key = on_key.clone();
        let groups = rows.iter().map(|(title, entries)| {
            CommandGroup::new()
                .label(title.clone())
                .items(entries.iter().map(|(label, keywords, icon, action)| {
                    let item = CommandItem::new()
                        .label(label.clone())
                        .icon(Icon::new(*icon))
                        .keywords(keywords.clone());
                    match action {
                        Some(action) => item.action(action.boxed_clone()),
                        None => item,
                    }
                }))
        });
        let command = groups.fold(
            Command::new(&state)
                .placeholder("Search pull requests, files, commands…")
                .max_h(gpui_kit::rems(26.)),
            |command, group| command.group(group),
        );
        dialog
            .w(width)
            .margin_top(top)
            .close_button(false)
            .p_0()
            .child(
                command
                    .on_confirm(move |path: IndexPath, window, cx| {
                        let key = keys
                            .get(path.section)
                            .and_then(|section| section.get(path.row))
                            .cloned()
                            .flatten();
                        window.close_dialog(cx);
                        if let Some(key) = key {
                            on_key(key, window, cx);
                        }
                    })
                    .on_cancel(|window, cx| window.close_dialog(cx)),
            )
    });
}

/// Opens Go to file over the window: `paths` ranked by [`rank`] as the query changes.
/// Confirming one closes it and calls `on_pick` with the path.
pub fn open_files(
    paths: Vec<SharedString>,
    on_pick: impl Fn(SharedString, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let paths = Rc::new(paths);
    let shown: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new((0..paths.len()).collect()));
    let state = cx.new(|cx| CommandState::new(window, cx));
    let query = state.clone();
    let on_pick: KeyHandler = Rc::new(on_pick);
    let width = window.rem_size() * 40.;
    let top = window.viewport_size().height / 6.;
    window.open_dialog(cx, move |dialog, _, _| {
        let items: Vec<CommandItem> = shown
            .borrow()
            .iter()
            .map(|ix| {
                CommandItem::new()
                    .label(paths[*ix].clone())
                    .icon(Icon::new(IconName::FileCode))
            })
            .collect();
        let ranked = shown.clone();
        let query_paths = paths.clone();
        let picked = shown.clone();
        let pick_paths = paths.clone();
        let on_pick = on_pick.clone();
        let command = Command::new(&state)
            .placeholder("Go to file…")
            .max_h(gpui_kit::rems(26.))
            .filterable(false)
            .group(CommandGroup::new().label("Files").items(items))
            .on_query(move |query, window, _| {
                *ranked.borrow_mut() = rank(query, &query_paths);
                window.refresh();
            })
            .on_confirm(move |path: IndexPath, window, cx| {
                let path = picked
                    .borrow()
                    .get(path.row)
                    .map(|ix| pick_paths[*ix].clone());
                window.close_dialog(cx);
                if let Some(path) = path {
                    on_pick(path, window, cx);
                }
            })
            .on_cancel(|window, cx| window.close_dialog(cx));
        dialog
            .w(width)
            .margin_top(top)
            .close_button(false)
            .p_0()
            .child(div().id("go-to-file").test_support().child(command))
    });
    // The dialog takes focus as it opens; the query field needs it.
    query.update(cx, |state, cx| state.focus(window, cx));
}
