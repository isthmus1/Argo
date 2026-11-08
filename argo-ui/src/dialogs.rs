//! Project chrome: the toolbar, the file/function explorer, and the open and progress dialogs.
//! These open projects and navigate them; nothing here edits the graph or starts work.

use rune_ui::{
    Color, Cursor, El, FileDialogOptions, LoopSpec, ScrollSpec, Span, TextEdit, TextStyle, Tx, col, el, row,
    text, text_input,
};

use crate::app::State;
use crate::palette::Pal;
use crate::project::{self, Modal};
use crate::ui::{CONTROL_H, CONTROL_R, LABEL, M, MONO, TOP, UI, UI_STRONG, bar, card, explorer_w, label, viewport};

const TITLE: TextStyle = TextStyle::new(17.0, 650.0);
const BODY: TextStyle = TextStyle::new(12.5, 450.0);
const FIELD: TextStyle = TextStyle::new(12.5, 450.0);
const DIALOG_W: f32 = 520.0;
/// Padding inside a dialog's card.
const DIALOG_PAD: f32 = 22.0;
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

/// A field's placeholder, led by an en space: Rune draws a focused empty field's caret at the
/// placeholder's first glyph, which makes the field read as already filled. (A thin or word
/// space still leaves the 1.5 pt caret touching the glyph.)
fn hint(s: &str) -> String {
    format!("\u{2002}{s}")
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
        proj.root.display(),
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
        if on {
            r = r.fill(p.accent.alpha(0.16));
        }
        r.on_press(move |s: &mut State| project::open_function(s, id))
    };
    if empty {
        // Parsed, but nothing defines a function: say what it holds (the canvas shows those
        // declarations), and offer another project.
        list = list.child(
            col()
                .gap(10.0)
                .items_start()
                .child(label(&proj.empty_note(), p.muted).wrap().key("explorer-empty"))
                .child(
                    button(p, "Open another…", false)
                        .key("explorer-open-another")
                        .on_press(|s: &mut State| s.modal = Some(Modal::Open)),
                ),
        );
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
    // Centred in the band below the chrome bars, so a tall dialog never covers them.
    el().w_full().h_full()
        .pt(TOP)
        .pb(M)
        .fill(Color::BLACK.alpha(if p.dark { 0.5 } else { 0.25 }))
        .center()
        .child(
            card(p)
                .w(DIALOG_W.min(viewport()[0] - 2.0 * M))
                .padding(DIALOG_PAD)
                .gap(16.0)
                .child(dialog)
                .modal(which != Modal::Loading, dismiss),
        )
}

/// A dialog's title, and a line under it unless `sub` is empty.
fn heading(p: &Pal, title: &str, sub: &str) -> El {
    let c = col()
        .gap(6.0)
        .child(text(title.to_string()).font(TITLE).color(p.text));
    if sub.is_empty() {
        c
    } else {
        c.child(text(sub.to_string()).font(BODY).color(p.muted).wrap())
    }
}

fn actions(left: El, right: Vec<El>) -> El {
    let mut r = row().gap(8.0).items_center().child(left.grow(1.0));
    for b in right {
        r = r.child(b);
    }
    r
}

/// The open dialog's folder picker button.
const BROWSE: &str = "Browse…";
/// What the revision field says while the repository field names a local folder.
const REV_LOCAL: &str = "Only for git repositories";

/// Pick a local folder with the system's dialog and put it in the repository field.
fn browse(_: &mut State, tx: &mut Tx) {
    let options = FileDialogOptions {
        title: "Open C project folder".into(),
        ..Default::default()
    };
    tx.open_folder(options, |s: &mut State, result, _| match result {
        Ok(Some(path)) => {
            s.forms.url = TextEdit::new(path.display().to_string());
            s.forms.set_error(None);
        }
        Ok(None) => {}
        Err(e) => s.forms.set_error(Some(format!("Can't show a folder picker: {e}"))),
    });
}

