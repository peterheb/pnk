//! TSWP.StorageArchive → StyledText (docs/format/text.md, model-design §3.3).
//!
//! The storage holds one character buffer plus attribute tables keyed by
//! UTF-16 code-unit offsets. The splitter slices the buffer at paragraph
//! boundaries (newlines) and at character-style entry offsets, inlines the
//! resolved styles, converts U+FFFC positions to InlineObjectRuns and
//! textual/smart fields to FieldRuns. No offsets survive.

use crate::ctx::Ctx;
use crate::model::*;
use crate::pb::Msg;
use crate::styles::{resolve_list_format_minimal, resolve_para_style};

/// One attribute-table entry keyed by a UTF-16 offset.
#[derive(Debug, Clone)]
struct Entry {
    utf16_off: usize,
    object_id: Option<u64>,
}

pub struct ExtractedText {
    pub text: StyledText,
    /// Footnote bodies keyed by the paragraph index where their mark occurs.
    pub footnotes: Vec<(u32, StyledText)>,
    /// Table-of-contents entries from any TOC attachment in the storage.
    pub toc_entries: Vec<TocEntry>,
    /// Reviewer comments anchored in the storage (table_highlight).
    pub comments: Vec<Comment>,
    /// Bookmark anchors in the storage (table_bookmark).
    pub bookmarks: Vec<Bookmark>,
}

/// Side outputs of an attachment walk: footnote bodies and TOC entries.
struct Side {
    footnotes: Vec<(u32, StyledText)>,
    toc: Vec<TocEntry>,
}

/// table_language (19) entries: `{ character_index = 1, language = 2 }` —
/// the tag is a plain string in field 2, not a reference [inferred: paadopt
/// eb2a7cde stores "en" spans; an entry without field 2 ends the span].
fn language_entries_of(table: Option<Msg>) -> Vec<(usize, Option<String>)> {
    let Some(table) = table else {
        return Vec::new();
    };
    let mut out: Vec<(usize, Option<String>)> = table
        .msgs(1)
        .into_iter()
        .map(|e| (e.varint(1).unwrap_or(0) as usize, e.string(2).filter(|l| is_language_tag(l))))
        .collect();
    out.sort_by_key(|e| e.0);
    out
}

