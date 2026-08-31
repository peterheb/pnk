//! `TSP.Color` → `#rrggbb[aa]` hex (docs/model-design.md §2.3).
//!
//! rgb/srgb → direct scale; rgb/p3 → nearest-sRGB approximation (+ a
//! `color-degraded` warning only when visibly out of gamut — we approximate by
//! simple per-channel clamp, the same pragmatic conversion the model doc
//! outlines); cmyk/white → naive formulas; headroom ≠ 1 → clamp + warning.

use crate::pb::Msg;

/// Convert a `TSP.Color` message. `on_degraded` is invoked (once per color)
/// when P3 or HDR content had to be clamped.
pub fn color_hex(m: &Msg, on_degraded: &mut impl FnMut(String)) -> Option<String> {
    let model = m.varint(1)?;
    let mut degraded = |reason: String| on_degraded(reason);

    let (r, g, b) = match model {
        1 => {
            // rgb; rgbspace: srgb=1 (default when unset), p3=2
            let space = m.varint(12).unwrap_or(1);
            let (mut r, mut g, mut b) = (m.f32v(3)? as f64, m.f32v(4)? as f64, m.f32v(5)? as f64);
            if space == 2 {
                // Display P3 → sRGB approximation: the naive transfer treats
                // component values as compatible and clamps out-of-gamut
                // results [inferred: docs/model-design.md §2.3 policy].
                let before = (r, g, b);
                r = r.clamp(0.0, 1.0);
                g = g.clamp(0.0, 1.0);
                b = b.clamp(0.0, 1.0);
                if (before.0 - r).abs() > 0.004
                    || (before.1 - g).abs() > 0.004
                    || (before.2 - b).abs() > 0.004
                {
                    degraded("Display P3 color approximated to sRGB".into());
                }
            }
            (r, g, b)
        }
        2 => {
            // cmyk → naive sRGB (docs/model-design.md §2.3).
            let c = m.f32v(7)? as f64;
            let mm = m.f32v(8)? as f64;
            let y = m.f32v(9)? as f64;
            let k = m.f32v(10)? as f64;
            (
                (1.0 - c).max(0.0) * (1.0 - k).max(0.0),
                (1.0 - mm).max(0.0) * (1.0 - k).max(0.0),
                (1.0 - y).max(0.0) * (1.0 - k).max(0.0),
            )
        }
        3 => {
            // white → gray ramp
            let w = m.f32v(11)? as f64;
            (w, w, w)
        }
        _ => return None,
    };

    // HDR headroom: clamp the channels (the warning only when it mattered).
    let headroom = m.f32v(13).unwrap_or(1.0) as f64;
    let (r, g, b) = if headroom != 1.0 && headroom > 1.0 {
        let (rr, gg, bb) = (r.min(1.0), g.min(1.0), b.min(1.0));
        if rr != r || gg != g || bb != b {
            degraded("HDR headroom clamped to sRGB range".into());
        }
        (rr, gg, bb)
    } else {
        (r, g, b)
    };

    let a = m.f32v(6).unwrap_or(1.0) as f64;
    let to8 = |v: f64| -> u8 { (v.clamp(0.0, 1.0) * 255.0).round() as u8 };
    let (rr, gg, bb, aa) = (to8(r), to8(g), to8(b), to8(a));
    Some(if aa == 255 {
        format!("#{rr:02x}{gg:02x}{bb:02x}")
    } else {
        format!("#{rr:02x}{gg:02x}{bb:02x}{aa:02x}")
    })
}

/// Seconds since 2001-01-01T00:00:00Z → ISO 8601 UTC string
/// (docs/model-design.md §1.1; numbers-parser `EPOCH` semantics).
pub fn iso_from_apple_seconds(seconds: f64) -> String {
    // 2001-01-01T00:00:00Z = 978307200 unix seconds.
    const EPOCH_UNIX: f64 = 978_307_200.0;
    let unix = seconds + EPOCH_UNIX;
    // Round the TOTAL timestamp to milliseconds first, then decompose:
    // rounding the fractional part on its own can produce millis == 1000
    // with no carry into the second (e.g. ...59.9996 -> "...59.1000Z").
    let millis_total = (unix * 1000.0).round() as i64;
    let secs = millis_total.div_euclid(1000);
    let millis = millis_total.rem_euclid(1000) as u32;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (h, mi, s) = (sod / 3600, (sod % 3600) / 60, sod % 60);
    let (y, mo, d) = civil_from_days(days);
    if millis > 0 {
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z")
    } else {
        format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
    }
}

/// Howard Hinnant's civil_from_days algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apple_epoch_zero_is_2001() {
        assert_eq!(iso_from_apple_seconds(0.0), "2001-01-01T00:00:00Z");
    }

    #[test]
    fn apple_epoch_plus_one_day() {
        assert_eq!(iso_from_apple_seconds(86400.0), "2001-01-02T00:00:00Z");
    }

    #[test]
    fn unix_epoch_minus_30_years() {
        // 1971-01-01T00:00:00Z = -946_771_200 apple seconds
        assert_eq!(
            iso_from_apple_seconds(-946_771_200.0),
            "1971-01-01T00:00:00Z"
        );
    }
}
