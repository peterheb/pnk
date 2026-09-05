//! Phase 5 — fallback dumpers: readable plain-text and markdown renderings
//! of a converted document. These are the shippable safety net: readable
//! output for every fixture without the viewer. Style information appears
//! only where it aids reading (bold/italic markers).

use crate::model::*;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn to_text(doc: &PnkDocument) -> String {
    let mut out = String::new();
    match doc {
        PnkDocument::Keynote(d) => keynote_text(d, &mut out),
        PnkDocument::Pages(d) => pages_text(d, &mut out),
        PnkDocument::Numbers(d) => numbers_text(d, &mut out),
    }
    out
}

pub fn to_markdown(doc: &PnkDocument) -> String {
    let mut out = String::new();
    match doc {
        PnkDocument::Keynote(d) => keynote_md(d, &mut out),
        PnkDocument::Pages(d) => pages_md(d, &mut out),
        PnkDocument::Numbers(d) => numbers_md(d, &mut out),
    }
    out
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

pub(crate) fn para_plain(p: &Paragraph) -> String {
    para_line(p, false)
}

/// One paragraph as a line. With `math`, inline LaTeX equations go out as
/// `$...$` spans (markdown); without, as their bare source (text).
fn para_line(p: &Paragraph, math: bool) -> String {
    let mut s = String::new();
    for item in &p.items {
        match item {
            ParagraphItem::Plain(text) | ParagraphItem::Text { text, .. } => {
                // Soft breaks render as spaces in the flat dumpers (the
                // paragraph block is one line); raw LS/PS would confuse
                // line-oriented tooling (gotchas #15).
                s.push_str(&text.replace(['\u{2028}', '\u{2029}'], " "))
            }
            // An inline equation image carries its source expression; any
            // other attached object is a placeholder space.
            ParagraphItem::InlineObject {
                drawable:
                    Drawable::Image {
                        equation: Some(eq), ..
                    },
                ..
            } => {
                if math {
                    s.push_str(&equation_markdown(eq));
                } else {
                    s.push_str(&eq.source);
                }
            }
            ParagraphItem::InlineObject { .. } => s.push(' '), // object placeholder
            ParagraphItem::Field { value, field, .. } => match (value, field) {
                (Some(v), _) => s.push_str(v),
                (None, FieldKind::PageNumber) => s.push('1'),
                (None, FieldKind::PageCount) => s.push('1'),
                _ => {}
            },
        }
    }
    s.trim_end().to_string()
}

/// Run text with markdown emphasis where the style says so (bold/italic only).
fn para_markdown(p: &Paragraph, char_styles: &[CharStyle]) -> String {
    let mut s = String::new();
    for item in &p.items {
        match item {
            ParagraphItem::Plain(text) => {
                s.push_str(&escape_html(&text.replace('\t', "    ")));
            }
            ParagraphItem::Text { text, c_style, .. } => {
                let style = c_style.and_then(|i| char_styles.get(i as usize));
                // Soft break → markdown hard line break inside the paragraph.
                let text = if text.contains('\u{2028}') || text.contains('\u{2029}') {
                    text.replace(['\u{2028}', '\u{2029}'], "  \n")
                } else {
                    text.clone()
                };
                let t = escape_html(&text.replace('\t', "    "));
                let bold = style.and_then(|s| s.bold) == Some(true);
                let italic = style.and_then(|s| s.italic) == Some(true);
                if t.trim().is_empty() {
                    s.push_str(&t);
                } else if bold && italic {
                    s.push_str(&format!("***{t}***"));
                } else if bold {
                    s.push_str(&format!("**{t}**"));
                } else if italic {
                    s.push_str(&format!("*{t}*"));
                } else {
                    s.push_str(&t);
                }
            }
            ParagraphItem::InlineObject {
                drawable:
                    Drawable::Image {
                        equation: Some(eq), ..
                    },
                ..
            } => s.push_str(&equation_markdown(eq)),
            ParagraphItem::InlineObject { .. } => s.push(' '),
            ParagraphItem::Field { value, .. } => {
                if let Some(v) = value {
                    s.push_str(&escape_html(v));
                }
            }
        }
    }
    s.trim_end().to_string()
}

/// LaTeX sources go out as `$...$` math spans, unescaped: math-aware
/// renderers read the span raw, and `<` is common in LaTeX. MathML markup
/// goes out as a code span.
fn equation_markdown(eq: &EquationInfo) -> String {
    match eq.format {
        EquationFormat::Latex => format!("${}$", eq.source.trim()),
        EquationFormat::Mathml => format!("`{}`", eq.source.replace('`', "'")),
    }
}

/// A drawable text for markdown: equations as display math, others as is.
fn md_text_block(kind: &str, t: &str) -> String {
    if kind == "equation" {
        format!("$$\n{}\n$$", t.trim())
    } else {
        t.to_string()
    }
}

/// A canvas-level equation as a display block: `$$...$$` on its own lines.
fn equation_display_markdown(eq: &EquationInfo) -> String {
    match eq.format {
        EquationFormat::Latex => format!("$$\n{}\n$$", eq.source.trim()),
        EquationFormat::Mathml => format!("`{}`", eq.source.replace('`', "'")),
    }
}

pub(crate) fn styled_plain(st: &StyledText) -> String {
    st.paragraphs
        .iter()
        .map(para_plain)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

fn escape_md(s: &str) -> String {
    escape_html(s).replace('|', "\\|").replace('\n', " ")
}

/// Neutralize raw HTML so document text can never smuggle `<script>` (or any
/// tag) into a Markdown consumer that renders inline HTML (FINDINGS.md
/// M-13). Markdown punctuation stays as-is — the output is documented as
/// generated from untrusted documents, and `&`/`<` are the HTML injection
/// points.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
}

fn fmt_num(n: f64) -> String {
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{n}");
        s
    }
}

