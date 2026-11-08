//! Project chrome: the toolbar, the file/function explorer, and the open and progress dialogs.
//! These open projects and navigate them; nothing here edits the graph or starts work.

use rune_ui::{Color, Cursor, El, LoopSpec, ScrollSpec, TextStyle, Tx, col, el, row, text, text_input};

use crate::app::State;
use crate::palette::Pal;
use crate::project::{self, Modal};
use crate::ui::{CONTROL_H, CONTROL_R, LABEL, M, MONO, TOP, UI, UI_STRONG, bar, card, explorer_w, label, viewport};

const TITLE: TextStyle = TextStyle::new(17.0, 650.0);
const BODY: TextStyle = TextStyle::new(12.5, 450.0);
const FIELD: TextStyle = TextStyle::new(12.5, 450.0);
const DIALOG_W: f32 = 520.0;
/// Rows the explorer lists at most while filtering.
const FILTER_LIMIT: usize = 300;

/// Horizontal padding inside a text button.
const BUTTON_PX: f32 = 14.0;

/// A width a button's label is sure to fit in: a generous per-character advance for the
/// 11.5 pt semibold UI face, so a platform font that sets wider than measured still fits.
fn button_min_w(title: &str) -> f32 {
    title.chars().count() as f32 * 7.2 + 2.0 * BUTTON_PX
}

/// A text button, as tall as a bar control and concentric with a bar card's corner. Its width
/// comes from its label and it never shrinks, so the label is never cut.
fn button(p: &Pal, title: &str, primary: bool) -> El {
    let (fill, fg, hover) = if primary {
        (p.accent, Color::WHITE, p.accent.mix(Color::WHITE, 0.12))
    } else {
        (p.code_bg, p.text, p.code_bg.mix(p.text, 0.08))
    };
    row()
        .h(CONTROL_H)
        .px(BUTTON_PX)
        .min_w(button_min_w(title))
        .radius(CONTROL_R)
        .fill(fill)
        .hover(hover)
        .items_center()
        .justify_center()
        .cursor(Cursor::Pointer)
        .border(1.0, if primary { fill } else { p.panel_border })
        .child(text(title.to_string()).font(UI_STRONG).color(fg))
}

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

/// The toolbar's buttons: title, tooltip, action. Opening a project is the only one, with or
/// without a project open: the app shows a migration and never starts one.
type ToolbarButton = (&'static str, &'static str, fn(&mut State));
const TOOLBAR: [ToolbarButton; 1] = [(
    "Open…",
    "Open a git repository or local directory (Ctrl+O)",
    |s| s.modal = Some(Modal::Open),
)];

