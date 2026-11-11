use std::collections::HashSet;

use diff_view_ui::{DiffRow, diff_rows};
use review_model::TimelineItem;

use app_state_ui::StoresExt as _;

use crate::ReviewThreadView;

/// One row of the virtualized timeline. A file expands into one row per diff line so
/// a frame lays out only the lines on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TimelineRow {
    /// A non-file timeline item, by index into the filtered items.
    Item(usize),
    /// One row of the file at `item`; `last` closes its card.
    File {
        item: usize,
        row: DiffRow,
        last: bool,
    },
    /// Shown when the filter matches nothing.
    Empty,
    /// An agent review in progress, by index into the view's active runs.
    AgentRun(usize),
    /// The Reviews tab's heading while agents have run: how many are at work, with
    /// Add an agent or Hide.
    ReviewsHeader,
    /// The Review with an agent form.
    StartForm,
    /// The banner above a single commit's changes.
    CommitBanner,
    /// An agent review of this session in full, by index into the view's runs.
    RunDetails(usize),
    /// The comment composer at the end of Everything.
    Composer,
}

impl TimelineRow {
    /// Whether the row starts a new entry, which gets the gap between entries.
    pub(crate) fn starts_entry(self) -> bool {
        !matches!(
            self,
            TimelineRow::File { row, .. } if row != DiffRow::Header
        )
    }
}

impl ReviewThreadView {
    /// Whether the Reviews tab shows the start form: open until the first run, then
    /// only when asked for.
    pub(crate) fn start_form_open(&self, cx: &gpui_kit::App) -> bool {
        self.start_form_open.unwrap_or_else(|| {
            agent_review_ui::runs_for(&self.key, cx).is_empty()
                && cx.stores().settings().read(cx).default_agent().is_some()
        })
    }

    /// Rebuilds the rows for the current detail, filter, mode and collapsed files, and
    /// tells the list which range changed so it keeps measured heights and the scroll
    /// position for the rest.
    pub(crate) fn sync_rows(&mut self, cx: &gpui_kit::App) {
        let Some(detail) = self.detail.clone() else {
            return;
        };
        let mut rows = Vec::new();
        let items: Vec<TimelineItem> = match &self.commit {
            Some(commit) => {
                rows.push(TimelineRow::CommitBanner);
                match &commit.files {
                    Some(Ok(files)) => files
                        .iter()
                        .map(|file| TimelineItem::File {
                            path: file.path.clone(),
                        })
                        .collect(),
                    _ => Vec::new(),
                }
            }
            None if self.filter == crate::TimelineFilter::Unresolved => {
                let mut findings: Vec<&review_model::AgentFinding> = detail
                    .findings
                    .iter()
                    .filter(|f| f.state == review_model::FindingState::Draft)
                    .collect();
                crate::triage::sort_findings(&self.key, &mut findings, cx);
                findings
                    .into_iter()
                    .map(|f| TimelineItem::Finding { id: f.id.clone() })
                    .collect()
            }
            None => crate::filters::items(&detail, self.filter),
        };
        let whole = self.commit.is_none();
        self.runs = if whole && self.filter == crate::TimelineFilter::Reviews {
            agent_review_ui::runs_for(&self.key, cx)
        } else {
            Vec::new()
        };
        if whole && self.filter == crate::TimelineFilter::Reviews {
            if !self.runs.is_empty() {
                rows.push(TimelineRow::ReviewsHeader);
            }
            if self.start_form_open(cx) {
                rows.push(TimelineRow::StartForm);
            }
        }
        rows.extend((0..self.runs.len()).map(TimelineRow::RunDetails));
        for (ix, item) in items.iter().enumerate() {
            let TimelineItem::File { path } = item else {
                rows.push(TimelineRow::Item(ix));
                continue;
            };
            let Some(file) = self.files.get(path) else {
                continue;
            };
            let mut anchored: HashSet<u32> = detail
                .threads
                .iter()
                .filter(|thread| &thread.path == path)
                .map(|thread| thread.line)
                .collect();
            if let Some((new_path, line, _)) = &self.new_thread
                && new_path == path
            {
                anchored.insert(*line);
            }
            anchored.extend(self.ask_anchors(path, cx));
            let file_rows = diff_rows(file, self.mode, self.collapsed.contains(path), |line| {
                anchored.contains(&line)
            });
            let count = file_rows.len();
            rows.extend(
                file_rows
                    .into_iter()
                    .enumerate()
                    .map(|(row_ix, row)| TimelineRow::File {
                        item: ix,
                        row,
                        last: row_ix + 1 == count,
                    }),
            );
        }
        if rows.is_empty() {
            rows.push(TimelineRow::Empty);
        }
        self.active_runs = if !whole || self.filter == crate::TimelineFilter::Reviews {
            Vec::new()
        } else {
            agent_review_ui::active_runs(&self.key, cx)
        };
        rows.extend((0..self.active_runs.len()).map(TimelineRow::AgentRun));
        if whole && self.filter == crate::TimelineFilter::Everything {
            rows.push(TimelineRow::Composer);
        }

        let old = &self.rows;
        let prefix = old.iter().zip(&rows).take_while(|(a, b)| a == b).count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(rows[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let changed = prefix != old.len() || old.len() != rows.len();
        if changed {
            self.list_state
                .splice(prefix..old.len() - suffix, rows.len() - prefix - suffix);
        }
        self.items = items.into();
        // Kept when nothing changed, so find can tell it needn't search again.
        if changed {
            self.rows = rows.into();
        }
    }

    /// The row of `path`'s header, after [`Self::sync_rows`].
    pub(crate) fn header_row(&self, path: &str) -> Option<usize> {
        self.rows.iter().position(|row| {
            matches!(row, TimelineRow::File { item, row: DiffRow::Header, .. }
                if matches!(&self.items[*item], TimelineItem::File { path: p } if p == path))
        })
    }
}
