//! Minimal property-list reader for the two metadata files an iWork document
//! carries: `Metadata/Properties.plist` (a binary plist: a dictionary of
//! strings, UUIDs and booleans) and `Metadata/BuildVersionHistory.plist`
//! (XML: an array of strings). It replaces the `plist` crate, which cost
//! 49 KB of wasm to read two files of about 300 bytes each.
//!
//! Coverage: every value kind the binary format defines is walked (so a
//! dictionary containing a date or a data blob still parses); the XML side
//! reads `array`, `dict`, `key`, `string`, `integer`, `real`, `true`,
//! `false`, `date` and `data`. Anything malformed returns `Err` with a short
//! reason; the caller turns that into a document warning.
//!
//! Binary layout [inferred from Apple's CFBinaryPList.c, public since 2005]:
//! `bplist00` header, objects, an offset table, and a 32-byte trailer that
//! holds the offset-int size, the object-ref size, the object count, the
//! top object index and the offset-table position.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Bool(bool),
    Array(Vec<Value>),
    Dict(BTreeMap<String, Value>),
    /// A real, date, data blob, UID or set: walked but not represented.
    Other,
}

impl Value {
    pub fn as_string(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_dict(&self) -> Option<&BTreeMap<String, Value>> {
        match self {
            Value::Dict(d) => Some(d),
            _ => None,
        }
    }
}

/// Parse a property list in either serialization.
pub fn parse(bytes: &[u8]) -> Result<Value, String> {
    if bytes.starts_with(b"bplist0") {
        Binary::new(bytes)?.top()
    } else {
        Xml::new(bytes).document()
    }
}

// ---------------------------------------------------------------------------
// Binary
// ---------------------------------------------------------------------------

const MAX_DEPTH: usize = 32;

struct Binary<'a> {
    b: &'a [u8],
    offset_size: usize,
    ref_size: usize,
    count: usize,
    top: usize,
    table: usize,
}

impl<'a> Binary<'a> {
    fn new(b: &'a [u8]) -> Result<Self, String> {
        if b.len() < 40 {
            return Err("binary plist too short".into());
        }
        let t = &b[b.len() - 32..];
        let offset_size = t[6] as usize;
        let ref_size = t[7] as usize;
        let count = be_u64(&t[8..16]) as usize;
        let top = be_u64(&t[16..24]) as usize;
        let table = be_u64(&t[24..32]) as usize;
        if !(1..=8).contains(&offset_size) || !(1..=8).contains(&ref_size) {
            return Err("binary plist: bad trailer sizes".into());
        }
        if top >= count || table.saturating_add(count.saturating_mul(offset_size)) > b.len() - 32 {
            return Err("binary plist: bad trailer".into());
        }
        Ok(Binary {
            b,
            offset_size,
            ref_size,
            count,
            top,
            table,
        })
    }

    fn top(&self) -> Result<Value, String> {
        self.object(self.top, 0)
    }

    fn offset_of(&self, index: usize) -> Result<usize, String> {
        if index >= self.count {
            return Err("binary plist: object index out of range".into());
        }
        let at = self.table + index * self.offset_size;
        let off = be_uint(&self.b[at..at + self.offset_size]) as usize;
        if off >= self.b.len() - 32 {
            return Err("binary plist: object offset out of range".into());
        }
        Ok(off)
    }

