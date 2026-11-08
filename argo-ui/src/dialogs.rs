//! Project chrome: the toolbar, the file/function explorer, and the open, progress and rewrite
//! dialogs. These navigate and request work; the canvas itself stays read-only.

use rune_ui::{Color, Cursor, El, ScrollSpec, TextStyle, col, el, row, text, text_input};

use crate::app::State;
use crate::palette::Pal;
use crate::project::{self, Modal, Scope};
use crate::ui::{EXPLORER_W, H, LABEL, M, UI, UI_STRONG, W, card, label};

const TITLE: TextStyle = TextStyle::new(17.0, 650.0);
const BODY: TextStyle = TextStyle::new(12.5, 450.0);
const FIELD: TextStyle = TextStyle::new(12.5, 450.0);
const DIALOG_W: f32 = 520.0;
/// Rows the explorer lists at most while filtering.
const FILTER_LIMIT: usize = 300;

fn button(p: &Pal, title: &str, primary: bool) -> El {
    let (fill, fg, hover) = if primary {
        (p.accent, Color::WHITE, p.accent.mix(Color::WHITE, 0.12))
    } else {
        (p.code_bg, p.text, p.code_bg.mix(p.text, 0.08))
    };
    row()
        .h(32.0)
        .px(14.0)
        .radius(8.0)
        .fill(fill)
        .hover(hover)
        .items_center()
        .cursor(Cursor::Pointer)
        .border(1.0, if primary { fill } else { p.panel_border })
        .child(
            text(title.to_string())
                .font(UI_STRONG)
                .color(fg)
                .single_line(),
        )
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

/// Top-right actions beside the status card: open a project, and (with one open) start a rewrite.
pub fn toolbar(p: &Pal, s: &State) -> El {
    let mut r = row().h(40.0).px(6.0).gap(6.0).items_center().child(
        button(p, "Open…", false)
            .help("Open a git repository or local directory (Ctrl+O)")
            .on_press(|s: &mut State| s.modal = Some(Modal::Open)),
    );
    if s.project.is_some() {
        r = r.child(
            button(p, "Rewrite in Rust…", true)
                .help("Start a Rust rewrite of this function, file or project")
                .on_press(|s: &mut State| s.modal = Some(Modal::Rewrite)),
        );
    }
    card(p).child(r)
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

/// The open dialog (or progress, rewrite and not-implemented), over a dimmed canvas.
pub fn modal(p: &Pal, s: &State) -> El {
    let Some(which) = s.modal else { return el() };
    let dialog = match which {
        Modal::Open => open_dialog(p, s),
        Modal::Loading => loading_dialog(p, s),
        Modal::Rewrite => rewrite_dialog(p, s),
        Modal::NotImplemented => not_implemented(p),
    };
    let dismiss = |s: &mut State| {
        // A running load can't be cancelled midway; its dialog stays until it finishes.
        if s.modal != Some(Modal::Loading) {
            s.modal = None;
        }
    };
    el().size(W, H)
        .fill(Color::BLACK.alpha(if p.dark { 0.5 } else { 0.25 }))
        .center()
        .child(
            card(p)
                .w(DIALOG_W)
                .padding(22.0)
                .gap(16.0)
                .child(dialog)
                .modal(true, dismiss),
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
        c = c.child(
            row()
                .padding(10.0)
                .radius(8.0)
                .fill(p.bad.alpha(0.12))
                .child(text(e.clone()).font(BODY).color(p.bad).wrap()),
        );
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

fn loading_dialog(p: &Pal, s: &State) -> El {
    let Some(l) = &s.loader else {
        return heading(p, "Opening", "");
    };
    let mut c = col().gap(14.0).child(heading(p, &l.title, &l.step));
    let frac = l.progress.map_or(0.0, |(d, t)| d as f32 / t.max(1) as f32);
    let bar_w = DIALOG_W - 44.0;
    let bar = el().w(bar_w).h(6.0).radius(3.0).fill(p.code_bg);
    c = c.child(if l.progress.is_some() {
        bar.child(el().w(bar_w * frac).h(6.0).radius(3.0).fill(p.accent))
    } else {
        bar.child(
            el().w(bar_w * 0.3)
                .h(6.0)
                .radius(3.0)
                .fill(p.accent.alpha(0.6)),
        )
    });
    c.child(label(
        "Files that the preprocessor or parser can't read are left out and listed in the explorer.",
        p.faint,
    ))
}

fn rewrite_dialog(p: &Pal, s: &State) -> El {
    let Some(proj) = &s.project else {
        return not_implemented(p);
    };
    let current = project::current(s);
    let module = current.as_ref().and_then(|(id, _)| proj.module_of(*id));
    let option = |scope: Scope, title: String, detail: String| {
        let on = s.forms.scope == scope;
        row()
            .padding(12.0)
            .gap(12.0)
            .radius(10.0)
            .items_center()
            .cursor(Cursor::Pointer)
            .fill(if on { p.accent.alpha(0.12) } else { p.code_bg })
            .border(
                if on { 1.5 } else { 1.0 },
                if on { p.accent } else { p.panel_border },
            )
            .child(
                el().size(16.0, 16.0)
                    .radius(8.0)
                    .border(1.5, if on { p.accent } else { p.faint })
                    .center()
                    .child(if on {
                        el().size(8.0, 8.0).radius(4.0).fill(p.accent)
                    } else {
                        el()
                    }),
            )
            .child(
                col()
                    .gap(3.0)
                    .grow(1.0)
                    .min_w(0.0)
                    .child(
                        text(title)
                            .font(UI_STRONG)
                            .color(p.text)
                            .single_line()
                            .truncate(),
                    )
                    .child(
                        text(detail)
                            .font(UI)
                            .color(p.muted)
                            .single_line()
                            .truncate(),
                    ),
            )
            .on_press(move |s: &mut State| s.forms.scope = scope)
    };
    let mut scopes = col().gap(8.0);
    if let Some((_, name)) = &current {
        scopes = scopes.child(option(
            Scope::Function,
            "This function".into(),
            format!("{name}()"),
        ));
    }
    if let Some(m) = module {
        scopes = scopes.child(option(
            Scope::File,
            "This file".into(),
            format!("{} · {} functions", m.name, m.functions.len()),
        ));
    }
    scopes = scopes.child(option(
        Scope::Project,
        "Whole project".into(),
        format!(
            "{} · {} functions in {} files",
            proj.name,
            proj.function_count(),
            proj.included()
        ),
    ));
    col().gap(16.0)
        .child(heading(p, "Rewrite in Rust", "Agents translate the chosen C into Rust, one work unit at a time; each candidate is checked against the original C before it replaces it."))
        .child(col().gap(6.0).child(field_label(p, "Scope")).child(scopes))
        .child(actions(el(), vec![
            button(p, "Cancel", false).on_press(|s: &mut State| s.modal = None),
            button(p, "Start rewrite", true).default_button().on_press(|s: &mut State| s.modal = Some(Modal::NotImplemented)),
        ]))
}

fn not_implemented(p: &Pal) -> El {
    col().gap(16.0)
        .child(row().gap(12.0).items_center()
            .child(el().size(34.0, 34.0).radius(17.0).fill(p.accent.alpha(0.16)).center()
                .child(text("i").font(TextStyle::new(16.0, 750.0)).color(p.accent)))
            .child(text("Not yet implemented").font(TITLE).color(p.text)))
        .child(text("Starting a Rust rewrite from the app isn't built yet: there are no translation workers or validation behind this button. Nothing was changed.".to_string()).font(BODY).color(p.muted).wrap())
        .child(actions(el(), vec![button(p, "OK", true).default_button().on_press(|s: &mut State| s.modal = None)]))
}
