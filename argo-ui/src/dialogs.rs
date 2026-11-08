//! Project chrome: the file/function explorer over the ingested project. It navigates; nothing
//! here edits the graph or starts work.

use std::collections::{HashMap, HashSet};

use rune_ui::{Color, Cursor, El, ScrollSpec, TextStyle, col, el, row, text, text_input};

use crate::app::State;
use crate::model::Evidence;
use crate::palette::Pal;
use crate::project;
use crate::ui::{LABEL, M, TOP, UI, UI_STRONG, card, explorer_w, label, viewport};

const FIELD: TextStyle = TextStyle::new(12.5, 450.0);
/// Rows the explorer lists at most while filtering.
const FILTER_LIMIT: usize = 300;

fn field(p: &Pal, input: El) -> El {
    el().h(34.0)
        .px(10.0)
        .radius(8.0)
        .fill(p.code_bg)
        .border(1.0, p.panel_border)
        .items_center()
        .child(input.grow(1.0))
}

/// A field's placeholder, led by an en space: Rune draws a focused empty field's caret at the
/// placeholder's first glyph, which makes the field read as already filled. (A thin or word
/// space still leaves the 1.5 pt caret touching the glyph.)
fn hint(s: &str) -> String {
    format!("\u{2002}{s}")
}

fn field_label(p: &Pal, s: &str) -> El {
    text(s.to_uppercase()).font(LABEL).color(p.muted)
}

/// A function's translation state, as the explorer marks it with a dot at the row's right end.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mark {
    /// A worker is translating it right now: blue.
    Working,
    /// Rust was pushed for it: green.
    Done,
    /// Translation failed: red.
    Failed,
}

impl Mark {
    /// From the function's evidence and claim. An active claim wins: a requeued function being
    /// translated again shows as in progress.
    pub(crate) fn of(evidence: Evidence, claim: Option<crate::model::Claim>) -> Option<Mark> {
        match (claim, evidence) {
            (Some(c), _) if c.active => Some(Mark::Working),
            (_, Evidence::Accepted | Evidence::Unvalidated) => Some(Mark::Done),
            (_, Evidence::Rejected) => Some(Mark::Failed),
            (_, Evidence::None) => None,
        }
    }

    pub(crate) fn color(self, p: &Pal) -> Color {
        match self {
            Mark::Working => p.accent,
            Mark::Done => p.ok,
            Mark::Failed => p.bad,
        }
    }
}

/// Width of the explorer's right-hand status column, so dots and bubbles line up.
const MARK_W: f32 = 16.0;

/// Each listed function's evidence and claim, by node. The snapshot holds a status row for
/// every graph node (tens of thousands) and finds one by scanning, so the explorer looks its
/// functions up together in one pass rather than one scan per row on every frame.
type Statuses = HashMap<argo_protocol::NodeId, (Evidence, Option<crate::model::Claim>)>;

fn statuses(s: &State, proj: &project::Project) -> Statuses {
    let Some(live) = s.live.as_ref() else { return Statuses::new() };
    let listed: HashSet<argo_protocol::NodeId> = proj.modules.iter().flat_map(|m| m.functions.iter().map(|(id, _)| *id)).collect();
    // The listed functions' pieces too: each is a row of its own, translated before the function.
    let wanted: HashSet<argo_protocol::NodeId> = listed
        .iter()
        .flat_map(|f| live.pieces.get(f).into_iter().flatten().copied())
        .chain(listed.iter().copied())
        .collect();
    let mut raw = Statuses::with_capacity(wanted.len());
    for st in &live.snap.status {
        // The first row for a node is the one `Snapshot::status` finds.
        if wanted.contains(&st.node) && !raw.contains_key(&st.node) {
            let (_, evidence, claim) = crate::bridge::status_shown(&live.snap, st);
            raw.insert(st.node, (evidence, claim));
        }
    }
    // A function as the explorer shows it: in progress while any of its pieces is being
    // translated, and done once every piece is, before its own row is.
    let mut out = Statuses::with_capacity(listed.len());
    for &f in &listed {
        let own = raw.get(&f).copied().unwrap_or((Evidence::None, None));
        let pieces: Vec<_> = live.pieces.get(&f).into_iter().flatten().map(|p| raw.get(p).copied().unwrap_or((Evidence::None, None))).collect();
        let effective = match Mark::of(own.0, own.1) {
            Some(Mark::Working) => own,
            mark => match pieces.iter().find_map(|(_, c)| c.filter(|c| c.active)) {
                Some(c) => (own.0, Some(c)),
                None if mark.is_none()
                    && !pieces.is_empty()
                    && pieces.iter().all(|(e, c)| Mark::of(*e, *c) == Some(Mark::Done)) =>
                {
                    (Evidence::Unvalidated, None)
                }
                None => own,
            },
        };
        out.insert(f, effective);
    }
    out
}