    fn take(&self, at: usize, n: usize) -> Result<&'a [u8], String> {
        self.b
            .get(at..at.checked_add(n).ok_or("binary plist: length overflow")?)
            .ok_or_else(|| "binary plist: object runs past the file".to_string())
    }

    /// The element count of a container or string: the marker's low nibble,
    /// or, when that is 0xF, an integer object that follows the marker.
    /// Returns (count, offset of the payload).
    fn count_after(&self, at: usize, info: u8) -> Result<(usize, usize), String> {
        if info != 0x0F {
            return Ok((info as usize, at + 1));
        }
        let m = *self.take(at + 1, 1)?.first().unwrap();
        if m >> 4 != 0x1 {
            return Err("binary plist: count is not an integer".into());
        }
        let n = 1usize << (m & 0x0F);
        if n > 8 {
            return Err("binary plist: count too wide".into());
        }
        let raw = self.take(at + 2, n)?;
        Ok((be_uint(raw) as usize, at + 2 + n))
    }

    fn object(&self, index: usize, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err("binary plist: nesting too deep".into());
        }
        let at = self.offset_of(index)?;
        let marker = self.b[at];
        let (kind, info) = (marker >> 4, marker & 0x0F);
        Ok(match kind {
            0x0 => match info {
                0x8 => Value::Bool(false),
                0x9 => Value::Bool(true),
                _ => Value::Other, // null, fill
            },
            0x1 => {
                let n = 1usize << info;
                if n > 16 {
                    return Err("binary plist: integer too wide".into());
                }
                let raw = self.take(at + 1, n)?;
                // 16-byte integers keep their low 8 bytes; iWork writes none.
                let low = &raw[raw.len().saturating_sub(8)..];
                let v = be_uint(low);
                // 1/2/4-byte integers are unsigned and fit; 8-byte ones are
                // two's complement, which the cast reproduces.
                Value::Integer(v as i64)
            }
            0x2 => {
                self.take(at + 1, 1usize << info)?;
                Value::Other
            }
            0x3 => {
                self.take(at + 1, 8)?;
                Value::Other
            }
            0x4 => {
                let (n, p) = self.count_after(at, info)?;
                self.take(p, n)?;
                Value::Other
            }
            0x5 => {
                let (n, p) = self.count_after(at, info)?;
                let raw = self.take(p, n)?;
                Value::String(raw.iter().map(|&c| c as char).collect())
            }
            0x6 => {
                let (n, p) = self.count_after(at, info)?;
                let raw = self.take(p, n.checked_mul(2).ok_or("binary plist: string too long")?)?;
                let units: Vec<u16> = raw
                    .chunks(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                Value::String(String::from_utf16_lossy(&units))
            }
            0x8 => {
                self.take(at + 1, info as usize + 1)?;
                Value::Other
            }
            0xA | 0xC => {
                let (n, p) = self.count_after(at, info)?;
                let refs = self.take(
                    p,
                    n.checked_mul(self.ref_size)
                        .ok_or("binary plist: array too long")?,
                )?;
                let mut items = Vec::with_capacity(n.min(1024));
                for r in refs.chunks(self.ref_size) {
                    items.push(self.object(be_uint(r) as usize, depth + 1)?);
                }
                if kind == 0xA {
                    Value::Array(items)
                } else {
                    Value::Other
                }
            }
            0xD => {
                let (n, p) = self.count_after(at, info)?;
                let span = n
                    .checked_mul(self.ref_size)
                    .ok_or("binary plist: dict too long")?;
                let keys = self.take(p, span)?;
                let vals = self.take(p + span, span)?;
                let mut d = BTreeMap::new();
                for (k, v) in keys.chunks(self.ref_size).zip(vals.chunks(self.ref_size)) {
                    let key = match self.object(be_uint(k) as usize, depth + 1)? {
                        Value::String(s) => s,
                        _ => return Err("binary plist: non-string dictionary key".into()),
                    };
                    d.insert(key, self.object(be_uint(v) as usize, depth + 1)?);
                }
                Value::Dict(d)
            }
            _ => return Err(format!("binary plist: unknown marker 0x{marker:02x}")),
        })
    }
}

fn be_u64(b: &[u8]) -> u64 {
    be_uint(b)
}

/// Big-endian unsigned integer of 1 to 8 bytes.
fn be_uint(b: &[u8]) -> u64 {
    b.iter().take(8).fold(0u64, |acc, &x| (acc << 8) | x as u64)
}

// ---------------------------------------------------------------------------
// XML
// ---------------------------------------------------------------------------