/// Top-right actions beside the status card.
pub fn toolbar(p: &Pal, _: &State) -> El {
    let mut r = row().gap(6.0);
    for (title, tip, action) in TOOLBAR {
        r = r.child(button(p, title, false).help(tip).on_press(action));
    }
    bar(p, r).key("toolbar")
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
        proj.root.display(),
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
        .w(explorer_w())
        .h(viewport()[1] - TOP - M)
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

/// The open dialog, or a load's progress, over a dimmed canvas.
pub fn modal(p: &Pal, s: &State) -> El {
    let Some(which) = s.modal else { return el() };
    let dialog = match which {
        Modal::Open => open_dialog(p, s),
        Modal::Loading => loading_dialog(p, s),
    };
    // Esc closes the open dialog or cancels a load. A click outside only closes the open
    // dialog: a stray click shouldn't throw away a clone in progress.
    let dismiss = |s: &mut State| {
        if s.modal == Some(Modal::Loading) {
            project::cancel(s);
        } else {
            s.modal = None;
        }
    };
    el().w_full().h_full()
        .fill(Color::BLACK.alpha(if p.dark { 0.5 } else { 0.25 }))
        .center()
        .child(
            card(p)
                .w(DIALOG_W.min(viewport()[0] - 2.0 * M))
                .padding(22.0)
                .gap(16.0)
                .child(dialog)
                .modal(which != Modal::Loading, dismiss),
        )
}

fn heading(p: &Pal, title: &str, sub: &str) -> El {
    col()
        .gap(6.0)
        .child(text(title.to_string()).font(TITLE).color(p.text))
        .child(text(sub.to_string()).font(BODY).color(p.muted).wrap())
}

fn actions(left: El, right: Vec<El>) -> El {
    let mut r = row().gap(8.0).items_center().child(left.grow(1.0));
    for b in right {
        r = r.child(b);
    }
    r
}

fn open_dialog(p: &Pal, s: &State) -> El {
    let f = &s.forms;
    let mut c = col().gap(16.0)
        .child(heading(p, "Open a C project", "Clone a git repository into a temporary folder, or open a local directory. Every .c file is run through the C preprocessor and combined into one graph."))
        .child(col().gap(6.0).child(field_label(p, "Repository"))
            .child(field(p, text_input(&f.url, FIELD).single_line()
                .placeholder("https://github.com/user/project.git   or   ~/src/project", p.faint)
                .on_edit(|s: &mut State| &mut s.forms.url)
                .on_submit(|s: &mut State, _| project::start(s)))))
        .child(col().gap(6.0).child(field_label(p, "Branch, tag or commit"))
            .child(field(p, text_input(&f.rev, FIELD).single_line()
                .placeholder("default branch", p.faint)
                .on_edit(|s: &mut State| &mut s.forms.rev)
                .on_submit(|s: &mut State, _| project::start(s)))))
        .child(col().gap(6.0).child(field_label(p, "Preprocessor flags"))
            .child(field(p, text_input(&f.flags, FIELD).single_line()
                .placeholder("-DNAME=value -Iinclude", p.faint)
                .on_edit(|s: &mut State| &mut s.forms.flags)
                .on_submit(|s: &mut State, _| project::start(s))))
            .child(label("Added after any -D/-I flags found in the project's Makefile.", p.faint)));
    if let Some(e) = &f.error {
        c = c.child(error_box(p, e, f.error_open));
    }
    c.child(actions(
        el(),
        vec![
            button(p, "Cancel", false).on_press(|s: &mut State| s.modal = None),
            button(p, "Open", true)
                .default_button()
                .on_press(project::start),
        ],
    ))
}

/// Lines of a load error shown before `Show all`.
const ERROR_LINES: usize = 6;
/// Height of one error line, and the most the expanded list takes before it scrolls.
const ERROR_LINE_H: f32 = 18.0;
const ERROR_LIST_MAX_H: f32 = 200.0;
/// The open dialog's height without its error lines (measured from a 1024x640 capture), so the
/// expanded list never pushes the dialog past a short window.
const OPEN_DIALOG_REST_H: f32 = 500.0;

/// How tall the expanded error list may grow before it scrolls: up to `ERROR_LIST_MAX_H`, as
/// the window allows, and never shorter than the collapsed list.
fn error_list_room(window_h: f32) -> f32 {
    (window_h - 2.0 * M - OPEN_DIALOG_REST_H)
        .clamp(ERROR_LINES as f32 * ERROR_LINE_H, ERROR_LIST_MAX_H)
}

/// A load error: its headline, then the first files with their reasons (one line each, the whole
/// line on hover). `Show all` lists the rest in a scrolling list; `Copy diagnostics` puts the
/// whole text on the clipboard.
fn error_box(p: &Pal, e: &str, open: bool) -> El {
    let (head, files, more) = project::error_parts(e, if open { usize::MAX } else { ERROR_LINES });
    let total = files.len() + more;
    let mut c = col()
        .gap(4.0)
        .padding(10.0)
        .radius(8.0)
        .fill(p.bad.alpha(0.12))
        .child(text(head.to_string()).font(BODY).color(p.bad).wrap());
    if !files.is_empty() {
        let mut list = col();
        for line in &files {
            list = list.child(
                row().h(ERROR_LINE_H).items_center().help(line.to_string()).child(
                    text(line.to_string())
                        .font(MONO)
                        .color(p.text)
                        .single_line()
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                ),
            );
        }
        let list_h = files.len() as f32 * ERROR_LINE_H;
        let room = error_list_room(viewport()[1]);
        c = c.child(el().h(2.0)).child(if list_h > room {
            list.h(room)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                // The edge fades over the lines scrolled out of view say there are more.
                .scroll_edge_fade(ERROR_LINE_H)
                .key("error-list")
        } else {
            list
        });
    }
    if project::needs_flags(e) {
        c = c.child(label(
            "Most need a header path or a define: add -I or -D under Preprocessor flags.",
            p.muted,
        )
        .wrap());
    }
    let toggle = if total > ERROR_LINES {
        let title = if open {
            "Show fewer".to_string()
        } else {
            format!("Show all {total}")
        };
        row()
            .h(CONTROL_H)
            .items_center()
            .cursor(Cursor::Pointer)
            .child(text(title).font(UI_STRONG).color(p.accent))
            .on_press(|s: &mut State| s.forms.error_open = !s.forms.error_open)
    } else {
        el()
    };
    c.child(el().h(2.0)).child(actions(
        toggle,
        vec![
            button(p, "Copy diagnostics", false)
                .help("Copy the whole error to the clipboard")
                .on_press_tx(|s: &mut State, tx: &mut Tx| {
                    if let Some(e) = &s.forms.error {
                        tx.copy(e.clone());
                    }
                }),
        ],
    ))
}

fn loading_dialog(p: &Pal, s: &State) -> El {
    let Some(l) = &s.loader else {
        return heading(p, "Opening", "");
    };
    let fill = el().h_full().radius(3.0).fill(p.accent);
    let track = row()
        .w_full()
        .h(6.0)
        .radius(3.0)
        .fill(p.code_bg)
        .clip_soft(0.0)
        .child(match l.progress {
            Some((done, total)) => fill.w_frac(done as f32 / total.max(1) as f32),
            // No count yet (fetching, combining, parsing): a quarter-width segment sweeps the
            // track on the render clock, wrapping from the left.
            None => fill
                .w_frac(0.25)
                .looping(LoopSpec::slide(1.4, [-1.0, 0.0], [4.0, 0.0])),
        });
    col()
        .gap(14.0)
        .child(heading(p, &l.title, &l.step))
        .child(track)
        .child(label(
            "Files the preprocessor or parser can't read are left out and listed in the explorer, under the file count.",
            p.faint,
        )
        .wrap())
        .child(actions(
            el(),
            vec![
                button(p, "Cancel", false)
                    .help("Stop opening (Esc)")
                    .on_press(project::cancel),
            ],
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toolbar_only_opens_projects() {
        let titles: Vec<&str> = TOOLBAR.iter().map(|(t, _, _)| *t).collect();
        assert_eq!(titles, ["Open…"]);
        let mut s = State::load("fixture");
        (TOOLBAR[0].2)(&mut s);
        assert_eq!(s.modal, Some(Modal::Open));
    }

    #[test]
    fn the_expanded_error_list_fits_the_window() {
        let collapsed = ERROR_LINES as f32 * ERROR_LINE_H;
        // A 640 pt window has no room to spare: the list scrolls at its collapsed height.
        assert_eq!(error_list_room(640.0), collapsed);
        assert_eq!(error_list_room(900.0), ERROR_LIST_MAX_H);
        assert!(error_list_room(700.0) > collapsed && error_list_room(700.0) < ERROR_LIST_MAX_H);
    }
}