/// `id`'s mark and its tooltip, in SpacetimeDB mode only.
fn mark(st: &Statuses, id: argo_protocol::NodeId) -> Option<(Mark, String)> {
    let (evidence, claim) = st.get(&id).copied().unwrap_or((Evidence::None, None));
    let mark = Mark::of(evidence, claim)?;
    let tip = match (mark, evidence) {
        (Mark::Working, _) => format!("Being translated by worker {}", claim.map_or(0, |c| c.worker)),
        (Mark::Done, Evidence::Accepted) => "Rust pushed and accepted".into(),
        (Mark::Done, _) => "Rust pushed (compiles; not linked or tested yet)".into(),
        (Mark::Failed, _) => "Translation failed".into(),
    };
    Some((mark, tip))
}

/// The right-hand status column of a function row: its dot, or empty space.
fn dot_slot(p: &Pal, st: &Statuses, id: argo_protocol::NodeId) -> El {
    let slot = row().w(MARK_W).h(MARK_W).items_center().justify_center();
    match mark(st, id) {
        Some((m, tip)) => slot
            .child(el().size(8.0, 8.0).radius(4.0).fill(m.color(p)))
            .help(tip),
        None => slot,
    }
}

/// A folder row's counts, from its functions' marks: how many are being translated right now
/// (the blue bubble) and how many have Rust pushed (all of them: the green check).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
struct Tally {
    working: usize,
    done: usize,
}

impl Tally {
    fn of(marks: impl IntoIterator<Item = Option<Mark>>) -> Tally {
        let mut t = Tally::default();
        for m in marks {
            match m {
                Some(Mark::Working) => t.working += 1,
                Some(Mark::Done) => t.done += 1,
                _ => {}
            }
        }
        t
    }
}

/// The right-hand status column of a folder row: a green check once every one of its functions
/// has Rust pushed, and a blue bubble counting those being translated right now (rightmost),
/// left out at zero.
fn bubble_slot(p: &Pal, st: &Statuses, functions: &[(argo_protocol::NodeId, String)]) -> El {
    let t = Tally::of(functions.iter().map(|(id, _)| mark(st, *id).map(|(m, _)| m)));
    let bubble = |n: usize, fill: Color, tip: String| {
        row()
            .h(14.0)
            .min_w(14.0)
            .px(4.0)
            .radius(7.0)
            .fill(fill)
            .items_center()
            .justify_center()
            .help(tip)
            .child(
                text(n.to_string())
                    .font(TextStyle::new(9.0, 700.0))
                    .color(Color::WHITE),
            )
    };
    let mut slot = row().min_w(MARK_W).h(MARK_W).gap(4.0).items_center().justify_end();
    if t.done > 0 && t.done == functions.len() {
        // A plain check in the dots' green, no background.
        let tip = format!("All {} functions in this file have Rust pushed", t.done);
        slot = slot.child(
            row()
                .h(14.0)
                .items_center()
                .help(tip)
                .child(text("✓").font(TextStyle::new(11.0, 800.0)).color(p.ok)),
        );
    }
    if t.working > 0 {
        let tip = match t.working {
            1 => "1 function in this file is being translated".to_string(),
            n => format!("{n} functions in this file are being translated"),
        };
        slot = slot.child(bubble(t.working, p.accent, tip));
    }
    slot
}

