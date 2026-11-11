//! The pull request list's controls, placed in the title bar: the scope's name and
//! open count on the left; filter text, the Filter and Sort menus and Refresh on the
//! right. Filters beyond the state stay visible as removable chips above the list.

use app_state_ui::StoresExt as _;
use app_state_ui::actions::Refresh;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::Input,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
};
use gpui_kit::{
    App, Context, Entity, FontWeight, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled as _, Window, div, prelude::FluentBuilder as _, rems,
};
use pr_model::{CheckStatus, PullRequest, RepoSlug, ReviewDecision};
use theme_ui::{ActiveAppTheme as _, TypographyExt as _};
use widgets_ui::{Counter, EditMenu};

use crate::{CI_FILTERS, ListFilters, PullRequestList, SortOrder, StateTab, filters, tab_count};

/// The scope's name and how many of its pull requests are open, for the title bar.
#[derive(IntoElement)]
pub struct ListTitle {
    list: Entity<PullRequestList>,
}

impl ListTitle {
    /// The title of `list`'s current scope.
    pub fn new(list: Entity<PullRequestList>) -> Self {
        Self { list }
    }
}

impl RenderOnce for ListTitle {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let list = self.list.read(cx);
        let store = cx.stores().pull_requests().read(cx);
        let count = tab_count(
            store.pull_requests(),
            &list.scope,
            store.saved_filters(),
            StateTab::Open,
        );
        h_flex()
            .min_w_0()
            .gap_1p5()
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_ui()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(list.title(cx)),
            )
            .child(Counter::new(count))
    }
}

/// Filter text, Filter, Sort and Refresh, for the title bar.
#[derive(IntoElement)]
pub struct ListActions {
    list: Entity<PullRequestList>,
}

impl ListActions {
    /// The controls for `list`.
    pub fn new(list: Entity<PullRequestList>) -> Self {
        Self { list }
    }
}

/// The values the Filter menu offers, from the pull requests in scope.
struct Choices {
    counts: Vec<(StateTab, usize)>,
    repositories: Vec<RepoSlug>,
    authors: Vec<String>,
    labels: Vec<String>,
}

fn choices(list: &PullRequestList, cx: &App) -> Choices {
    let store = cx.stores().pull_requests().read(cx);
    let prs = store.pull_requests();
    let saved = store.saved_filters();
    let in_scope: Vec<&PullRequest> = prs
        .iter()
        .filter(|pr| filters::in_scope(pr, &list.scope, saved))
        .collect();
    let mut repositories: Vec<RepoSlug> = in_scope.iter().map(|pr| pr.key.repo().clone()).collect();
    repositories.sort();
    repositories.dedup();
    let mut authors: Vec<String> = in_scope.iter().map(|pr| pr.author.clone()).collect();
    authors.sort();
    authors.dedup();
    let mut labels: Vec<String> = in_scope.iter().flat_map(|pr| pr.labels.clone()).collect();
    labels.sort();
    labels.dedup();
    Choices {
        counts: StateTab::ALL
            .iter()
            .map(|tab| (*tab, tab_count(prs, &list.scope, saved, *tab)))
            .collect(),
        repositories,
        authors,
        labels,
    }
}

/// How many filters beyond the state and text are set.
fn active_count(filters: &ListFilters) -> usize {
    [
        filters.repository.is_some(),
        filters.author.is_some(),
        filters.label.is_some(),
        filters.ci.is_some(),
        filters.review.is_some(),
    ]
    .into_iter()
    .filter(|on| *on)
    .count()
}

fn update(list: &Entity<PullRequestList>, cx: &mut App, edit: impl FnOnce(&mut ListFilters)) {
    list.update(cx, |list, cx| {
        edit(&mut list.filters);
        list.selected = None;
        cx.notify();
    });
}

/// A submenu that picks one value of a field, or any.
#[allow(clippy::too_many_arguments)]
fn field_submenu<T: Clone + PartialEq + 'static>(
    menu: PopupMenu,
    name: &'static str,
    current: Option<T>,
    options: Vec<(SharedString, T)>,
    list: Entity<PullRequestList>,
    apply: fn(&mut ListFilters, Option<T>),
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let title: SharedString = match options
        .iter()
        .find(|(_, value)| Some(value) == current.as_ref())
    {
        Some((label, _)) => format!("{name}: {label}").into(),
        None => name.into(),
    };
    menu.submenu(title, window, cx, move |menu, window, _| {
        let any_list = list.clone();
        let menu = menu.scrollable(true).max_h(window.rem_size() * 24.).item(
            PopupMenuItem::new(format!("Any {}", name.to_lowercase()))
                .checked(current.is_none())
                .on_click(move |_, _, cx| update(&any_list, cx, |f| apply(f, None))),
        );
        let menu = if options.is_empty() {
            menu
        } else {
            menu.separator()
        };
        options.iter().fold(menu, |menu, (label, value)| {
            let list = list.clone();
            let value = value.clone();
            let checked = current.as_ref() == Some(&value);
            menu.item(PopupMenuItem::new(label.clone()).checked(checked).on_click(
                move |_, _, cx| {
                    let value = value.clone();
                    update(&list, cx, |f| apply(f, Some(value)))
                },
            ))
        })
    })
}

