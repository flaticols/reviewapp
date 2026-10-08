//! Unified patches for the files of pull request #482, in the shape GitHub's files API
//! returns them.

pub(crate) const THREAD_RS: &str = "\
@@ -136,6 +136,9 @@ impl ReviewThread {
     pub fn apply_push(&mut self, push: Push, cx: &mut Context<Self>) {
         let blocks = push.files.iter().map(DiffBlock::from_file);
-        self.blocks = blocks.collect();
+        for block in blocks {
+            let key = BlockKey::new(&block.path, block.hunk_hash());
+            self.blocks.entry(key).or_insert(block);
+        }
         self.reanchor_threads(cx);
         cx.notify();
     }
@@ -201,6 +204,9 @@ impl ReviewThread {
     fn reanchor_threads(&mut self, cx: &mut Context<Self>) {
         for thread in self.threads.values_mut() {
-            thread.anchor = self.blocks.line_for(thread.original_line);
+            let Some(block) = self.blocks.get(&thread.block_key) else {
+                continue;
+            };
+            thread.anchor = block.translate(thread.original_line);
         }
         cx.notify();
     }
@@ -240,5 +246,5 @@ impl ReviewThread {
     pub fn unresolved(&self) -> impl Iterator<Item = &Thread> {
-        self.threads.values().filter(|t| !t.resolved)
+        self.threads.values().filter(|thread| !thread.is_resolved)
     }

     pub fn blocks(&self) -> impl Iterator<Item = &DiffBlock> {
";

pub(crate) const DIFF_VIEW_RS: &str = "\
@@ -0,0 +1,120 @@
+//! Renders one file's diff block inside the review thread.
+
+use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
+use gpui_kit::{
+    App, ElementId, IntoElement, ParentElement as _, RenderOnce, SharedString, Styled as _,
+    Window, div,
+};
+
+use gpui_kit::prelude::FluentBuilder as _;
+
+use crate::block::{DiffBlock, LineKind};
+
+/// Files with more changed lines than this start collapsed.
+pub const COLLAPSE_THRESHOLD: usize = 100;
+
+/// One file's hunks with a two-column gutter.
+#[derive(IntoElement)]
+pub struct DiffView {
+    id: ElementId,
+    block: DiffBlock,
+    collapsed: bool,
+}
+
+impl DiffView {
+    /// Creates a view for `block`, collapsed when it is large.
+    pub fn new(id: impl Into<ElementId>, block: DiffBlock) -> Self {
+        let collapsed = block.changed_lines() > COLLAPSE_THRESHOLD;
+        Self {
+            id: id.into(),
+            block,
+            collapsed,
+        }
+    }
+
+    /// Overrides the default collapsed state.
+    pub fn collapsed(mut self, collapsed: bool) -> Self {
+        self.collapsed = collapsed;
+        self
+    }
+
+    fn render_header(&self, cx: &App) -> impl IntoElement {
+        h_flex()
+            .h_10()
+            .px_3()
+            .gap_2()
+            .bg(cx.theme().muted)
+            .child(SharedString::from(self.block.path.clone()))
+            .child(
+                div()
+                    .text_color(cx.theme().success)
+                    .child(format!(\"+{}\", self.block.additions())),
+            )
+            .child(
+                div()
+                    .text_color(cx.theme().danger)
+                    .child(format!(\"-{}\", self.block.deletions())),
+            )
+    }
+
+    fn render_lines(&self, cx: &App) -> impl IntoElement {
+        v_flex().children(self.block.lines().map(|line| {
+            let background = match line.kind {
+                LineKind::Added => cx.theme().success.opacity(0.1),
+                LineKind::Removed => cx.theme().danger.opacity(0.1),
+                LineKind::Context => cx.theme().transparent,
+            };
+            h_flex()
+                .h_5()
+                .bg(background)
+                .child(
+                    div()
+                        .w_12()
+                        .text_color(cx.theme().muted_foreground)
+                        .child(line.old_number()),
+                )
+                .child(
+                    div()
+                        .w_12()
+                        .text_color(cx.theme().muted_foreground)
+                        .child(line.new_number()),
+                )
+                .child(div().flex_1().child(line.text.clone()))
+        }))
+    }
+}
+
+impl RenderOnce for DiffView {
+    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
+        v_flex()
+            .id(self.id.clone())
+            .border_1()
+            .border_color(cx.theme().border)
+            .rounded(cx.theme().radius)
+            .child(self.render_header(cx))
+            .when(!self.collapsed, |this| this.child(self.render_lines(cx)))
+    }
+}
+
+#[cfg(test)]
+mod tests {
+    use super::*;
+
+    #[test]
+    fn large_blocks_start_collapsed() {
+        let block = DiffBlock::synthetic(\"big.rs\", COLLAPSE_THRESHOLD + 1);
+        assert!(DiffView::new(\"big\", block).collapsed);
+    }
+
+    #[test]
+    fn small_blocks_start_expanded() {
+        let block = DiffBlock::synthetic(\"small.rs\", 3);
+        assert!(!DiffView::new(\"small\", block).collapsed);
+    }
+
+    #[test]
+    fn collapsed_can_be_overridden() {
+        let block = DiffBlock::synthetic(\"big.rs\", 500);
+        assert!(!DiffView::new(\"big\", block).collapsed(false).collapsed);
+    }
+}
";

pub(crate) const HUNK_RS: &str = "\
@@ -24,19 +24,12 @@ impl Hunk {
     pub fn hash(&self) -> HunkHash {
         let mut hasher = DefaultHasher::new();
-        for line in &self.lines {
-            match line.kind {
-                LineKind::Context => {}
-                LineKind::Added => {
-                    1u8.hash(&mut hasher);
-                    line.text.hash(&mut hasher);
-                }
-                LineKind::Removed => {
-                    2u8.hash(&mut hasher);
-                    line.text.hash(&mut hasher);
-                }
-            }
-        }
-        HunkHash(hasher.finish())
+        let ids: Vec<_> = self.hunks.iter().map(|hunk| hunk.id).collect();
+        self.fetch_blobs(&ids);
+        for line in self.lines.iter().filter(|line| line.kind != LineKind::Context) {
+            (line.kind as u8).hash(&mut hasher);
+            line.text.hash(&mut hasher);
+        }
+        HunkHash(hasher.finish())
     }

     pub fn translate(&self, line: u32) -> Option<u32> {
@@ -61,4 +54,5 @@ impl Hunk {
         let offset = self.new_start as i64 - self.old_start as i64;
         Some((line as i64 + offset) as u32)
     }
+
 }
";

pub(crate) const RENDER_RS: &str = "\
@@ -80,13 +80,27 @@ impl Render for ReviewThread {
     fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
         let items = self.timeline.items();
-        div()
-            .id(\"thread\")
-            .overflow_y_scroll()
+        v_flex()
+            .id(\"thread\")
+            .size_full()
+            .overflow_y_scroll()
+            .track_scroll(&self.scroll_handle)
             .children(items.map(|item| match item {
-                TimelineItem::Diff(block) => DiffView::new(block.key(), block).into_any_element(),
+                TimelineItem::Diff(block) => {
+                    let key = block.key();
+                    let collapsed = self.collapsed.contains(&key);
+                    DiffView::new(key, block)
+                        .collapsed(collapsed)
+                        .into_any_element()
+                }
+                TimelineItem::Finding(finding) => {
+                    FindingCard::new(finding.id.clone(), finding).into_any_element()
+                }
+                TimelineItem::Check(check) => {
+                    CheckRow::new(check.id.clone(), check).into_any_element()
+                }
                 TimelineItem::Comment(comment) => {
                     CommentCard::new(comment.id.clone(), comment).into_any_element()
                 }
             }))
     }
 }
";

pub(crate) const MOD_RS: &str = "\
@@ -1,6 +1,8 @@
 mod block;
+mod diff_view;
 mod hunk;
 mod render;
 mod thread;

+pub use diff_view::DiffView;
 pub use thread::ReviewThread;
";

pub(crate) const CARGO_TOML: &str = "\
@@ -8,6 +8,6 @@ publish = false
 [dependencies]
 anyhow.workspace = true
-gpui-kit = \"0.8\"
+gpui-kit = \"0.9\"
 smallvec.workspace = true

 [dev-dependencies]
";

pub(crate) const README_MD: &str = "\
@@ -12,3 +12,9 @@ cargo run -p switchfold
 ## Review thread

 The review screen is one scroll.
+
+- Descriptions, commits, diffs, comments, agent findings and checks appear in the
+  order they happened.
+- Each diff block is keyed by file and hunk hash, so unchanged hunks keep their
+  scroll position and inline threads across pushes.
+- Files with more than 100 changed lines start collapsed.
";

/// A small patch for pull requests that only need a plausible diff.
pub(crate) fn generic_patch(title: &str) -> String {
    format!(
        "\
@@ -10,5 +10,7 @@ pub fn run(cx: &mut App) {{
     let settings = Settings::load(cx);
     let window = open_window(&settings, cx);
-    window.focus(cx);
+    // {title}
+    window.activate(cx);
+    window.focus(cx);
     cx.activate(true);
 }}
"
    )
}
