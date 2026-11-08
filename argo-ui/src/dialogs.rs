//! Project chrome: the file/function explorer. It navigates; the canvas itself stays read-only.

use rune_ui::{Cursor, El, ScrollSpec, TextStyle, col, el, row, text, text_input};

use crate::app::State;
use crate::model::Evidence;
use crate::palette::Pal;
use crate::project;
use crate::ui::{EXPLORER_W, H, LABEL, M, UI, UI_STRONG, card, label};

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

fn field_label(p: &Pal, s: &str) -> El {
    text(s.to_uppercase()).font(LABEL).color(p.muted)
}

/// A function's translation state beside its name: the claiming worker, else its evidence badge.
/// Calls fold into statements on the canvas, so this is where every function's state shows.
fn work_mark(p: &Pal, s: &State, id: argo_protocol::NodeId) -> Option<El> {
    let live = s.live.as_ref()?;
    let (_, evidence, claim) = crate::bridge::own_status(&live.snap, id);
    if let Some(c) = claim {
        let tip = format!(
            "worker {} · {}",
            c.worker,
            if c.active { "translating" } else { "queued" }
        );
        return Some(crate::graph::avatar(p, c.worker, c.active, 16.0).help(tip));
    }
    let (glyph, color, tip) = match evidence {
        Evidence::None => return None,
        Evidence::Accepted => ("✓", p.ok, "Rust accepted"),
        Evidence::Unvalidated => ("?", p.pending, "Rust candidate, not validated"),
        Evidence::Rejected => ("✕", p.bad, "translation failed"),
    };
    Some(
        el().size(16.0, 16.0)
            .radius(8.0)
            .fill(color.alpha(0.18))
            .center()
            .help(tip)
            .child(text(glyph).font(TextStyle::new(9.5, 800.0)).color(color)),
    )
}

/// Files and their functions, filterable, docked on the left under the breadcrumb.
pub fn explorer(p: &Pal, s: &State) -> El {
    let Some(proj) = &s.project else { return el() };
    let current = project::current(s).map(|(id, _)| id);
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
    let sub = label(
        &format!("{} functions · {}", proj.function_count(), proj.source),
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
            .placeholder("Filter functions", p.faint)
            .on_edit(|s: &mut State| &mut s.forms.filter),
    );

    let mut list = col().gap(1.0);
    let query = s.forms.filter.text.trim().to_lowercase();
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
        if let Some(mark) = work_mark(p, s, id) {
            r = r.child(mark);
        }
        if let Some(m) = module {
            r = r.child(
                text(m.to_string())
                    .font(TextStyle::new(10.5, 450.0))
                    .color(p.faint)
                    .single_line(),
            );
        }
        if on {
            r = r.fill(p.accent.alpha(0.16));
        }
        r.on_press(move |s: &mut State| project::open_function(s, id))
    };
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
    card(p)
        .w(EXPLORER_W)
        .h(H - 72.0 - M)
        .padding(12.0)
        .gap(10.0)
        .child(col().gap(3.0).child(header).child(sub))
        .child(filter)
        .child(
            el().grow(1.0).min_h(0.0).child(
                list.scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .grow(1.0)
                .min_h(0.0),
            ),
        )
}