/// The branch, tag or commit field; muted and inert while the repository field names a local
/// folder, which has no revisions to pick.
fn rev_field(p: &Pal, rev: &TextEdit, local: bool) -> El {
    if local {
        return col()
            .gap(6.0)
            .key("open-rev-local")
            .help(REV_LOCAL)
            .child(text("Branch, tag or commit".to_uppercase()).font(LABEL).color(p.muted.alpha(0.45)))
            .child(
                el().h(34.0)
                    .px(10.0)
                    .radius(8.0)
                    .border(1.0, p.panel_border)
                    .items_center()
                    .disabled(true)
                    .child(text(hint(REV_LOCAL)).font(FIELD).color(p.faint).single_line().truncate()),
            );
    }
    col().gap(6.0).child(field_label(p, "Branch, tag or commit"))
        .child(field(p, text_input(rev, FIELD).single_line()
            .placeholder(hint("default branch"), p.faint)
            .on_edit(|s: &mut State| &mut s.forms.rev)
            .on_submit(|s: &mut State, _| project::start(s))))
}

fn open_dialog(p: &Pal, s: &State) -> El {
    let f = &s.forms;
    // Once a load has failed the error says what matters; the explanation gives it the room.
    let sub = if f.error.is_some() {
        ""
    } else {
        "Clone a git repository into a temporary folder, or open a local directory. Every .c file is run through the C preprocessor and combined into one graph."
    };
    let url = field(p, text_input(&f.url, FIELD).single_line()
        .placeholder(hint("https://github.com/user/project.git   or   ~/src/project"), p.faint)
        .on_edit(|s: &mut State| &mut s.forms.url)
        .on_submit(|s: &mut State, _| project::start(s)));
    let mut c = col().gap(16.0)
        .child(heading(p, "Open a C project", sub))
        .child(col().gap(6.0).child(field_label(p, "Repository or folder"))
            .child(row().gap(8.0).items_center()
                .child(url.grow(1.0).min_w(0.0))
                .child(button(p, BROWSE, false).key("open-browse").on_press_tx(browse))))
        .child(rev_field(p, &f.rev, project::path_shaped(&f.url.text)))
        .child(col().gap(6.0).child(field_label(p, "Preprocessor flags"))
            .child(field(p, text_input(&f.flags, FIELD).single_line()
                .placeholder(hint("-DNAME=value -Iinclude"), p.faint)
                .on_edit(|s: &mut State| &mut s.forms.flags)
                .on_submit(|s: &mut State, _| project::start(s)))
                .help("Added after any -D/-I flags found in the project's Makefile")));
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

/// Lines of a load error shown before `Show all`, when the window has room for them.
const ERROR_LINES: usize = 6;
/// The fewest error lines shown: on a shorter window the list scrolls from here.
const ERROR_LINES_MIN: usize = 2;
/// Height of one error line, and the most the expanded list takes before it scrolls.
const ERROR_LINE_H: f32 = 18.0;
const ERROR_LIST_MAX_H: f32 = 200.0;
/// The open dialog's height with an error but without its file lines (measured from a 1440x900
/// capture of `project-open-error`: 540 pt with six lines, plus its border), so the list never
/// pushes the dialog over the chrome bars.
const OPEN_DIALOG_REST_H: f32 = 434.0;

/// The band a dialog is centred in: below the chrome bars (`TOP`), `M` above the window's bottom.
fn modal_band(window_h: f32) -> f32 {
    window_h - TOP - M
}

/// How tall the error list may grow before it scrolls: whole lines, as many as the band below
/// the chrome bars has room for, between `ERROR_LINES_MIN` lines and `ERROR_LIST_MAX_H`.
fn error_list_room(window_h: f32) -> f32 {
    let fit = ((modal_band(window_h) - OPEN_DIALOG_REST_H) / ERROR_LINE_H).floor() * ERROR_LINE_H;
    fit.clamp(ERROR_LINES_MIN as f32 * ERROR_LINE_H, ERROR_LIST_MAX_H)
}

/// Error lines listed before `Show all`: `ERROR_LINES`, or fewer when the window is short.
fn error_lines_shown(window_h: f32) -> usize {
    ERROR_LINES.min((error_list_room(window_h) / ERROR_LINE_H) as usize)
}

/// Padding inside the error panel.
const ERROR_PAD: f32 = 10.0;
/// Gap between an error line's location and its reason.
const ERROR_GAP: f32 = 8.0;

/// The width an error line's reason wraps at in a `window_w` window: the dialog's inner width
/// less the panel's padding, its location and the gap after it. A point short of it on each side
/// for the card's border, so a line never wraps more than measured.
fn error_reason_w(window_w: f32, at: &str) -> f32 {
    let inner = DIALOG_W.min(window_w - 2.0 * M) - 2.0 * DIALOG_PAD - 2.0 * ERROR_PAD - 2.0;
    let at_w = if at.is_empty() {
        0.0
    } else {
        rune_ui::text::measure(at, &[], MONO, None).0 + ERROR_GAP
    };
    (inner - at_w).max(1.0)
}

/// The height of one line of error text.
fn error_text_line_h() -> f32 {
    rune_ui::text::measure("x", &[], MONO, None).1
}

/// How many `ERROR_LINE_H` rows an error line takes in a `window_w` window, its reason wrapped
/// as [`error_line`] wraps it.
fn error_line_rows(window_w: f32, line: &str) -> usize {
    let (at, why) = project::error_row(line);
    let wrap = rune_ui::text::RichLayoutOptions::new().wrap_anywhere();
    let h = rune_ui::text::measure_with(why, &[], MONO, Some(error_reason_w(window_w, at)), wrap).1;
    ((h / error_text_line_h()).round() as usize).max(1)
}

/// How many of the leading lines (`rows` each) fit `room_rows`: at least one, at most `cap`.
fn lines_that_fit(rows: &[usize], room_rows: usize, cap: usize) -> usize {
    let mut used = 0;
    let n = rows
        .iter()
        .take_while(|&&r| {
            used += r;
            used <= room_rows
        })
        .count();
    n.clamp(1, cap.max(1)).min(rows.len())
}

/// One file of a load error: its `path:line`, then the reason, wrapped (at a space or after a
/// `/`, inside a word only when it can't fit a line) so a long header path shows whole. `rows`
/// rows tall, the location beside the first; the whole line is on hover.
fn error_line(p: &Pal, line: &str, rows: usize) -> El {
    let (at, why) = project::error_row(line);
    let mut r = row()
        .h(rows as f32 * ERROR_LINE_H)
        .pt(((ERROR_LINE_H - error_text_line_h()) / 2.0).max(0.0))
        .gap(ERROR_GAP)
        .items_start()
        .help(line.to_string());
    if !at.is_empty() {
        r = r.child(text(at.to_string()).font(MONO).color(p.muted).single_line());
    }
    r.child(text(why.to_string()).font(MONO).color(p.text).wrap_anywhere().grow(1.0).min_w(0.0))
}

/// A load error: its headline, then the first files with their reasons (wrapped, the whole line
/// on hover). `Show all` lists the rest in a scrolling list; `Copy diagnostics` puts the whole
/// text on the clipboard.
fn error_box(p: &Pal, e: &str, open: bool) -> El {
    let [window_w, window_h] = viewport();
    let (head, all, _) = project::error_parts(e, usize::MAX);
    let rows: Vec<usize> = all.iter().map(|l| error_line_rows(window_w, l)).collect();
    let collapsed = lines_that_fit(&rows, error_lines_shown(window_h), ERROR_LINES);
    let shown = if open { all.len() } else { collapsed };
    let total = all.len();
    let mut c = col()
        .gap(6.0)
        .padding(ERROR_PAD)
        .radius(8.0)
        .fill(p.bad.alpha(0.12))
        .child(text(head.to_string()).font(BODY).color(p.bad).wrap());
    if !all.is_empty() {
        let mut list = col();
        for (line, &n) in all.iter().zip(&rows).take(shown) {
            list = list.child(error_line(p, line, n));
        }
        let list_h = rows[..shown].iter().sum::<usize>() as f32 * ERROR_LINE_H;
        let room = error_list_room(window_h);
        c = c.child(if list_h > room {
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
    let toggle = if total > collapsed {
        let title = if open {
            "Show fewer".to_string()
        } else {
            format!("Show all {total}")
        };
        // The panel's text colour, underlined: the accent is too faint on the error's red.
        let link = vec![Span::new(0, title.len()).underline(p.text)];
        row()
            .h(CONTROL_H)
            .items_center()
            .cursor(Cursor::Pointer)
            .key("error-toggle")
            .child(text(title).font(UI_STRONG).color(p.text).spans(link))
            .on_press(|s: &mut State| s.forms.error_open = !s.forms.error_open)
    } else {
        el()
    };
    c.child(actions(
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
    use rune_ui::{Env, Runtime, TextEdit, accesskit};

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
        // Centred in the band below the chrome bars, the tallest dialog's top stays at or
        // under `TOP` (clear of the breadcrumb and toolbar) at 1024x640 and up.
        for h in [640.0, 700.0, 900.0, 1440.0] {
            let dialog_h = OPEN_DIALOG_REST_H + error_list_room(h);
            let top = TOP + (modal_band(h) - dialog_h) / 2.0;
            assert!(top >= TOP, "at {h} pt the dialog's top is {top}");
        }
        // A shorter window lists fewer lines before `Show all`, and scrolls from fewer still.
        assert_eq!(error_lines_shown(640.0), ERROR_LINES);
        assert!(error_lines_shown(540.0) < ERROR_LINES);
        assert!(error_list_room(400.0) >= ERROR_LINES_MIN as f32 * ERROR_LINE_H);
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
            root: "/tmp/settings".into(),
            units: vec![argo_c::project::Unit {
                path: "config.c".into(),
                skipped: None,
                flags: vec![],
            }],
            flags: vec![],
            modules: vec![],
            declarations: 2,
        }
    }

    #[test]
    fn browse_puts_the_picked_folder_in_the_field_and_mutes_the_revision() {
        let mut s = State::load("fixture");
        s.modal = Some(Modal::Open);
        s.forms.url = TextEdit::new("https://github.com/a/b.git");
        let mut rt = render(s);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains("REPOSITORY OR FOLDER") && tree.contains(BROWSE), "{tree}");
        assert!(!tree.contains(REV_LOCAL), "a remote has revisions");

        assert!(rt.accessibility_action("open-browse", accesskit::Action::Click, None));
        rt.frame(2.0 / 60.0);
        let (kind, options) = rt.file_dialog().expect("the folder picker shows");
        assert_eq!(kind, rune_ui::FileDialogKind::Folder);
        assert_eq!(options.title, "Open C project folder");
        let dir = std::env::temp_dir();
        rt.resolve_file_dialog(Some(dir.clone())).unwrap();
        rt.frame(3.0 / 60.0);
        assert_eq!(rt.state.forms.url.text, dir.display().to_string());

        // A folder has no branches: the revision field says so and takes no input.
        rt.frame(4.0 / 60.0);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains(REV_LOCAL), "{tree}");

        // A cancelled picker leaves the field alone.
        assert!(rt.accessibility_action("open-browse", accesskit::Action::Click, None));
        rt.frame(5.0 / 60.0);
        rt.resolve_file_dialog(None).unwrap();
        rt.frame(6.0 / 60.0);
        assert_eq!(rt.state.forms.url.text, dir.display().to_string());
    }

    #[test]
    fn a_git_failure_leads_with_its_cause_and_lists_git_s_output() {
        // What opening a missing GitHub repository reports.
        let e = "git fetch: repository 'https://github.com/a/missing.git/' not found\nremote: Repository not found.\nfatal: repository 'https://github.com/a/missing.git/' not found";
        let mut s = State::load("fixture");
        s.modal = Some(Modal::Open);
        s.forms.error = Some(e.into());
        let rt = render(s);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains("git fetch: repository 'https://github.com/a/missing.git/' not found"), "{tree}");
        assert!(tree.contains("Repository not found."), "{tree}");
        assert!(tree.contains("Copy diagnostics"));
    }

    #[test]
    fn a_project_without_functions_shows_the_empty_state() {
        let mut s = State::load("fixture");
        s.project = Some(empty_project());
        let mut rt = render(s);
        let tree = rt.accessibility_json().to_string();
        // It says what the project does hold, and there is nothing to filter.
        assert!(tree.contains("2 declarations, no function bodies"), "{tree}");
        assert!(tree.contains("Open another…"));
        assert!(!tree.contains("0 functions") && !tree.contains("Filter functions"), "{tree}");

        // Its one action opens the open dialog.
        assert!(rt.accessibility_action("explorer-open-another", accesskit::Action::Click, None));
        rt.frame(2.0 / 60.0);
        assert_eq!(rt.state.modal, Some(Modal::Open));

        // A filter typed for an earlier project doesn't hide what this one holds.
        rt.update(|s, _| {
            s.modal = None;
            s.forms.filter = TextEdit::new("x");
        });
        rt.frame(3.0 / 60.0);
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

    #[test]
    fn collapsed_error_lines_fill_whole_rows() {
        // Six one-row lines fit six rows; a two-row line that would overrun waits for `Show all`.
        assert_eq!(lines_that_fit(&[1; 9], 6, ERROR_LINES), 6);
        assert_eq!(lines_that_fit(&[1, 1, 1, 1, 1, 2, 1, 1, 1], 6, ERROR_LINES), 5);
        assert_eq!(lines_that_fit(&[1, 2, 1, 1, 1, 1], 6, ERROR_LINES), 5);
        // Never more than the cap or the lines there are, and at least one even if it scrolls.
        assert_eq!(lines_that_fit(&[1; 9], 20, ERROR_LINES), ERROR_LINES);
        assert_eq!(lines_that_fit(&[1, 1], 6, ERROR_LINES), 2);
        assert_eq!(lines_that_fit(&[8, 1], 6, ERROR_LINES), 1);
        assert_eq!(lines_that_fit(&[], 6, ERROR_LINES), 0);
    }

    #[test]
    fn a_missing_header_s_path_wraps_after_a_slash_instead_of_being_cut() {
        // A header path with no spaces still has somewhere to break: after each `/`, so it wraps
        // within the reason's width rather than running past it.
        let path = "argo_absent/deeply/nested/include/directory/slirp_config.h";
        let w = 200.0;
        let one = error_text_line_h();
        let (pw, ph) = rune_ui::text::measure(path, &[], MONO, Some(w));
        assert!(pw <= w && ph > 1.5 * one, "{pw} x {ph}");
        // It is the slashes: the same name without them has nowhere to break.
        let flat = path.replace('/', "_");
        assert!(rune_ui::text::measure(&flat, &[], MONO, Some(w)).0 > w);
        // In the sample at 1024x640, the slirp line takes two rows with its path whole on them.
        let (_, files, _) = project::error_parts(project::SAMPLE_ERROR, usize::MAX);
        let slirp = files.iter().find(|l| l.starts_with("slirp/")).unwrap();
        assert_eq!(error_line_rows(1024.0, slirp), 2);
        assert_eq!(error_line_rows(1024.0, "jsemu.c: emscripten.h: No such file or directory (jsemu.c:1)"), 1);
    }

    #[test]
    fn the_error_toggle_lists_every_line_and_back() {
        let mut s = State::load("fixture");
        s.modal = Some(Modal::Open);
        s.forms.error = Some(project::SAMPLE_ERROR.into());
        let mut rt = render(s);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains("Show all 9"), "{tree}");
        assert!(rt.accessibility_action("error-toggle", accesskit::Action::Click, None));
        rt.frame(2.0 / 60.0);
        assert!(rt.state.forms.error_open);
        let tree = rt.accessibility_json().to_string();
        assert!(tree.contains("Show fewer"), "{tree}");
    }
}