fn drawable_texts(d: &Drawable, out: &mut Vec<(String, String)>) {
    // (kind, text) pairs, in paint order; recurse into groups.
    match d {
        Drawable::Textbox { text, .. } => {
            let t = styled_plain(text);
            if !t.is_empty() {
                out.push(("textbox".into(), t));
            }
        }
        Drawable::Shape { text: Some(t), .. } => {
            let s = styled_plain(t);
            if !s.is_empty() {
                out.push(("shape-text".into(), s));
            }
        }
        Drawable::Group { children, .. } => {
            for c in children {
                drawable_texts(c, out);
            }
        }
        Drawable::Image {
            equation: Some(eq), ..
        } => out.push(("equation".into(), eq.source.clone())),
        Drawable::Image { .. } => out.push(("image".into(), String::new())),
        Drawable::Table { table, .. } => {
            out.push((
                "table".into(),
                format!(
                    "[table: {}]",
                    table.name.clone().unwrap_or_else(|| "unnamed".into())
                ),
            ));
        }
        Drawable::Chart { chart, .. } => {
            out.push(("chart".into(), format!("[chart: {:?}]", chart.r#type)));
        }
        _ => {}
    }
}

fn warnings_block(doc_warnings: &[Warning], out: &mut String, markdown: bool) {
    if doc_warnings.is_empty() {
        return;
    }
    if markdown {
        out.push_str(&format!(
            "\n> **Warnings ({}):** {} conversion note(s); first: {}\n",
            doc_warnings.len(),
            doc_warnings.len(),
            doc_warnings[0].message
        ));
    } else {
        out.push_str(&format!("\nWarnings: {} recorded.\n", doc_warnings.len()));
    }
}

// ---------------------------------------------------------------------------
// Keynote
// ---------------------------------------------------------------------------

fn slide_title_and_bullets(
    slide: &Slide,
    para_styles: &[ParaStyle],
    markdown: bool,
) -> (Option<String>, Vec<String>) {
    // The title is the converter's `Slide.title` (the first title placeholder
    // with text); the dumpers print its first line and skip that drawable
    // when collecting bullets.
    let title = slide
        .title
        .as_deref()
        .and_then(|t| t.lines().next())
        .map(str::to_string);
    let mut title_seen = false;
    let mut bullets = Vec::new();
    for d in &slide.drawables {
        let role = match d {
            Drawable::Shape { common, .. } | Drawable::Textbox { common, .. } => {
                common.placeholder.as_ref().map(|p| p.role.clone())
            }
            _ => None,
        };
        let text = match d {
            Drawable::Textbox { text, .. } => Some(text),
            Drawable::Shape { text: Some(t), .. } => Some(t),
            // A canvas-level equation image is content: its source is the line.
            Drawable::Image {
                equation: Some(eq), ..
            } => {
                bullets.push(if markdown {
                    equation_display_markdown(eq)
                } else {
                    eq.source.trim().to_string()
                });
                None
            }
            _ => None,
        };
        let Some(text) = text else { continue };
        if role.as_deref() == Some("title")
            && title.is_some()
            && !title_seen
            && !styled_plain(text).is_empty()
        {
            title_seen = true;
            continue;
        }
        for p in &text.paragraphs {
            let line = para_line(p, markdown);
            if line.is_empty() {
                continue;
            }
            // List markers render as bullets; plain paragraphs as lines.
            let is_list = p
                .p_style
                .and_then(|i| para_styles.get(i as usize))
                .map(|s| s.list.is_some())
                .unwrap_or(false);
            if is_list {
                bullets.push(format!("- {line}"));
            } else {
                bullets.push(line);
            }
        }
    }
    (title, bullets)
}

fn keynote_text(d: &KeynoteDocument, out: &mut String) {
    out.push_str(&format!("Keynote show — {} slides\n", d.slides.len()));
    if let Some(name) = &d.theme_name {
        out.push_str(&format!("Theme: {name}\n"));
    }
    for (i, slide) in d.slides.iter().enumerate() {
        out.push_str(&format!("\n--- Slide {} ---\n", i + 1));
        if slide.skipped == Some(true) {
            out.push_str("[skipped]\n");
        }
        let (title, bullets) = slide_title_and_bullets(slide, &d.styles.para, false);
        if let Some(t) = &title {
            out.push_str(&format!("Title: {t}\n"));
        }
        for b in &bullets {
            out.push_str(&format!("{b}\n"));
        }
        if let Some(notes) = &slide.notes {
            let n = styled_plain(notes);
            if !n.is_empty() {
                out.push_str(&format!("Notes: {n}\n"));
            }
        }
    }
    warnings_block(&d.warnings, out, false);
}

fn keynote_md(d: &KeynoteDocument, out: &mut String) {
    out.push_str("# Keynote Show\n\n");
    if let Some(name) = &d.theme_name {
        out.push_str(&format!("*Theme: {name} — {} slides*\n\n", d.slides.len()));
    }
    for (i, slide) in d.slides.iter().enumerate() {
        let (title, bullets) = slide_title_and_bullets(slide, &d.styles.para, true);
        let heading = title.clone().unwrap_or_else(|| format!("Slide {}", i + 1));
        out.push_str(&format!("## {heading}\n\n"));
        if slide.skipped == Some(true) {
            out.push_str("*(skipped slide)*\n\n");
        }
        for b in &bullets {
            out.push_str(&format!("{b}\n"));
        }
        if !bullets.is_empty() {
            out.push('\n');
        }
        if let Some(notes) = &slide.notes {
            let n = styled_plain(notes);
            if !n.is_empty() {
                out.push_str(&format!("> Notes: {n}\n\n"));
            }
        }
    }
    warnings_block(&d.warnings, out, true);
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

fn pages_body_md(body: &StyledText, styles: &StylePools, out: &mut String) {
    // Tiered list labels need the numbers of the enclosing levels.
    let mut level_numbers: Vec<u32> = Vec::new();
    for p in &body.paragraphs {
        let text = para_markdown(p, &styles.char);
        if text.is_empty() {
            continue;
        }
        let style = p.p_style.and_then(|i| styles.para.get(i as usize));
        let level = style.and_then(|s| s.outline_level).unwrap_or(0);
        if level > 0 {
            let hashes = "#".repeat((level as usize).clamp(1, 6));
            out.push_str(&format!("{hashes} {text}\n\n"));
            continue;
        }
        // List items: the converter's computed number (Paragraph.listNumber)
        // or a bullet, indented two spaces per nesting level.
        if let Some(list) = style.and_then(|s| s.list.as_ref()) {
            if list.marker_kind != ListMarkerKind::None {
                let lv = (list.level as usize).min(8);
                let indent = "  ".repeat(lv);
                let marker = match p.list_number {
                    Some(n) => {
                        level_numbers.truncate(lv + 1);
                        if level_numbers.len() <= lv {
                            level_numbers.resize(lv + 1, 1);
                        }
                        level_numbers[lv] = n;
                        if list.tiered == Some(true) {
                            let path: Vec<String> =
                                level_numbers.iter().map(|n| n.to_string()).collect();
                            format!("{}.", path.join("."))
                        } else {
                            format!("{}.", list_label(n, list.number_kind))
                        }
                    }
                    None => "-".to_string(),
                };
                out.push_str(&format!("{indent}{marker} {text}\n\n"));
                continue;
            }
        }
        out.push_str(&format!("{text}\n\n"));
    }
}

/// A list number in its scheme: "3", "III", "iii", "C", "c".
fn list_label(n: u32, kind: Option<NumberKind>) -> String {
    fn roman(mut n: u32) -> String {
        let pairs = [
            (1000, "M"),
            (900, "CM"),
            (500, "D"),
            (400, "CD"),
            (100, "C"),
            (90, "XC"),
            (50, "L"),
            (40, "XL"),
            (10, "X"),
            (9, "IX"),
            (5, "V"),
            (4, "IV"),
            (1, "I"),
        ];
        let mut out = String::new();
        for (v, sym) in pairs {
            while n >= v {
                out.push_str(sym);
                n -= v;
            }
        }
        out
    }
    fn alpha(mut n: u32) -> String {
        let mut out = String::new();
        while n > 0 {
            n -= 1;
            out.insert(0, (b'A' + (n % 26) as u8) as char);
            n /= 26;
        }
        out
    }
    match kind {
        Some(NumberKind::RomanUpper) => roman(n),
        Some(NumberKind::RomanLower) => roman(n).to_ascii_lowercase(),
        Some(NumberKind::AlphaUpper) => alpha(n),
        Some(NumberKind::AlphaLower) => alpha(n).to_ascii_lowercase(),
        _ => n.to_string(),
    }
}

/// One comment (and its replies, nested as blockquotes) for the markdown dump.
fn comment_md(c: &Comment, depth: usize, out: &mut String) {
    let quote = ">".repeat(depth + 1);
    let who = match (&c.author, &c.date) {
        (Some(a), Some(d)) => format!("{a}, {d}"),
        (Some(a), None) => a.clone(),
        (None, Some(d)) => d.clone(),
        (None, None) => "comment".to_string(),
    };
    let on = c
        .quoted_text
        .as_deref()
        .map(|q| format!(" on \"{}\"", q.trim()))
        .unwrap_or_default();
    out.push_str(&format!(
        "{quote} **{who}** (paragraph {}){on}: {}\n\n",
        c.anchor_paragraph_index + 1,
        c.text.trim().replace('\n', " ")
    ));
    for r in c.replies.iter().flatten() {
        comment_md(r, depth + 1, out);
    }
}

fn pages_text(d: &PagesDocument, out: &mut String) {
    out.push_str(&format!("Pages document ({:?})\n", d.flavor));
    match d.flavor {
        PagesFlavor::WordProcessing => {
            if let Some(body) = &d.body {
                for p in &body.paragraphs {
                    let text = para_plain(p);
                    if text.is_empty() {
                        continue;
                    }
                    let level = p
                        .p_style
                        .and_then(|i| d.styles.para.get(i as usize))
                        .and_then(|s| s.outline_level)
                        .unwrap_or(0);
                    if level > 0 {
                        // Same clamp as the Markdown path: a document-supplied
                        // outline level is untrusted and sizes an allocation.
                        out.push_str(&format!(
                            "{} {}\n",
                            "#".repeat((level as usize).clamp(1, 6)),
                            text
                        ));
                    } else {
                        out.push_str(&format!("{text}\n"));
                    }
                }
            }
        }
        PagesFlavor::PageLayout => {
            for (i, page) in d.floating.iter().enumerate() {
                out.push_str(&format!(
                    "\n--- Page {} ---\n",
                    page.page_index.unwrap_or(i as u32) + 1
                ));
                let mut texts = Vec::new();
                for dr in &page.drawables {
                    drawable_texts(dr, &mut texts);
                }
                for (_, t) in texts {
                    if !t.is_empty() {
                        out.push_str(&format!("{t}\n"));
                    }
                }
            }
        }
    }
    warnings_block(&d.warnings, out, false);
}

fn pages_md(d: &PagesDocument, out: &mut String) {
    let flavor = match d.flavor {
        PagesFlavor::WordProcessing => "word-processing",
        PagesFlavor::PageLayout => "page-layout",
    };
    out.push_str(&format!("# Pages Document ({flavor})\n\n"));
    match d.flavor {
        PagesFlavor::WordProcessing => {
            if let Some(body) = &d.body {
                pages_body_md(body, &d.styles, out);
            }
            if let Some(footnotes) = &d.footnotes {
                if !footnotes.is_empty() {
                    out.push_str("---\n\n");
                    for f in footnotes {
                        let t = styled_plain(&f.text);
                        out.push_str(&format!("^{}: {t}\n\n", f.anchor_paragraph_index + 1));
                    }
                }
            }
            if let Some(comments) = &d.comments {
                if !comments.is_empty() {
                    out.push_str("## Comments\n\n");
                    for c in comments {
                        comment_md(c, 0, out);
                    }
                }
            }
            if let Some(changes) = &d.changes {
                if !changes.is_empty() {
                    out.push_str("## Tracked changes\n\n");
                    for c in changes {
                        let kind = match c.kind {
                            ChangeKind::Insertion => "inserted",
                            ChangeKind::Deletion => "deleted",
                        };
                        let who = c.author.as_deref().unwrap_or("unknown author");
                        let when = c
                            .date
                            .as_deref()
                            .map(|d| format!(" on {d}"))
                            .unwrap_or_default();
                        out.push_str(&format!(
                            "- paragraph {}: {kind} by {who}{when}: {:?}\n",
                            c.paragraph_index + 1,
                            c.text
                        ));
                    }
                    out.push('\n');
                }
            }
        }
        PagesFlavor::PageLayout => {
            for (i, page) in d.floating.iter().enumerate() {
                let label = page.page_index.map(|p| p + 1).unwrap_or(i as u32 + 1);
                out.push_str(&format!("## Page {label}\n\n"));
                for dr in &page.drawables {
                    let mut texts = Vec::new();
                    drawable_texts(dr, &mut texts);
                    for (kind, t) in texts {
                        if !t.is_empty() {
                            out.push_str(&format!("{}\n\n", md_text_block(&kind, &t)));
                        }
                    }
                }
            }
        }
    }
    warnings_block(&d.warnings, out, true);
}

// ---------------------------------------------------------------------------
// Numbers
// ---------------------------------------------------------------------------

/// Render one table as a markdown grid. Formula cells show the formula's
/// sourceText when trivially recoverable, else the stored last-calculated
/// value (docs/model-design.md §2.8; the dumpers never re-evaluate).
fn cell_text_plain(cell: &TableCell) -> String {
    match (&cell.v, cell.r#type) {
        (GridValue::None, _) => String::new(),
        (GridValue::Number(n), Some(CellTypeTag::Duration)) => {
            // Apple duration rendering: h:mm:ss (h omitted when 0)
            let total = *n as i64;
            let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
            if h > 0 {
                format!("{h}:{m:02}:{s:02}")
            } else {
                format!("{m:02}:{s:02}")
            }
        }
        (GridValue::Number(n), Some(CellTypeTag::Currency)) => match &cell.cur {
            Some(c) => format!("{} {}", fmt_num(*n), c),
            None => fmt_num(*n),
        },
        (GridValue::Richtext(t), _) => styled_plain(t),
        (GridValue::Scalar(s), Some(CellTypeTag::Error)) => format!("ERROR: {s}"),
        (GridValue::Scalar(s), _) => s.clone(),
        (GridValue::Number(n), _) => fmt_num(*n),
        (GridValue::Bool(b), _) => if *b { "TRUE" } else { "FALSE" }.to_string(),
    }
}

fn table_markdown(t: &TableModel, out: &mut String) {
    let name = t.name.clone().unwrap_or_else(|| "Table".to_string());
    out.push_str(&format!(
        "**{name}** ({} rows × {} columns)\n\n",
        t.row_count, t.column_count
    ));

    let hr = t.header_row_count as usize;
    let cell_text = |r: usize, c: usize| -> String {
        match t
            .grid
            .get(r)
            .and_then(|row| row.get(c))
            .and_then(|slot| slot.as_ref())
        {
            Some(GridCell::Plain(GridPlain::Text(s))) => escape_md(s),
            Some(GridCell::Plain(GridPlain::Number(n))) => fmt_num(n.as_f64().unwrap_or(0.0)),
            Some(GridCell::Plain(GridPlain::Bool(b))) => {
                if *b { "TRUE" } else { "FALSE" }.to_string()
            }
            Some(GridCell::Cell(cell)) => {
                // Formula cells: prefer the formula string when the source
                // carried a recoverable one, else the calculated value.
                if let Some(src) = cell.formula.as_ref().and_then(|f| f.source_text.as_ref()) {
                    return format!("`={src}`");
                }
                escape_md(&cell_text_plain(cell))
            }
            None => String::new(),
        }
    };

    let nrows = t.row_count.min(200) as usize; // cap dump size per table
    let ncols = t.column_count.min(40) as usize;

    // Markdown allows exactly one header line. Row 0 fills it when the
    // table declares headers (extra header rows continue as body — they
    // used to be dropped entirely); a zero-header table gets an empty
    // header line so the pipe block still parses as a table
    // (FINDINGS.md M-13).
    let body_start = if hr > 0 {
        out.push('|');
        for c in 0..ncols {
            out.push_str(&format!(" {} |", cell_text(0, c)));
        }
        out.push('\n');
        1
    } else {
        out.push('|');
        for _ in 0..ncols {
            out.push_str("   |");
        }
        out.push('\n');
        0
    };
    out.push('|');
    for _ in 0..ncols {
        out.push_str(" --- |");
    }
    out.push('\n');
    for r in body_start..nrows {
        out.push('|');
        for c in 0..ncols {
            out.push_str(&format!(" {} |", cell_text(r, c)));
        }
        out.push('\n');
    }
    out.push('\n');
}

fn numbers_text(d: &NumbersDocument, out: &mut String) {
    out.push_str(&format!("Numbers document — {} sheet(s)\n", d.sheets.len()));
    for sheet in &d.sheets {
        out.push_str(&format!("\n=== Sheet: {} ===\n", sheet.name));
        for dr in &sheet.drawables {
            match dr {
                Drawable::Table { table, .. } => {
                    let name = table.name.clone().unwrap_or_else(|| "Table".into());
                    out.push_str(&format!(
                        "\n{} ({} rows × {} cols)\n",
                        name, table.row_count, table.column_count
                    ));
                    let mut md = String::new();
                    table_markdown(table, &mut md);
                    // Strip markdown emphasis for the plain dump.
                    out.push_str(&md.replace("**", "").replace('`', ""));
                }
                Drawable::Chart { chart, .. } => {
                    out.push_str(&format!(
                        "\n[chart: {:?}, {} series]\n",
                        chart.r#type,
                        chart.series.len()
                    ));
                }
                Drawable::Image {
                    equation: Some(eq), ..
                } => out.push_str(&format!("\n{}\n", eq.source)),
                Drawable::Image { .. } => out.push_str("\n[image]\n"),
                Drawable::Group { children, .. } => {
                    let mut texts = Vec::new();
                    for c in children {
                        drawable_texts(c, &mut texts);
                    }
                    for (_, t) in texts {
                        if !t.is_empty() {
                            out.push_str(&format!("{t}\n"));
                        }
                    }
                }
                Drawable::Textbox { text, .. }
                | Drawable::Shape {
                    text: Some(text), ..
                } => {
                    let t = styled_plain(text);
                    if !t.is_empty() {
                        out.push_str(&format!("{t}\n"));
                    }
                }
                _ => {}
            }
        }
    }
    warnings_block(&d.warnings, out, false);
}

fn numbers_md(d: &NumbersDocument, out: &mut String) {
    out.push_str("# Numbers Document\n\n");
    for sheet in &d.sheets {
        out.push_str(&format!("## {}\n\n", escape_md(&sheet.name)));
        for dr in &sheet.drawables {
            match dr {
                Drawable::Table { table, .. } => table_markdown(table, out),
                Drawable::Chart { chart, .. } => {
                    out.push_str(&format!("*Chart ({:?}):* ", chart.r#type));
                    if chart.series.is_empty() {
                        out.push_str("no inline data\n\n");
                    } else {
                        out.push('\n');
                        out.push_str("| Series | Values |\n| --- | --- |\n");
                        for s in &chart.series {
                            let vals: Vec<String> = s
                                .values
                                .iter()
                                .map(|v| match v {
                                    Some(ChartValue::Number(n)) => fmt_num(*n),
                                    Some(ChartValue::Date(d)) => d.clone(),
                                    None => String::new(),
                                })
                                .collect();
                            out.push_str(&format!(
                                "| {} | {} |\n",
                                escape_md(s.name.as_deref().unwrap_or("")),
                                escape_md(&vals.join(", "))
                            ));
                        }
                        out.push('\n');
                    }
                }
                Drawable::Image {
                    equation: Some(eq), ..
                } => out.push_str(&format!("{}\n\n", equation_display_markdown(eq))),
                Drawable::Image { .. } => out.push_str("*(image)*\n\n"),
                Drawable::Movie { .. } => out.push_str("*(movie)*\n\n"),
                Drawable::Group { children, .. } => {
                    let mut texts = Vec::new();
                    for c in children {
                        drawable_texts(c, &mut texts);
                    }
                    for (kind, t) in texts {
                        if !t.is_empty() {
                            out.push_str(&format!("{}\n\n", md_text_block(&kind, &t)));
                        }
                    }
                }
                Drawable::Textbox { text, .. } => {
                    let t = styled_plain(text);
                    if !t.is_empty() {
                        out.push_str(&format!("{t}\n\n"));
                    }
                }
                Drawable::Shape {
                    text: Some(text), ..
                } => {
                    let t = styled_plain(text);
                    if !t.is_empty() {
                        out.push_str(&format!("{t}\n\n"));
                    }
                }
                _ => {}
            }
        }
    }
    warnings_block(&d.warnings, out, true);
}