impl RenderOnce for ListActions {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let list = self.list.read(cx);
        let filters = list.filters.clone();
        let filter_input = list.filter_input.clone();
        let choices = choices(list, cx);
        let active = active_count(&filters);
        let accent = cx.app_theme().accent;

        let state_label = filters.tab.label();
        let filter_label: SharedString = match active {
            0 => state_label.into(),
            1 => format!("{state_label} · 1 filter").into(),
            n => format!("{state_label} · {n} filters").into(),
        };
        let menu_list = self.list.clone();
        let filter = Button::new("list-filter")
            .ghost()
            .small()
            .dropdown_caret(true)
            .icon(Icon::new(IconName::ListFilter).when(active > 0, |icon| icon.text_color(accent)))
            .label(filter_label)
            .tooltip("Which pull requests to show")
            .dropdown_menu_with_anchor(
                gpui_kit::Anchor::TopRight,
                move |menu: PopupMenu, window, cx| {
                    let list = menu_list.clone();
                    let menu = menu.min_w(window.rem_size() * 14.).label("Show");
                    let menu = choices.counts.iter().fold(menu, |menu, (tab, count)| {
                        let list = list.clone();
                        let tab = *tab;
                        let label = if *count > 0 && matches!(tab, StateTab::Open | StateTab::Draft)
                        {
                            format!("{} ({count})", tab.label())
                        } else {
                            tab.label().to_string()
                        };
                        menu.item(
                            PopupMenuItem::new(label)
                                .checked(filters.tab == tab)
                                .on_click(move |_, _, cx| update(&list, cx, |f| f.tab = tab)),
                        )
                    });
                    let menu = menu.separator().label("Filter by");
                    let menu = field_submenu(
                        menu,
                        "Repository",
                        filters.repository.clone(),
                        choices
                            .repositories
                            .iter()
                            .map(|repo| (SharedString::from(repo.name().clone()), repo.clone()))
                            .collect(),
                        list.clone(),
                        |f, v| f.repository = v,
                        window,
                        cx,
                    );
                    let menu = field_submenu(
                        menu,
                        "Author",
                        filters.author.clone(),
                        choices
                            .authors
                            .iter()
                            .map(|a| (a.clone().into(), a.clone()))
                            .collect(),
                        list.clone(),
                        |f, v| f.author = v,
                        window,
                        cx,
                    );
                    let menu = field_submenu(
                        menu,
                        "Label",
                        filters.label.clone(),
                        choices
                            .labels
                            .iter()
                            .map(|l| (l.clone().into(), l.clone()))
                            .collect(),
                        list.clone(),
                        |f, v| f.label = v,
                        window,
                        cx,
                    );
                    let menu = field_submenu(
                        menu,
                        "CI",
                        filters.ci,
                        CI_FILTERS
                            .iter()
                            .map(|ci| (SharedString::from(ci.label()), *ci))
                            .collect::<Vec<(SharedString, CheckStatus)>>(),
                        list.clone(),
                        |f, v| f.ci = v,
                        window,
                        cx,
                    );
                    let menu = field_submenu(
                        menu,
                        "Review",
                        filters.review,
                        ReviewDecision::ALL
                            .iter()
                            .map(|r| (SharedString::from(r.label()), *r))
                            .collect::<Vec<(SharedString, ReviewDecision)>>(),
                        list.clone(),
                        |f, v| f.review = v,
                        window,
                        cx,
                    );
                    let clear_list = list.clone();
                    menu.separator().item(
                        PopupMenuItem::new("Clear filters")
                            .disabled(active == 0)
                            .on_click(move |_, _, cx| {
                                update(&clear_list, cx, |f| f.clear_fields())
                            }),
                    )
                },
            );

        let sort_list = self.list.clone();
        let current_sort = filters.sort;
        let sort = Button::new("list-sort")
            .ghost()
            .small()
            .dropdown_caret(true)
            .label(current_sort.label())
            .tooltip("Order of the list")
            .dropdown_menu_with_anchor(gpui_kit::Anchor::TopRight, move |menu: PopupMenu, _, _| {
                SortOrder::ALL.iter().fold(menu, |menu, order| {
                    let list = sort_list.clone();
                    let order = *order;
                    menu.item(
                        PopupMenuItem::new(order.label())
                            .checked(order == current_sort)
                            .on_click(move |_, _, cx| update(&list, cx, |f| f.sort = order)),
                    )
                })
            });

        h_flex()
            .gap_1()
            .child(
                EditMenu::new(
                    "filter-edit-menu",
                    &filter_input,
                    Input::new(&filter_input)
                        .small()
                        .cleanable(true)
                        .prefix(Icon::new(IconName::Search).small()),
                )
                .w(rems(13.)),
            )
            .child(filter)
            .child(sort)
            .child(
                Button::new("refresh")
                    .ghost()
                    .small()
                    .icon(IconName::RefreshCw)
                    .tooltip_with_action("Refresh", &Refresh, None)
                    .on_click(|_, window, cx| window.dispatch_action(Box::new(Refresh), cx)),
            )
    }
}
