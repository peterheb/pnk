//! TSWP.StorageArchive → StyledText (docs/format/text.md, model-design §3.3).
//!
//! The storage holds one character buffer plus attribute tables keyed by
//! UTF-16 code-unit offsets. The splitter slices the buffer at paragraph
//! boundaries (newlines) and at character-style entry offsets, inlines the
//! resolved styles, converts U+FFFC positions to InlineObjectRuns and
//! textual/smart fields to FieldRuns. No offsets survive.

use crate::ctx::Ctx;
use crate::styles::{resolve_char_style, resolve_list_format_minimal, resolve_para_style};
use crate::model::*;
use crate::pb::Msg;

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
    let Some(table) = table else { return Vec::new() };
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

pub fn extract_from_msg(ctx: &mut Ctx, storage: &Msg) -> Option<ExtractedText> {
    let text = storage.string(3).unwrap_or_default();
    let map = utf16_map(&text);

    let para_entries = entries_of(storage.msg(5));
    let char_entries = entries_of(storage.msg(8));
    let attach_entries = entries_of(storage.msg(9));
    let smart_entries = entries_of(storage.msg(11));

    // List membership + levels + restart flags (fixture-verified against
    // G1-golden: gotchas #14):
    // - table_list_style (7): paragraph → list-style ranges (the style's
    //   label_type per level carries marker kind).
    // - table_para_data (6).first = list LEVEL (0-based) for the para.
    // - table_para_starts (14).first = list RESTART flag (numbering restarts
    //   at this paragraph).
    let list_entries = entries_of(storage.msg(7));
    let para_data = para_table(storage.msg(6));
    let para_starts = para_table(storage.msg(14));

    let mut footnotes: Vec<(u32, StyledText)> = Vec::new();

    // Paragraph boundaries: newlines in the character buffer (text.md
    // §Paragraph model). Each paragraph spans [start_char, end_char) in
    // char-index space; the trailing newline is not part of the paragraph.
    let mut para_ranges: Vec<(usize, usize)> = Vec::new();
    {
        let mut start_char = 0usize;
        for (ci, ch) in text.chars().enumerate() {
            if ch == '\n' {
                para_ranges.push((start_char, ci));
                start_char = ci + 1;
            }
        }
        para_ranges.push((start_char, text.chars().count()));
    }

    // Per-paragraph list info: (list style id, level, restart). An entry at
    // paragraph start applies from there until the next entry.
    let mut list_by_para: Vec<Option<(u64, u32, bool)>> = vec![None; para_ranges.len()];
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
            // Level + restart are PER-PARAGRAPH tables, read at each
            // paragraph start (G1: "Two"/"Three" continue the numbering
            // started at "One" — restart=true only at the entry paragraph).
            list_by_para[pi] = cur.map(|lid| {
                (lid, para_level(&para_data, p_start_u16), para_restart(&para_starts, p_start_u16))
            });
        }
    }

    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut last_style: Option<u64> = None;

    for (pi, (start, end)) in para_ranges.iter().enumerate() {
        let p_start_u16 = map[*start];
        let p_end_u16 = map[*end];

        // Paragraph style: entry exactly at the paragraph start sets/overrides
        // the style; a null object (or no entry) carries the previous
        // paragraph's style forward (docs/format/text.md §Paragraph model).
        let mut style_ref = last_style;
        if let Some(e) = entry_at(&para_entries, p_start_u16) {
            if e.utf16_off == p_start_u16 || style_ref.is_none() {
                style_ref = e.object_id;
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
        for (ci, ch) in text.chars().enumerate().skip(*start).take(end.saturating_sub(*start)) {
            if ch == '\u{FFFC}' {
                boundaries.push(map[ci]);
            }
        }
        boundaries.push(p_end_u16);
        boundaries.sort_unstable();
        boundaries.dedup();

        for w in boundaries.windows(2) {
            let (b0, b1) = (w[0], w[1]);
            if b1 <= b0 {
                continue;
            }
            // Effective char style: last char-style entry at or before b0.
            // A null object means "no override" → default style.
            let char_style = match entry_at(&char_entries, b0).and_then(|e| e.object_id) {
                Some(sid) => resolve_char_style(ctx, sid),
                None => CharStyle::default(),
            };
            let c_style = ctx.char_pool.intern(char_style);

            let seg_start_char = u16_to_char_index(&map, b0);
            let seg_end_char = u16_to_char_index(&map, b1);
            let seg: String = text
                .chars()
                .skip(seg_start_char)
                .take(seg_end_char - seg_start_char)
                .collect();

            // A segment starting at an attachment entry (its U+FFFC anchor
            // char) becomes an inline object / field run. The attachment
            // occupies exactly one char; remaining text in the segment falls
            // through to the text path below (G5: writers glue FFFC to body
            // words in the same run).
            let seg_starts_fffc = seg.starts_with('\u{FFFC}');
            let att_at_start = seg_starts_fffc
                .then(|| attach_entries.iter().find(|e| e.utf16_off == b0))
                .flatten();
            if let Some(att) = att_at_start {
                match resolve_attachment(
                    ctx,
                    att.object_id,
                    pi as u32,
                    &mut footnotes,
                ) {
                    AttachmentResult::Drawable(drawable, h_off, v_off) => {
                        let offset = if h_off.is_some() || v_off.is_some() {
                            Some(InlineOffset { h_pt: h_off, v_pt: v_off })
                        } else {
                            None
                        };
                        items.push(ParagraphItem::InlineObject {
                            kind: InlineObjectTag::InlineObject,
                            drawable,
                            offset,
                        });
                        // Remaining chars of the segment (if any) continue as
                        // a text run via the normal path below.
                        if seg.len() > 1 {
                            let rest: String = seg.chars().skip(1).collect();
                            items.push(ParagraphItem::Plain(rest));
                        }
                        continue;
                    }
                    AttachmentResult::Field { style, value, field } => {
                        items.push(ParagraphItem::Field {
                            kind: FieldTag::Field,
                            c_style: ctx.char_pool.intern(style),
                            value: value.or(Some(seg.chars().skip(1).collect())),
                            field,
                        });
                        continue;
                    }
                    AttachmentResult::None => {}
                }
            }

            // Smart fields overlapping this segment: date fields or hyperlinks.
            let mut hyperlink: Option<String> = None;
            let mut field_kind: Option<FieldKind> = None;
            for se in &smart_entries {
                if se.utf16_off >= b0 && se.utf16_off < b1 {
                    if let Some(sid) = se.object_id {
                        if let Some(sm) = ctx.loaded.msg(sid).cloned() {
                            // DateTimeSmartFieldArchive: update_plan = 6
                            if sm.has(6) && sm.string(1).is_none() {
                                let plan = match sm.varint(6).unwrap_or(1) {
                                    0 => DateUpdatePlan::Never,
                                    2 => DateUpdatePlan::Once,
                                    _ => DateUpdatePlan::Auto,
                                };
                                field_kind = Some(FieldKind::Date { update_plan: plan });
                            } else if sm.has(2) {
                                // HyperlinkFieldArchive: url_ref = 2. The URL
                                // itself usually lives in a referenced object
                                // (field 1 or 3 there); resolve best effort.
                                hyperlink = sm
                                    .reference(2)
                                    .and_then(|u| ctx.loaded.msg(u))
                                    .and_then(|um| um.string(1).or_else(|| um.string(3)));
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

        let mut pstyle = match style_ref {
            Some(sid) => resolve_para_style(ctx, sid),
            None => ParaStyle::default(),
        };
        // Overlay list membership/level/restart from the storage tables
        // (gotchas #14). The theme list style carries the marker vocabulary;
        // the storage tables carry WHO is in a list, at WHAT level, and
        // whether numbering restarts — decodable without theme resolution.
        if let Some((lid, level, restart)) = list_by_para[pi] {
            let mut lf = resolve_list_format_minimal(ctx, lid, level);
            if restart && lf.marker_kind == ListMarkerKind::Number {
                lf.start = Some(1.0);
            }
            pstyle.list = Some(lf);
        }
        let p_style = ctx.para_pool.intern(pstyle);
        paragraphs.push(Paragraph { p_style, items });
    }

    Some(ExtractedText { text: StyledText { paragraphs }, footnotes })
}

enum AttachmentResult {
    Drawable(Drawable, Option<f64>, Option<f64>),
    Field {
        style: CharStyle,
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
    footnotes: &mut Vec<(u32, StyledText)>,
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
                    AttachmentResult::Drawable(d, h, v)
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
                style: CharStyle::default(),
                value: att.string(1),
                field: kind,
            }
        }
        // TSWP.NumberAttachmentArchive { super = 1, string_value = 3 } and
        // TSWP.TSWPTOCPageNumberAttachmentArchive { super = 1, page_number = 2 }
        2043 | 2010 => {
            let value = rec
                .msg
                .as_ref()
                .and_then(|a| a.string(3).or_else(|| a.string(2)));
            AttachmentResult::Field {
                style: CharStyle::default(),
                value,
                field: FieldKind::PageNumber,
            }
        }
        // TSWP.FootnoteReferenceAttachmentArchive
        // { super = 1, contained_storage = 2 } — footnote body text.
        2008 => {
            let storage_id = rec.msg.as_ref().and_then(|a| a.reference(2));
            if let Some(sid) = storage_id {
                if let Some(ex) = extract(ctx, sid) {
                    footnotes.push((para_index, ex.text));
                }
            }
            AttachmentResult::Field {
                style: CharStyle::default(),
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
                Drawable::Unknown {
                    common: None,
                    type_id: format!("0x{:x}", rec.type_id),
                    type_name,
                    reason,
                },
                None,
                None,
            )
        }
    }
}

/// ParaDataAttributeTable (fields 6/14/24): entries {idx, first, second}.
fn para_table(table: Option<Msg>) -> Vec<(usize, u64, u64)> {
    let Some(table) = table else { return Vec::new() };
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

/// List RESTART flag: the last table_para_starts entry at or before the
/// paragraph start carries `.first` (1 = numbering restarts here).
fn para_restart(para_starts: &[(usize, u64, u64)], utf16_off: usize) -> bool {
    let mut restart = false;
    for (idx, first, _) in para_starts {
        if *idx <= utf16_off {
            restart = *first != 0;
        } else {
            break;
        }
    }
    restart
}
