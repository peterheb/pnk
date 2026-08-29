// Decode the numbered list levels: paras 5/6/7 are "One/Two/Three" (numbered).
// Numbered segment: ONE table entry at para 5 with label=number.
// Levels for Bullet L1/L2: table_para_data(6) @175 (para 9) first=1 — the
// para at idx 175 has first=1 → LEVEL 1 (0-based) for "Bullet Level 2"?
// para 8 "Bullet Level 1" has NO para_data entry → level 0. first=1 at para 9
// = level 1 → renders as level-2 bullet. CONSISTENT with "Bullet Level 2"!
// For numbered: para_starts(14) @146 first=1 (restart numbering at para 5),
// @150 first=0 (continue at para 6), entry 160 first=1 = bullets restart?
// Final check: what makes One=1, Two=2, Three=3 (continuing numbering)?
fn main() {
    println!("MODEL: table_list_style(7) = paragraph→list-style ranges");
    println!("  label: 0=none 2=string(bullet) 3=number");
    println!("  numbering continues across paragraphs until a new entry;");
    println!("table_para_starts(14).first = list RESTART flag (1 = numbering restarts at this para)");
    println!("table_para_data(6).first = LIST LEVEL (0-based) for this para");
}
