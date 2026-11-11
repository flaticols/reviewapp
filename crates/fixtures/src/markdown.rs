/// A pull request description that uses most of GitHub Flavored Markdown, for checking
/// how comments and descriptions render.
pub const KITCHEN_SINK: &str = r#"> [!NOTE]
> Useful information that users should know, even when skimming content.

> [!TIP]
> Helpful advice for doing things better or more easily.

> [!IMPORTANT]
> Key information users need to know to achieve their goal.

> [!WARNING]
> Urgent info that needs immediate user attention to avoid problems.

> [!CAUTION]
> Advises about risks or negative outcomes of certain actions.

```mermaid
flowchart LR
  A[PR opened] --> B{CI green?}
  B -- yes --> C[Review]
  B -- no --> D[Fix it]
  D --> B
  C --> E((Merge))
```

## Checklist

- [x] Parse alerts
- [ ] Render ~~everything~~ most things
- [ ] Ship it :rocket:

| Area | Status | Owner |
| :--- | :----: | ----: |
| Alerts | done | @flaticols |
| Mermaid | **in progress** | #482 |

Plain quote with `inline code` and a [link](https://github.com).

<details>
<summary>Logs</summary>

```rust
fn main() {
    println!("hidden until opened");
}
```

</details>

Press <kbd>Cmd</kbd>+<kbd>K</kbd>. H<sub>2</sub>O and x<sup>2</sup>.

Footnote reference[^1] and an autolink https://example.com.

---

```sequence
not a language, stays a code block
```

```mermaid
sequenceDiagram
  participant R as Reviewer
  participant S as Switchfold
  participant G as GitHub
  R->>S: Approve
  S->>G: POST review
  G-->>S: 200 OK
```

[^1]: The footnote text.
"#;