struct Xml<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Xml<'a> {
    fn new(s: &'a [u8]) -> Self {
        // A UTF-8 BOM is allowed ahead of the declaration.
        let i = if s.starts_with(&[0xEF, 0xBB, 0xBF]) {
            3
        } else {
            0
        };
        Xml { s, i }
    }

    fn document(&mut self) -> Result<Value, String> {
        self.skip_misc();
        if !self.open_tag_is("plist") {
            return Err("xml plist: no <plist> element".into());
        }
        self.skip_tag()?;
        self.skip_misc();
        let v = self.value(0)?;
        Ok(v)
    }

    /// Skip whitespace, the XML declaration, DOCTYPE and comments.
    fn skip_misc(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            let rest = &self.s[self.i..];
            if rest.starts_with(b"<?") {
                self.i = find(self.s, self.i, b"?>")
                    .map(|p| p + 2)
                    .unwrap_or(self.s.len());
            } else if rest.starts_with(b"<!--") {
                self.i = find(self.s, self.i, b"-->")
                    .map(|p| p + 3)
                    .unwrap_or(self.s.len());
            } else if rest.starts_with(b"<!") {
                self.i = find(self.s, self.i, b">")
                    .map(|p| p + 1)
                    .unwrap_or(self.s.len());
            } else {
                return;
            }
        }
    }

    fn open_tag_is(&self, name: &str) -> bool {
        let rest = &self.s[self.i..];
        rest.starts_with(b"<")
            && rest[1..].starts_with(name.as_bytes())
            && matches!(
                rest.get(1 + name.len()),
                Some(b' ') | Some(b'>') | Some(b'/') | Some(b'\n') | Some(b'\t')
            )
    }

    /// The tag name at the cursor (after `<` or `</`), or None.
    fn tag_name(&self) -> Option<&'a str> {
        let rest = &self.s[self.i..];
        if !rest.starts_with(b"<") {
            return None;
        }
        let start = if rest.starts_with(b"</") { 2 } else { 1 };
        let end = rest[start..]
            .iter()
            .position(|&c| c == b'>' || c == b'/' || c.is_ascii_whitespace())
            .map(|p| start + p)?;
        std::str::from_utf8(&rest[start..end]).ok()
    }

    /// Skip one tag (`<x ...>`, `</x>` or `<x/>`); reports whether it was
    /// self-closing.
    fn skip_tag(&mut self) -> Result<bool, String> {
        let end = find(self.s, self.i, b">").ok_or("xml plist: unterminated tag")?;
        let self_closing = self.s[end - 1] == b'/';
        self.i = end + 1;
        Ok(self_closing)
    }

    /// Text up to the next `<`, entities decoded.
    fn text(&mut self) -> Result<String, String> {
        let end = self.s[self.i..]
            .iter()
            .position(|&c| c == b'<')
            .map(|p| self.i + p)
            .ok_or("xml plist: unterminated text")?;
        let raw = std::str::from_utf8(&self.s[self.i..end])
            .map_err(|_| "xml plist: text is not UTF-8")?;
        self.i = end;
        Ok(decode_entities(raw))
    }

    fn expect_close(&mut self, name: &str) -> Result<(), String> {
        self.skip_misc();
        if self.s[self.i..].starts_with(b"</") && self.tag_name() == Some(name) {
            self.skip_tag()?;
            Ok(())
        } else {
            Err(format!("xml plist: expected </{name}>"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, String> {
        if depth > MAX_DEPTH {
            return Err("xml plist: nesting too deep".into());
        }
        self.skip_misc();
        let name = self.tag_name().ok_or("xml plist: expected an element")?;
        match name {
            "array" => {
                if self.skip_tag()? {
                    return Ok(Value::Array(Vec::new()));
                }
                let mut items = Vec::new();
                loop {
                    self.skip_misc();
                    if self.s[self.i..].starts_with(b"</") {
                        break;
                    }
                    items.push(self.value(depth + 1)?);
                }
                self.expect_close("array")?;
                Ok(Value::Array(items))
            }
            "dict" => {
                if self.skip_tag()? {
                    return Ok(Value::Dict(BTreeMap::new()));
                }
                let mut d = BTreeMap::new();
                loop {
                    self.skip_misc();
                    if self.s[self.i..].starts_with(b"</") {
                        break;
                    }
                    if self.tag_name() != Some("key") {
                        return Err("xml plist: expected <key>".into());
                    }
                    let key = if self.skip_tag()? {
                        String::new()
                    } else {
                        let k = self.text()?;
                        self.expect_close("key")?;
                        k
                    };
                    d.insert(key, self.value(depth + 1)?);
                }
                self.expect_close("dict")?;
                Ok(Value::Dict(d))
            }
            "string" | "date" | "data" => {
                let kind = name;
                if self.skip_tag()? {
                    return Ok(if kind == "string" {
                        Value::String(String::new())
                    } else {
                        Value::Other
                    });
                }
                let t = self.text()?;
                self.expect_close(kind)?;
                Ok(if kind == "string" {
                    Value::String(t)
                } else {
                    Value::Other
                })
            }
            "integer" | "real" => {
                let kind = name;
                if self.skip_tag()? {
                    return Ok(Value::Other);
                }
                let t = self.text()?;
                self.expect_close(kind)?;
                Ok(match (kind, t.trim().parse::<i64>()) {
                    ("integer", Ok(n)) => Value::Integer(n),
                    _ => Value::Other,
                })
            }
            "true" | "false" => {
                let v = name == "true";
                if !self.skip_tag()? {
                    self.expect_close(if v { "true" } else { "false" })?;
                }
                Ok(Value::Bool(v))
            }
            other => Err(format!("xml plist: unexpected <{other}>")),
        }
    }
}

fn find(s: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    s[from..]
        .windows(pat.len())
        .position(|w| w == pat)
        .map(|p| from + p)
}

fn decode_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let ent = &rest[1..end];
        match ent {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let code = ent
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()));
                match code.and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push_str(&rest[..=end]),
                }
            }
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_array_of_strings() {
        let src = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<array>\n\t<string>M13.1-7020-1</string>\n\t<string>Template: Basic &amp; White (15.3)</string>\n</array>\n</plist>\n";
        let v = parse(src).unwrap();
        let a = v.as_array().unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a[1].as_string(), Some("Template: Basic & White (15.3)"));
    }

    #[test]
    fn xml_dict_with_scalars() {
        let src = b"<plist version=\"1.0\"><dict><key>Version</key><string>15.3.1</string><key>Build</key><string>7020</string><key>n</key><integer>42</integer><key>ok</key><true/><key>when</key><date>2026-09-05T00:00:00Z</date><key>empty</key><array/></dict></plist>";
        let v = parse(src).unwrap();
        let d = v.as_dict().unwrap();
        assert_eq!(d["Version"].as_string(), Some("15.3.1"));
        assert_eq!(d["n"], Value::Integer(42));
        assert_eq!(d["ok"], Value::Bool(true));
        assert_eq!(d["when"], Value::Other);
        assert_eq!(d["empty"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse(b"\xa5\xbd\x92\xac\x9bZ:;L1.\xf4\x9du").is_err());
        assert!(parse(b"bplist00\x00\x00\x00").is_err());
        assert!(parse(b"<plist><dict><string>no key</string></dict></plist>").is_err());
    }

    #[test]
    fn binary_dict() {
        // plistlib.dumps({"fileFormatVersion": "15.3", "isMultiPage": False,
        //   "revision": "1", "shareUUID": "9E8F-é", "n": 300}, fmt=FMT_BINARY)
        let bytes = include_bytes!("../tests/data/properties.bplist");
        let v = parse(bytes).unwrap();
        let d = v.as_dict().unwrap();
        assert_eq!(d["fileFormatVersion"].as_string(), Some("15.3"));
        assert_eq!(d["isMultiPage"], Value::Bool(false));
        assert_eq!(d["revision"].as_string(), Some("1"));
        assert_eq!(d["shareUUID"].as_string(), Some("9E8F-\u{e9}"));
        assert_eq!(d["n"], Value::Integer(300));
    }
}