/// A plausible language tag: Apple also stores the marker "__multilingual"
/// for mixed runs (fixture 77890685), which is not a language.
pub fn is_language_tag(tag: &str) -> bool {
    !tag.is_empty()
        && !tag.starts_with('_')
        && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Primary language subtag of a locale or language tag ("en_US" → "en").
fn primary_subtag(tag: &str) -> String {
    tag.split(['_', '-'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Deleted ranges (UTF-16) from table_deletion (22): an entry whose object is
/// a TSWP.ChangeArchive with kind deletion (2) opens a range that the next
/// entry closes [proto: ChangeArchive.kind; fixture 55d37c2b stores the
/// deleted newline in the buffer and Pages hides it in the accepted view].
fn change_ranges(ctx: &Ctx, entries: &[Entry], want_kind: u64) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let Some(oid) = e.object_id else { continue };
        let kind = ctx.loaded.msg(oid).and_then(|m| m.varint(1)).unwrap_or(0);
        if kind != want_kind {
            continue;
        }
        let end = entries.get(i + 1).map(|n| n.utf16_off).unwrap_or(usize::MAX);
        if end > e.utf16_off {
            out.push((e.utf16_off, end));
        }
    }
    out
}

/// TSD.CommentStorageArchive → Comment (replies recurse one level per hop).
fn comment_of(ctx: &Ctx, id: u64, anchor: u32, quoted: Option<String>, depth: u32) -> Option<Comment> {
    let m = ctx.loaded.msg(id)?;
    let author = m
        .reference(3)
        .and_then(|a| ctx.loaded.msg(a))
        .and_then(|a| a.string(1))
        .filter(|s| !s.is_empty());
    let date = m
        .msg(2)
        .and_then(|d| d.f64v(1))
        .map(crate::colors::iso_from_apple_seconds);
    let replies: Vec<Comment> = if depth < 4 {
        m.references(4)
            .into_iter()
            .filter_map(|r| comment_of(ctx, r, anchor, None, depth + 1))
            .collect()
    } else {
        Vec::new()
    };
    Some(Comment {
        anchor_paragraph_index: anchor,
        text: m.string(1).unwrap_or_default(),
        author,
        date,
        quoted_text: quoted.filter(|q| !q.trim().is_empty()),
        replies: if replies.is_empty() { None } else { Some(replies) },
    })
}

/// Build a UTF-16 offset → char-index map for the text: entry `i` is the
/// UTF-16 offset where char `i` begins; the last entry is the total length.
fn utf16_map(text: &str) -> Vec<usize> {
    let mut map = Vec::with_capacity(text.chars().count() + 1);
    let mut acc = 0usize;
    for ch in text.chars() {
        map.push(acc);
        acc += ch.len_utf16();
    }
    map.push(acc);
    map
}

/// Convert a UTF-16 offset to a char index using the map (nearest lower bound).
fn u16_to_char_index(map: &[usize], off: usize) -> usize {
    match map.binary_search(&off) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    }
}

/// Pull (character_index, object reference id) entries from an
/// ObjectAttributeTable message.
fn entries_of(table: Option<Msg>) -> Vec<Entry> {
    let Some(table) = table else {
        return Vec::new();
    };
    let mut out: Vec<Entry> = table
        .msgs(1)
        .into_iter()
        .map(|e| Entry {
            utf16_off: e.varint(1).unwrap_or(0) as usize,
            object_id: e.reference(2),
        })
        .collect();
    out.sort_by_key(|e| e.utf16_off);
    out
}

/// Entry with the largest offset <= `off`.
fn entry_at(entries: &[Entry], off: usize) -> Option<&Entry> {
    if entries.is_empty() {
        return None;
    }
    let idx = entries.partition_point(|e| e.utf16_off <= off);
    if idx == 0 {
        None
    } else {
        Some(&entries[idx - 1])
    }
}

pub fn extract(ctx: &mut Ctx, storage_id: u64) -> Option<ExtractedText> {
    let storage = ctx.loaded.msg(storage_id)?.clone();
    extract_from_msg(ctx, &storage)
}

/// Contained-storage nesting ceiling. Apple allows one level (a footnote
/// body); a crafted cyclic storage→attachment→storage graph must bottom out
/// instead of overflowing the stack (FINDINGS.md H-4).
const MAX_TEXT_NEST: u32 = 8;

pub fn extract_from_msg(ctx: &mut Ctx, storage: &Msg) -> Option<ExtractedText> {
    if ctx.text_extract_depth >= MAX_TEXT_NEST {
        ctx.warn(
            crate::model::WarningCode::UnsupportedFeature,
            format!("text storages nest deeper than {MAX_TEXT_NEST} levels; inner content dropped"),
        );
        return None;
    }
    ctx.text_extract_depth += 1;
    let out = extract_from_msg_inner(ctx, storage);
    ctx.text_extract_depth -= 1;
    out
}

fn extract_from_msg_inner(ctx: &mut Ctx, storage: &Msg) -> Option<ExtractedText> {
    let text = storage.string(3).unwrap_or_default();
    let map = utf16_map(&text);

    let para_entries = entries_of(storage.msg(5));
    let char_entries = entries_of(storage.msg(8));
    let attach_entries = entries_of(storage.msg(9));
    let smart_entries = entries_of(storage.msg(11));
    // Footnote anchors live in their OWN table (table_footnote, field 16
    // [proto: TSWPArchives.proto StorageArchive]) keyed to a U+000E anchor
    // char in the buffer — NOT in table_attachment with U+FFFC (fixture G5:
    // "This word\u{0E} is footnoted." with the
    // FootnoteReferenceAttachmentArchive only reachable from field 16).
    let footnote_entries = entries_of(storage.msg(16));
    // Drop caps: table_drop_cap_style (28 [proto: TSWPArchives.proto
    // StorageArchive]) keys paragraph starts to DropCapStyleArchives. Applied
    // only to the paragraph whose start EXACTLY matches an entry with an
    // object [inferred: G5 stores {0: null, 6366: dropcap-style-0} and Apple
    // caps only the paragraph at 6366, not the following ones — so the
    // usual carry-forward range semantics do not hold here].
    let dropcap_entries = entries_of(storage.msg(28));
    // Annotation tables: bookmarks (15), language spans (19), tracked
    // changes (21 insertion / 22 deletion), comment highlights (23).
    let bookmark_entries = entries_of(storage.msg(15));
    let language_entries = language_entries_of(storage.msg(19));
    let insertion_entries = entries_of(storage.msg(21));
    let deletion_entries = entries_of(storage.msg(22));
    let highlight_entries = entries_of(storage.msg(23));
    let doc_lang = ctx.meta.locale.as_deref().map(primary_subtag);
    // The language a run would carry at `off`: the span's tag when it
    // differs from the document locale, else nothing (omit-default).
    let emitted_lang = |off: usize| -> Option<String> {
        let idx = language_entries.partition_point(|(o, _)| *o <= off);
        let (_, lang) = idx.checked_sub(1).map(|i| &language_entries[i])?;
        let lang = lang.as_ref()?;
        (doc_lang.as_deref() != Some(primary_subtag(lang).as_str())).then(|| lang.clone())
    };
    // Run boundaries only where the emitted language actually changes: a
    // span equal to the locale must not split a run (golden G2 keeps
    // "Shapes can hold text too." as one item).
    let language_boundaries: Vec<usize> = language_entries
        .iter()
        .filter(|(off, _)| *off > 0 && emitted_lang(*off) != emitted_lang(off - 1))
        .map(|(off, _)| *off)
        .collect();

    // Tracked changes: emit the ACCEPTED view (insertions kept, deletions
    // omitted) and say so once; the review markup itself is not modeled.
    let deleted = change_ranges(ctx, &deletion_entries, 2);
    let inserted = change_ranges(ctx, &insertion_entries, 1);
    if !deleted.is_empty() || !inserted.is_empty() {
        ctx.warn(
            WarningCode::UnsupportedFeature,
            format!(
                "tracked changes: {} insertion(s) kept, {} deletion(s) omitted; text is the accepted view",
                inserted.len(),
                deleted.len()
            ),
        );
    }
    let in_deleted = |off: usize| deleted.iter().any(|(a, b)| off >= *a && off < *b);

    // List membership + levels + item numbers (fixture-verified against
    // G1-golden: gotchas #14):
    // - table_list_style (7): paragraph → list-style ranges (the style's
    //   label_type per level carries marker kind).
    // - table_para_data (6).first = list LEVEL (0-based) for the para.
    // - table_para_starts (14).first = the list item NUMBER Apple prints for
    //   this paragraph (0 = none, keep counting).
    let list_entries = entries_of(storage.msg(7));
    let para_data = para_table(storage.msg(6));
    let para_starts = para_table(storage.msg(14));

    let mut side = Side {
        footnotes: Vec::new(),
        toc: Vec::new(),
    };

    // Paragraph boundaries: newlines in the character buffer (text.md
    // §Paragraph model). Each paragraph spans [start_char, end_char) in
    // char-index space; the trailing newline is not part of the paragraph.
    // U+0004 is Pages' section-break marker and ends a paragraph exactly
    // like a newline: 10a06959's buffer reads "…\u{FFFC}\u{FFFC}\u{4}Stap 1"
    // where the two anchored cover boxes belong to the section BEFORE the
    // break (Apple's export: page 1) and "Stap 1" opens the next section on
    // page 2. Treating the marker as text glued the anchors to the page-2
    // paragraph. 12 corpus docs carry the marker. [inferred]
    // U+0005 is the hard PAGE break, again ending a paragraph: b31db822
    // stores one alone before "Executive Summary", "Introduction", each
    // annex heading — every one a page top in Apple's export — and
    // 155d6ba3 alternates image / U+0005 / image for one photo per page.
    // The paragraph after the marker carries `page_break_before`.
    // [inferred, 26 corpus docs]
    let mut para_ranges: Vec<(usize, usize)> = Vec::new();
    let mut page_break_before: Vec<bool> = vec![false];
    {
        let mut start_char = 0usize;
        for (ci, ch) in text.chars().enumerate() {
            if ch == '\n' || ch == '\u{4}' || ch == '\u{5}' {
                para_ranges.push((start_char, ci));
                page_break_before.push(ch == '\u{5}');
                start_char = ci + 1;
            }
        }
        para_ranges.push((start_char, text.chars().count()));
    }
    // Paragraph index holding a UTF-16 offset (anchors of bookmarks and
    // comments); an offset past the end lands in the last paragraph.
    let para_at = |off: usize| -> u32 {
        let ci = u16_to_char_index(&map, off);
        para_ranges
            .iter()
            .position(|(_, end)| ci <= *end)
            .unwrap_or(para_ranges.len().saturating_sub(1)) as u32
    };
    let bookmarks: Vec<Bookmark> = bookmark_entries
        .iter()
        .filter_map(|e| {
            let m = ctx.loaded.msg(e.object_id?)?;
            let id = m.msg(1).and_then(|sup| sup.string(1)).filter(|s| !s.is_empty())?;
            Some(Bookmark {
                id,
                name: m.string(2).filter(|s| !s.is_empty()),
                paragraph_index: para_at(e.utf16_off),
            })
        })
        .collect();
    let mut comments: Vec<Comment> = Vec::new();
    for (i, e) in highlight_entries.iter().enumerate() {
        let Some(hid) = e.object_id else { continue };
        let Some(cid) = ctx.loaded.msg(hid).and_then(|h| h.reference(1)) else {
            continue;
        };
        let end = highlight_entries
            .get(i + 1)
            .map(|n| n.utf16_off)
            .unwrap_or(e.utf16_off);
        let quoted: String = {
            let a = u16_to_char_index(&map, e.utf16_off);
            let b = u16_to_char_index(&map, end);
            text.chars().skip(a).take(b.saturating_sub(a)).collect()
        };
        if let Some(c) = comment_of(ctx, cid, para_at(e.utf16_off), Some(quoted), 0) {
            comments.push(c);
        }
    }

    // Per-paragraph list info: (list style id, level, stored item number). An entry at
    // paragraph start applies from there until the next entry.
    let mut list_by_para: Vec<Option<(u64, u32, u64)>> = vec![None; para_ranges.len()];
    {
        let mut cur: Option<u64> = None;
        let mut ei = 0usize;
        for (pi, (start, _)) in para_ranges.iter().enumerate() {
            let p_start_u16 = map[*start];
            while ei < list_entries.len() && list_entries[ei].utf16_off <= p_start_u16 {
                let e = &list_entries[ei];
                cur = e.object_id; // None = list membership cleared
                ei += 1;
            }
            // Level + item number are PER-PARAGRAPH tables, read at each
            // paragraph start (G1: "Two"/"Three" carry no number of their own
            // and continue the counter "One" opened at 1).
            list_by_para[pi] = cur.map(|lid| {
                (
                    lid,
                    para_level(&para_data, p_start_u16),
                    para_list_number(&para_starts, p_start_u16),
                )
            });
        }
    }

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut last_style: Option<u64> = None;
    // List numbering state: the last number printed at each level. A stored
    // number (table_para_starts) restarts a level; otherwise the level
    // continues, and any item at a level restarts the levels below it
    // (48f5f124: "1. Turnus" then "1.1 … 1.4", "2. Turnus" then "2.1 …").
    let mut list_counters: Vec<u32> = Vec::new();

    for (pi, (start, end)) in para_ranges.iter().enumerate() {
        let p_start_u16 = map[*start];
        let p_end_u16 = map[*end];

        // Paragraph style: entry exactly at the paragraph start sets/overrides
        // the style; a null object (or no entry) carries the previous
        // paragraph's style forward (docs/format/text.md §Paragraph model,
        // [parser: iwork@02c26ebf] "A null style seems to mean keep the
        // previous one" — KVKK fixture: 15/20 body paragraphs sit on null
        // entries and must inherit the justify+8pt style, not reset to
        // default).
        let mut style_ref = last_style;
        if let Some(e) = entry_at(&para_entries, p_start_u16) {
            if let Some(oid) = e.object_id {
                if e.utf16_off == p_start_u16 || style_ref.is_none() {
                    style_ref = Some(oid);
                }
            }
        }
        if style_ref.is_some() {
            last_style = style_ref;
        }

        let mut items: Vec<ParagraphItem> = Vec::new();

        // Run boundaries: char-style entry offsets and U+FFFC positions
        // within the paragraph, plus the paragraph start.
        let mut boundaries: Vec<usize> = vec![p_start_u16];
        for e in &char_entries {
            if e.utf16_off > p_start_u16 && e.utf16_off < p_end_u16 {
                boundaries.push(e.utf16_off);
            }
        }
        // Anchored attachments occupy exactly their U+FFFC position; when a
        // writer glues the FFFC to adjacent text (fixture G5: inline images
        // in the same run as body words), the attachment entry offset must
        // still be a run boundary so the object can be split off.
        for e in &attach_entries {
            if e.utf16_off > p_start_u16 && e.utf16_off < p_end_u16 {
                boundaries.push(e.utf16_off);
            }
        }
        for e in &footnote_entries {
            if e.utf16_off > p_start_u16 && e.utf16_off < p_end_u16 {
                boundaries.push(e.utf16_off);
            }
        }
        // Smart-field spans (hyperlinks, date fields) are boundaries too:
        // without them a field found anywhere in a longer segment decorated
        // the WHOLE segment, expanding a link into surrounding text
        // (FINDINGS.md M-7). Each entry offset starts or (null object) ends
        // a span, so cutting at every offset confines the field exactly.
        for e in &smart_entries {
            if e.utf16_off > p_start_u16 && e.utf16_off < p_end_u16 {
                boundaries.push(e.utf16_off);
            }
        }
        for off in &language_boundaries {
            if *off > p_start_u16 && *off < p_end_u16 {
                boundaries.push(*off);
            }
        }
        for (a, b) in &deleted {
            for off in [*a, *b] {
                if off > p_start_u16 && off < p_end_u16 {
                    boundaries.push(off);
                }
            }
        }
        for (ci, ch) in text
            .chars()
            .enumerate()
            .skip(*start)
            .take(end.saturating_sub(*start))
        {
            if ch == '\u{FFFC}' {
                boundaries.push(map[ci]);
            }
        }
        boundaries.push(p_end_u16);
        boundaries.sort_unstable();
        boundaries.dedup();

        for w in boundaries.windows(2) {
            let (b0, b1) = (w[0], w[1]);
            if b1 <= b0 || in_deleted(b0) {
                continue;
            }
            // Effective char style: last char-style entry at or before b0
            // (null object = no run-level override), merged over the
            // PARAGRAPH style's char_properties chain — heading/title fonts
            // and placeholder text sizes live there (G5 goldens; RIPE deck).
            let char_sid = entry_at(&char_entries, b0).and_then(|e| e.object_id);
            let mut char_style = crate::styles::resolve_effective_char_style(ctx, char_sid, style_ref);
            // Run language from table_language, only when it differs from
            // the document locale (omit-default: the locale covers the rest).
            if char_style.language.is_none() {
                char_style.language = emitted_lang(b0);
            }
            let c_style = ctx
                .char_pool
                .intern(crate::ctx::strip_char_defaults(char_style));

            let seg_start_char = u16_to_char_index(&map, b0);
            let seg_end_char = u16_to_char_index(&map, b1);
            let mut seg: String = text
                .chars()
                .skip(seg_start_char)
                .take(seg_end_char - seg_start_char)
                .collect();

            // A segment starting at an attachment entry (its U+FFFC anchor
            // char) becomes an inline object / field run. The attachment
            // occupies exactly one char; remaining text in the segment falls
            // through to the text path below (G5: writers glue FFFC to body
            // words in the same run).
            // Footnote anchor at the segment start: its U+000E char becomes
            // the mark field; the referenced storage becomes the footnote
            // body (table_footnote, see above).
            if let Some(fe) = footnote_entries.iter().find(|e| e.utf16_off == b0) {
                if let AttachmentResult::Field {
                    style,
                    value,
                    field,
                } = resolve_attachment(ctx, fe.object_id, pi as u32, &mut side)
                {
                    items.push(ParagraphItem::Field {
                        kind: FieldTag::Field,
                        c_style: ctx
                            .char_pool
                            .intern(crate::ctx::strip_char_defaults(*style)),
                        value,
                        field,
                    });
                    // The anchor occupies exactly one char; the remainder
                    // falls THROUGH to the normal path so it keeps the run's
                    // style and any hyperlink (FINDINGS.md M-7 — it used to
                    // flatten to an unstyled Plain run).
                    seg = seg.chars().skip(1).collect();
                    if seg.is_empty() {
                        continue;
                    }
                }
            }

            let seg_starts_fffc = seg.starts_with('\u{FFFC}');
            let att_at_start = seg_starts_fffc
                .then(|| attach_entries.iter().find(|e| e.utf16_off == b0))
                .flatten();
            if let Some(att) = att_at_start {
                let resolved = resolve_attachment(ctx, att.object_id, pi as u32, &mut side);
                let consumed_anchor = !matches!(resolved, AttachmentResult::None);
                match resolved {
                    AttachmentResult::Drawable(boxed, h_off, v_off) => {
                        let drawable = *boxed;
                        // b31db822 stores 0xffffffff (NaN) offsets on two
                        // attachments — serde writes those as null; drop them
                        let clean = |v: Option<f64>| v.filter(|x| x.is_finite());
                        let (h_off, v_off) = (clean(h_off), clean(v_off));
                        let offset = if h_off.is_some() || v_off.is_some() {
                            Some(InlineOffset {
                                h_pt: h_off,
                                v_pt: v_off,
                            })
                        } else {
                            None
                        };
                        // Inline vs "Move with Text": both arrive as a
                        // DrawableAttachmentArchive at a U+FFFC. Corpus
                        // survey (323 Pages docs, 2026-09-01): every
                        // inline-with-text object stores h/v_offset 0,0 and
                        // exterior wrap kind none (370/374); anchored ones
                        // carry a non-zero offset from their anchor
                        // paragraph and a wrap kind (732/734). Fixtures
                        // 10a06959 (cover title boxes) and b31db822 (cover
                        // image + shape) render at anchor + offset in
                        // Apple's export. [inferred]
                        // Sub-4pt offsets with no wrap are docx-import
                        // residue on inline tables (0839b6d2: -3.5/-1.9 —
                        // Apple stacks the table in the flow), not placement.
                        let moved = |v: Option<f64>| v.map(|x| x.abs() >= 4.0).unwrap_or(false);
                        let wraps = drawable_wraps(&drawable);
                        let anchored = (moved(h_off) || moved(v_off) || wraps).then_some(true);
                        items.push(ParagraphItem::InlineObject {
                            kind: InlineObjectTag::InlineObject,
                            drawable,
                            offset,
                            anchored,
                        });
                    }
                    AttachmentResult::Field {
                        style,
                        value,
                        field,
                    } => {
                        items.push(ParagraphItem::Field {
                            kind: FieldTag::Field,
                            c_style: ctx
                                .char_pool
                                .intern(crate::ctx::strip_char_defaults(*style)),
                            value,
                            field,
                        });
                    }
                    AttachmentResult::None => {}
                }
                if consumed_anchor {
                    // The FFFC anchor occupies exactly one char; the
                    // remainder falls THROUGH to the normal path so it keeps
                    // the run's style and any hyperlink (FINDINGS.md M-7 —
                    // it used to flatten to an unstyled Plain run).
                    seg = seg.chars().skip(1).collect();
                    if seg.is_empty() {
                        continue;
                    }
                }
            }

            // Smart field covering this segment: date fields or hyperlinks.
            // Span semantics like char styles — the entry at-or-before b0
            // governs (a null entry ends the field), and the boundary set
            // above guarantees a segment never straddles a field edge
            // (FINDINGS.md M-7). Dispatch on the record's registry NAME —
            // field sniffing misfired on TSWP.PlaceholderSmartFieldArchive,
            // whose field 2 (localizable bool) turned whole template
            // paragraphs into garbage links (00C Textbook fixture).
            let mut hyperlink: Option<String> = None;
            let mut field_kind: Option<FieldKind> = None;
            {
                if let Some(se) = entry_at(&smart_entries, b0) {
                    if let Some(sid) = se.object_id {
                        let type_name = ctx
                            .loaded
                            .record(sid)
                            .and_then(|r| r.name.clone())
                            .unwrap_or_default();
                        if let Some(sm) = ctx.loaded.msg(sid).cloned() {
                            match type_name.as_str() {
                                "TSWP.DateTimeSmartFieldArchive" => {
                                    // update_plan = 6 [proto]
                                    let plan = match sm.varint(6).unwrap_or(1) {
                                        0 => DateUpdatePlan::Never,
                                        2 => DateUpdatePlan::Once,
                                        _ => DateUpdatePlan::Auto,
                                    };
                                    field_kind = Some(FieldKind::Date { update_plan: plan });
                                }
                                "TSWP.HyperlinkFieldArchive"
                                | "TSWP.UnsupportedHyperlinkFieldArchive" => {
                                    // url_ref is a plain STRING field 2 [proto:
                                    // TSWPArchives.proto:782]; older writers may
                                    // store a reference to a URL object instead.
                                    hyperlink =
                                        sm.string(2).filter(|u| valid_url(u)).or_else(|| {
                                            sm.reference(2)
                                                .and_then(|u| ctx.loaded.msg(u))
                                                .and_then(|um| {
                                                    um.string(1).or_else(|| um.string(3))
                                                })
                                                .filter(|u| valid_url(u))
                                        });
                                }
                                // Placeholder/bookmark/TOC smart fields: plain
                                // text, no decoration.
                                _ => {}
                            }
                        }
                    }
                }
            }

            if let Some(fk) = field_kind {
                items.push(ParagraphItem::Field {
                    kind: FieldTag::Field,
                    c_style,
                    value: Some(seg),
                    field: fk,
                });
            } else if !seg.is_empty() {
                // Bare string when plain (unstyled, no hyperlink/language);
                // object only when there is more to say.
                if c_style.is_none() && hyperlink.is_none() {
                    items.push(ParagraphItem::Plain(seg));
                } else {
                    items.push(ParagraphItem::Text {
                        text: seg,
                        c_style,
                        hyperlink,
                        language: None,
                    });
                }
            }
        }

        // An EMPTY paragraph still occupies a line, and its height is the
        // PARAGRAPH style's own font size — there is no run to carry it, so
        // the boundary loop above emitted nothing at all and the viewer fell
        // back to its 12pt default. d434501c's flyer opens with three blank
        // centred lines that Apple gives 28pt each; at 14pt the title landed
        // 40pt high, under its own anchored logo. One empty run carries the
        // size (and face) through; it is not content — no list marker, no
        // text — and is only emitted when the style actually names one.
        if items.is_empty() {
            let cs = crate::styles::resolve_effective_char_style(
                ctx,
                entry_at(&char_entries, p_start_u16).and_then(|e| e.object_id),
                style_ref,
            );
            if cs.font_size_pt.is_some() || cs.font_name.is_some() {
                let c_style = ctx.char_pool.intern(crate::ctx::strip_char_defaults(cs));
                items.push(ParagraphItem::Text {
                    text: String::new(),
                    c_style,
                    hyperlink: None,
                    language: None,
                });
            }
        }

        let mut pstyle = match style_ref {
            Some(sid) => resolve_para_style(ctx, sid),
            None => ParaStyle::default(),
        };
        // Overlay list membership/level/number from the storage tables
        // (gotchas #14). The theme list style carries the marker vocabulary;
        // the storage tables carry WHO is in a list, at WHAT level, and
        // which number each item prints — decodable without theme resolution.
        let mut list_number = None;
        if let Some((lid, level, number)) = list_by_para[pi] {
            let mut lf = resolve_list_format_minimal(ctx, lid, level);
            if number > 0 && lf.marker_kind == ListMarkerKind::Number {
                lf.start = Some(number as f64);
            }
            if lf.marker_kind != ListMarkerKind::None {
                let lv = level as usize;
                if list_counters.len() <= lv {
                    list_counters.resize(lv + 1, 0);
                }
                list_counters.truncate(lv + 1);
                if lf.marker_kind == ListMarkerKind::Number {
                    let n = if number > 0 { number as u32 } else { list_counters[lv] + 1 };
                    list_counters[lv] = n;
                    list_number = Some(n);
                }
            }
            pstyle.list = Some(lf);
        }
        // Drop cap for the paragraph starting exactly at a table entry.
        if let Some(dcid) = dropcap_entries
            .iter()
            .find(|e| e.utf16_off == p_start_u16)
            .and_then(|e| e.object_id)
        {
            pstyle.drop_cap = crate::styles::resolve_drop_cap(ctx, dcid);
        }
        let p_style = ctx
            .para_pool
            .intern(crate::ctx::strip_para_defaults(pstyle));
        paragraphs.push(Paragraph {
            p_style,
            items,
            page_break_before: page_break_before[pi].then_some(true),
            list_number,
        });
    }

    Some(ExtractedText {
        text: StyledText { paragraphs },
        footnotes: side.footnotes,
        toc_entries: side.toc,
        comments,
        bookmarks,
    })
}

/// A safe hyperlink target: printable, and on the scheme allowlist. The
/// value lands in an anchor `href` in the viewer, so this is a security
/// boundary, not just junk filtering — `javascript:`, `file:`, and custom
/// schemes from an untrusted document must never become clickable. The
/// viewer repeats this policy on its side (viewer/src/text.ts).
fn valid_url(u: &str) -> bool {
    if u.is_empty() || u.chars().any(|c| (c as u32) < 0x20 || c == '\u{FFFD}') {
        return false;
    }
    if u.starts_with('#') {
        return true; // same-document fragment
    }
    let lower = u.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}

// Boxing the big variants is deferred (FINDINGS.md P2); allow for now.
#[allow(clippy::large_enum_variant)]
/// Exterior text wrap other than `none` — the mark of a "Move with Text"
/// object (inline objects have no exterior wrap to speak of).
fn drawable_wraps(d: &Drawable) -> bool {
    let common = match d {
        Drawable::Shape { common, .. }
        | Drawable::Textbox { common, .. }
        | Drawable::Image { common, .. }
        | Drawable::Movie { common, .. }
        | Drawable::Group { common, .. }
        | Drawable::Table { common, .. }
        | Drawable::Chart { common, .. }
        | Drawable::ConnectionLine { common, .. } => common,
        Drawable::Unknown { .. } => return false,
    };
    common
        .text_wrap
        .as_ref()
        .map(|w| w.kind != TextWrapKind::None)
        .unwrap_or(false)
}

enum AttachmentResult {
    // boxed: a Drawable is ~11 KB and a CharStyle ~370 bytes, against a
    // handful of bytes for the rest (clippy large_enum_variant)
    Drawable(Box<Drawable>, Option<f64>, Option<f64>),
    Field {
        style: Box<CharStyle>,
        value: Option<String>,
        field: FieldKind,
    },
    None,
}

/// Resolve one attachment object (a table_attachment entry's target) into
/// either an embedded drawable or a field run.
fn resolve_attachment(
    ctx: &mut Ctx,
    object_id: Option<u64>,
    para_index: u32,
    side: &mut Side,
) -> AttachmentResult {
    let Some(oid) = object_id else {
        return AttachmentResult::None;
    };
    let Some(rec) = ctx.loaded.record(oid).cloned() else {
        ctx.warn_detail(
            WarningCode::UnresolvedReference,
            format!("attachment reference {oid} points nowhere"),
            oid.to_string(),
        );
        return AttachmentResult::None;
    };
    match rec.type_id {
        // TSWP.DrawableAttachmentArchive { drawable = 1, h_offset = 3, v_offset = 5 }
        2003 => {
            let Some(att) = rec.msg else {
                return AttachmentResult::None;
            };
            let drawable_id = att.reference(1);
            let h = att.f32v(3).map(|v| v as f64);
            let v = att.f32v(5).map(|v| v as f64);
            match drawable_id {
                Some(did) => {
                    let d = crate::drawables::convert_drawable(ctx, did);
                    AttachmentResult::Drawable(Box::new(d), h, v)
                }
                None => AttachmentResult::None,
            }
        }
        // TSWP.TextualAttachmentArchive { string_equivalent = 1, kind = 2 }
        // kind: 0 page-number, 1 page-count, 2 footnote-mark (ids 2004/2007/2009)
        2004 | 2007 | 2009 => {
            let Some(att) = rec.msg else {
                return AttachmentResult::None;
            };
            let kind = match att.varint(2).unwrap_or(0) {
                0 => FieldKind::PageNumber,
                1 => FieldKind::PageCount,
                _ => FieldKind::FootnoteMark,
            };
            AttachmentResult::Field {
                style: Box::new(CharStyle::default()),
                value: att.string(1),
                field: kind,
            }
        }
        // TSWP.TOCAttachmentArchive { super = 1: DrawableAttachmentArchive }
        // → TSWP.TOCInfoArchive (a ShapeInfo carrying the laid-out TOC text)
        // whose toc_entry_data (3) → TOCEntryInstanceArchive { paragraph_index
        // = 1, page_number = 2, heading = 4, indexed_paragraph_level = 8 }
        // [proto: TSWPArchives.proto; fixture eb2a7cde]. The rendered TOC
        // becomes an inline textbox; the entries go to the document.
        2241 => {
            let Some(att) = rec.msg.as_ref().and_then(|a| a.msg(1)) else {
                return AttachmentResult::None;
            };
            let h = att.f32v(3).map(|v| v as f64);
            let v = att.f32v(5).map(|v| v as f64);
            let Some(did) = att.reference(1) else {
                return AttachmentResult::None;
            };
            if let Some(info) = ctx.loaded.msg(did).cloned() {
                for eid in info.references(3) {
                    let Some(e) = ctx.loaded.msg(eid) else { continue };
                    side.toc.push(TocEntry {
                        text: e.string(4).unwrap_or_default(),
                        page_number: e.varint(2).map(|n| n as u32),
                        level: e.varint(8).map(|n| n as u32),
                        paragraph_index: e.varint(1).map(|n| n as u32),
                    });
                }
            }
            let d = crate::drawables::convert_drawable(ctx, did);
            AttachmentResult::Drawable(Box::new(d), h, v)
        }
        // TSWP.TSWPTOCPageNumberAttachmentArchive { super = 1, page_number
        // = 2, bookmark_name = 3 }: the number is field 2 (field 3 is the
        // target bookmark, not a value) [proto: TSWPArchives.proto].
        2010 => {
            let msg = rec.msg.as_ref();
            let value = msg.and_then(|a| a.string(2));
            let kind_val = msg
                .and_then(|a| a.msg(1))
                .and_then(|sup| sup.varint(2))
                .unwrap_or(0);
            let field = match kind_val {
                1 => FieldKind::PageCount {},
                2 => FieldKind::FootnoteMark {},
                _ => FieldKind::PageNumber {},
            };
            AttachmentResult::Field {
                style: Box::new(CharStyle::default()),
                value,
                field,
            }
        }
        // TSWP.NumberAttachmentArchive { super = 1 (TextualAttachment),
        //   number_format = 2, string_value = 3, number_format_name = 4 }
        // Field kind from the TextualAttachmentArchive super's kind (f2):
        // 0 = page-number, 1 = page-count, 2 = footnote-mark.
        2043 => {
            let msg = rec.msg.as_ref();
            let value = msg.and_then(|a| a.string(3).or_else(|| a.string(2)));
            let kind_val = msg
                .and_then(|a| a.msg(1))
                .and_then(|sup| sup.varint(2))
                .unwrap_or(0);
            let field = match kind_val {
                1 => FieldKind::PageCount {},
                2 => FieldKind::FootnoteMark {},
                _ => FieldKind::PageNumber {},
            };
            AttachmentResult::Field {
                style: Box::new(CharStyle::default()),
                value,
                field,
            }
        }
        // TSWP.FootnoteReferenceAttachmentArchive
        // { super = 1, contained_storage = 2 } — footnote body text.
        2008 => {
            let storage_id = rec.msg.as_ref().and_then(|a| a.reference(2));
            if let Some(sid) = storage_id {
                if let Some(ex) = extract(ctx, sid) {
                    side.footnotes.push((para_index, ex.text));
                }
            }
            AttachmentResult::Field {
                style: Box::new(CharStyle::default()),
                value: None,
                field: FieldKind::FootnoteMark,
            }
        }
        // Other attachment types (e.g. TSWP.UIGraphicalAttachment 2006):
        // represent as an unknown drawable so nothing is silently dropped.
        _ => {
            let type_name = rec.name.clone();
            let reason = format!(
                "attachment of unmodeled type {}; recorded as unknown",
                type_name
                    .clone()
                    .unwrap_or_else(|| format!("type id {}", rec.type_id))
            );
            AttachmentResult::Drawable(
                Box::new(Drawable::Unknown {
                    common: None,
                    type_id: format!("0x{:x}", rec.type_id),
                    type_name,
                    reason,
                }),
                None,
                None,
            )
        }
    }
}

/// ParaDataAttributeTable (fields 6/14/24): entries {idx, first, second}.
fn para_table(table: Option<Msg>) -> Vec<(usize, u64, u64)> {
    let Some(table) = table else {
        return Vec::new();
    };
    table
        .msgs(1)
        .into_iter()
        .map(|e| {
            (
                e.varint(1).unwrap_or(0) as usize,
                e.varint(2).unwrap_or(0),
                e.varint(3).unwrap_or(0),
            )
        })
        .collect()
}

/// List LEVEL for the paragraph starting at `utf16_off`: the last
/// table_para_data entry at or before it carries `.first` (0-based level).
fn para_level(para_data: &[(usize, u64, u64)], utf16_off: usize) -> u32 {
    let mut level = 0;
    for (idx, first, _) in para_data {
        if *idx <= utf16_off {
            level = *first as u32;
        } else {
            break;
        }
    }
    level.min(8)
}

/// List item NUMBER for a paragraph: the last table_para_starts entry at or
/// before the paragraph start carries `.first`, and it is the number Apple
/// prints — not a restart flag. b31db822 stores 1 on "Are these proposals
/// clear?", then 6 / 10 / 15 / 17 / 19 on the first question of each later
/// section, and Apple's export prints exactly those, numbering (1) … (19)
/// straight through the document; reading the field as a boolean restarted
/// every group at (1). 0 = no explicit number, so numbering continues.
/// [inferred: TSWP.ParagraphStartAttributeTable, 5 fixtures]
fn para_list_number(para_starts: &[(usize, u64, u64)], utf16_off: usize) -> u64 {
    let mut n = 0;
    for (idx, first, _) in para_starts {
        if *idx <= utf16_off {
            n = *first;
        } else {
            break;
        }
    }
    n
}