/// Files and their functions, filterable, docked on the left under the breadcrumb.
pub fn explorer(p: &Pal, s: &State) -> El {
    let Some(proj) = &s.project else { return el() };
    let current = project::current(s).map(|(id, _)| id);
    let st = statuses(s, proj);
    let skipped: Vec<_> = proj.units.iter().filter(|u| u.skipped.is_some()).collect();
    let tip = if skipped.is_empty() {
        format!("All {} .c files are in the graph", proj.units.len())
    } else {
        let lines: Vec<String> = skipped
            .iter()
            .map(|u| format!("{}: {}", u.path, u.skipped.as_deref().unwrap_or("")))
            .collect();
        format!("Left out:\n{}", lines.join("\n"))
    };
    let header = row()
        .gap(8.0)
        .items_center()
        .child(
            text(proj.name.clone())
                .font(TextStyle::new(14.0, 650.0))
                .color(p.text)
                .single_line()
                .truncate()
                .grow(1.0)
                .min_w(0.0),
        )
        .child(
            row()
                .h(20.0)
                .px(7.0)
                .radius(5.0)
                .fill(p.code_bg)
                .items_center()
                .help(tip)
                .child(
                    text(format!("{} / {} files", proj.included(), proj.units.len()))
                        .font(TextStyle::new(10.5, 600.0))
                        .color(p.muted),
                ),
        );
    // No function bodies: nothing to filter, and the list's one line says what there is.
    let empty = proj.modules.is_empty();
    let sub = label(
        &if empty {
            proj.source.clone()
        } else {
            format!("{} functions · {}", proj.function_count(), proj.source)
        },
        p.faint,
    )
    .single_line()
    .truncate()
    .help(format!(
        "Checked out at {}\nPreprocessor flags: {}",
        proj.checkout,
        proj.flags.join(" ")
    ));
    let filter = field(
        p,
        text_input(&s.forms.filter, FIELD)
            .single_line()
            .placeholder(hint("Filter functions"), p.faint)
            .on_edit(|s: &mut State| &mut s.forms.filter),
    );

    let mut list = col().gap(1.0);
    // A filter typed for an earlier project is ignored while this one has nothing to filter.
    let query = if empty { String::new() } else { s.forms.filter.text.trim().to_lowercase() };
    let fn_row = |name: &str, id, indent: f32, module: Option<&str>| {
        let on = Some(id) == current;
        let mut r = row()
            .h(26.0)
            .pl(8.0 + indent)
            .pr(8.0)
            .gap(8.0)
            .radius(6.0)
            .items_center()
            .cursor(Cursor::Pointer)
            .hover(p.code_bg)
            .child(text("ƒ").font(TextStyle::new(11.0, 650.0)).color(if on {
                p.accent
            } else {
                p.faint
            }))
            .child(
                text(name.to_string())
                    .font(crate::ui::MONO)
                    .color(if on { p.text } else { p.muted })
                    .single_line()
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            );
        if let Some(m) = module {
            r = r.child(
                text(m.to_string())
                    .font(TextStyle::new(10.5, 450.0))
                    .color(p.faint)
                    .single_line(),
            );
        }
        // Last, so the dot is right-aligned in the same column on every row.
        r = r.child(dot_slot(p, &st, id));
        if on {
            r = r.fill(p.accent.alpha(0.16));
        }
        r.on_press(move |s: &mut State| project::open_function(s, id))
    };
    if empty {
        // Parsed, but nothing defines a function: say what it holds (the canvas shows those
        // declarations).
        list = list.child(label(&proj.empty_note(), p.muted).wrap().key("explorer-empty"));
    }
    if query.is_empty() {
        for m in &proj.modules {
            let open = s.forms.expanded == Some(m.id);
            let id = m.id;
            list = list.child(
                row()
                    .h(28.0)
                    .px(8.0)
                    .gap(8.0)
                    .radius(6.0)
                    .items_center()
                    .cursor(Cursor::Pointer)
                    .hover(p.code_bg)
                    .child(
                        text(if open { "▾" } else { "▸" })
                            .font(TextStyle::new(10.0, 600.0))
                            .color(p.faint)
                            .w(10.0),
                    )
                    .child(
                        text(m.name.clone())
                            .font(UI_STRONG)
                            .color(p.text)
                            .single_line()
                            .truncate()
                            .grow(1.0)
                            .min_w(0.0),
                    )
                    .child(
                        text(m.functions.len().to_string())
                            .font(TextStyle::new(10.5, 600.0))
                            .color(p.faint),
                    )
                    .child(bubble_slot(p, &st, &m.functions))
                    .on_press(move |s: &mut State| {
                        s.forms.expanded = if s.forms.expanded == Some(id) {
                            None
                        } else {
                            Some(id)
                        };
                    }),
            );
            if open {
                for (fid, name) in &m.functions {
                    list = list.child(fn_row(name, *fid, 18.0, None));
                }
            }
        }
        if !skipped.is_empty() {
            list = list.child(el().h(10.0)).child(
                field_label(p, &format!("Left out · {}", skipped.len()))
                    .ml(8.0)
                    .mb(4.0),
            );
            for u in &skipped {
                list = list.child(
                    row()
                        .h(24.0)
                        .px(8.0)
                        .gap(8.0)
                        .items_center()
                        .help(u.skipped.clone().unwrap_or_default())
                        .child(text("–").font(UI).color(p.faint).w(10.0))
                        .child(
                            label(&u.path, p.faint)
                                .single_line()
                                .truncate()
                                .grow(1.0)
                                .min_w(0.0),
                        ),
                );
            }
        }
    } else {
        let mut n = 0;
        for m in &proj.modules {
            for (fid, name) in &m.functions {
                if n < FILTER_LIMIT && name.to_lowercase().contains(&query) {
                    list = list.child(fn_row(name, *fid, 0.0, Some(&m.name)));
                    n += 1;
                }
            }
        }
        if n == 0 {
            list = list.child(label("No matching functions", p.faint).ml(8.0).mt(6.0));
        }
    }
    let full_h = viewport()[1] - TOP - M;
    let panel = card(p)
        .w(explorer_w())
        .padding(12.0)
        .gap(10.0)
        .child(col().gap(3.0).child(header).child(sub));
    if empty {
        // As tall as what it says, up to the window's room for it.
        return panel.max_h(full_h).clip_soft(0.0).child(list);
    }
    panel.h(full_h).child(filter).child(
        el().grow(1.0).min_h(0.0).child(
            list.scroll(ScrollSpec {
                stick_bottom: false,
            })
            .grow(1.0)
            .min_h(0.0),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune_ui::{Env, Runtime, TextEdit};

    #[test]
    fn explorer_marks_working_blue_then_done_green() {
        use crate::model::Claim;
        let working = Some(Claim { worker: 3, active: true });
        let queued = Some(Claim { worker: 3, active: false });
        assert_eq!(Mark::of(Evidence::None, working), Some(Mark::Working));
        assert_eq!(Mark::of(Evidence::Unvalidated, working), Some(Mark::Working), "retranslating");
        assert_eq!(Mark::of(Evidence::None, queued), None, "a lapsed claim isn't being worked");
        assert_eq!(Mark::of(Evidence::Unvalidated, None), Some(Mark::Done));
        assert_eq!(Mark::of(Evidence::Accepted, None), Some(Mark::Done));
        assert_eq!(Mark::of(Evidence::Rejected, None), Some(Mark::Failed));
        assert_eq!(Mark::of(Evidence::None, None), None);
        let p = crate::palette::Pal::new(true);
        assert_eq!(Mark::Working.color(&p), p.accent);
        assert_eq!(Mark::Done.color(&p), p.ok);
    }

    #[test]
    fn a_folder_counts_its_functions_being_translated_and_done() {
        let (w, d, f) = (Some(Mark::Working), Some(Mark::Done), Some(Mark::Failed));
        assert_eq!(Tally::of([w, d, None]), Tally { working: 1, done: 1 });
        assert_eq!(Tally::of([w, w, d, d, d, f]), Tally { working: 2, done: 3 });
        assert_eq!(Tally::of([None, f]), Tally::default());
    }

    fn render(s: State) -> Runtime<State> {
        let mut rt = Runtime::new(s, crate::view, (1024.0, 640.0), Env::new(true, 1.0));
        rt.frame(0.0);
        rt.frame(1.0 / 60.0);
        rt
    }

    fn empty_project() -> project::Project {
        project::Project {
            name: "settings".into(),
            source: "~/src/settings".into(),
            checkout: "commit 0123abc".into(),
            units: vec![argo_db_client::Unit {
                path: "config.c".into(),
                skipped: None,
            }],
            flags: vec![],
            modules: vec![],
            declarations: 2,
        }
    }

    #[test]
    fn a_project_without_functions_shows_the_empty_state() {
        let mut s = State::load("fixture");
        s.project = Some(empty_project());
        let mut rt = render(s);
        let tree = rt.accessibility_json().to_string();
        // It says what the project does hold, and there is nothing to filter.
        assert!(tree.contains("2 declarations, no function bodies"), "{tree}");
        assert!(!tree.contains("0 functions") && !tree.contains("Filter functions"), "{tree}");

        // A filter typed for an earlier project doesn't hide what this one holds.
        rt.update(|s, _| s.forms.filter = TextEdit::new("x"));
        rt.frame(2.0 / 60.0);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains("2 declarations, no function bodies"), "{tree}");
        assert!(!tree.contains("No matching functions"));
    }

    #[test]
    fn the_empty_note_counts_what_was_parsed() {
        let mut p = empty_project();
        assert_eq!(p.empty_note(), "2 declarations, no function bodies");
        p.declarations = 1;
        assert_eq!(p.empty_note(), "1 declaration, no function bodies");
        p.declarations = 0;
        assert_eq!(p.empty_note(), "Nothing declared");
    }
}
