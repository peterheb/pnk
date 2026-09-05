//! TSD visual payload decoders — fills, strokes, shadows, reflections, line
//! ends, `TSP.Path` curves, geometry, and the six PathSourceArchive variants
//! flattened per docs/format/drawables.md + docs/model-design.md §2.5.

use crate::ctx::Ctx;
use crate::model::*;
use crate::pb::Msg;

/// Convert an inline TSP.Color field, routing degradation warnings to the ctx.
pub fn color_of(ctx: &mut Ctx, m: &Msg, field: u32) -> Option<String> {
    let c = m.msg(field)?;
    let mut warnings: Vec<String> = Vec::new();
    let hex = crate::colors::color_hex(&c, &mut |reason| warnings.push(reason));
    for w in warnings {
        ctx.warn(WarningCode::ColorDegraded, w);
    }
    hex
}

// ---------------------------------------------------------------------------
// TSP.Path → CurvePath (TSPMessages.proto:103-117)
// ---------------------------------------------------------------------------

pub fn tsp_path(m: &Msg) -> Option<CurvePath> {
    let mut elements = Vec::new();
    for el in m.msgs(1) {
        let ty = el.varint(1)?;
        let points: Vec<f64> = el
            .msgs(2)
            .into_iter()
            .filter_map(|p| Some([p.f32v(1)? as f64, p.f32v(2)? as f64]))
            .flatten()
            .collect();
        elements.push(match ty {
            1 => CurveElement::Move { points },
            2 => CurveElement::Line { points },
            3 => CurveElement::Quad { points },
            4 => CurveElement::Cubic { points },
            5 => CurveElement::Close { points },
            _ => continue,
        });
    }
    Some(CurvePath { elements })
}

/// Editable bezier subpaths → cubic curves (drawables.md PathSourceArchive 8).
/// `Node` carries inControlPoint(1) / nodePoint(2) / outControlPoint(3) /
/// NodeType(4: sharp=1, bezier=2, smooth=3); consecutive nodes A→B are cubic
/// segments [A.out, B.in, B.node].
fn editable_bezier(m: &Msg, natural: Option<Size>) -> ShapeGeometry {
    let mut elements = Vec::new();
    for subpath in m.msgs(1) {
        let nodes: Vec<Msg> = subpath.msgs(1);
        let closed = subpath.boolean(2).unwrap_or(false);
        let pts: Vec<(f64, f64)> = nodes
            .iter()
            .filter_map(|n| {
                let p = n.msg(2)?;
                Some((p.f32v(1)? as f64, p.f32v(2)? as f64))
            })
            .collect();
        let mut first = true;
        for (i, node) in nodes.iter().enumerate() {
            let Some(cur) = pts.get(i) else { continue };
            if first {
                elements.push(CurveElement::Move {
                    points: vec![cur.0, cur.1],
                });
                first = false;
            }
            let Some(next) = pts.get(i + 1) else { continue };
            let next_pt = [next.0, next.1];
            let node_type = node.varint(4).unwrap_or(1);
            let out = node
                .msg(3)
                .and_then(|m| Some((m.f32v(1)? as f64, m.f32v(2)? as f64)));
            let next_in = nodes[i + 1]
                .msg(1)
                .and_then(|m| Some((m.f32v(1)? as f64, m.f32v(2)? as f64)));
            match (node_type, out, next_in) {
                // Sharp nodes carry no real curvature: straight line.
                (1, _, _) => elements.push(CurveElement::Line {
                    points: next_pt.to_vec(),
                }),
                (_, Some(o), Some(nin)) => elements.push(CurveElement::Cubic {
                    points: vec![o.0, o.1, nin.0, nin.1, next_pt[0], next_pt[1]],
                }),
                (_, Some(o), None) => elements.push(CurveElement::Quad {
                    points: vec![o.0, o.1, next_pt[0], next_pt[1]],
                }),
                _ => elements.push(CurveElement::Line {
                    points: next_pt.to_vec(),
                }),
            }
        }
        if closed {
            if let Some(start) = pts.first() {
                elements.push(CurveElement::Line {
                    points: vec![start.0, start.1],
                });
            }
            elements.push(CurveElement::Close { points: vec![] });
        }
    }
    // Editable-bezier nodes are unit-less too: Keynote stores e.g. the
    // canonical 141.42-long line against a 46.8pt naturalSize (24_Briefing
    // master ticks) — fit tight bounds to naturalSize exactly like TSP.Path.
    // Paths authored at natural scale come through unchanged (scale ≈ 1,
    // G5 acid line).
    let path = normalize_path(CurvePath { elements }, natural.as_ref());
    ShapeGeometry {
        preset: None,
        scalar: None,
        natural_size: natural,
        path: Some(path),
        callout: None,
        point: None,
    }
}

// ---------------------------------------------------------------------------
// ShapeGeometry — the six PathSourceArchive variants, priority per §2.5
// ---------------------------------------------------------------------------

/// Fit a `TSP.Path`'s tight bounds onto the shape's naturalSize with a
/// PER-AXIS stretch. The stored coordinates carry no absolute unit; Apple
/// maps the path's own bounds onto the shape box axis by axis
/// (fixture-verified 2026-08-30: 388ca218's white caption band stores a
/// 100x100 canonical square against naturalSize 1023.5x216.5 and Apple
/// draws the full-width band — the earlier uniform-scale-and-center read
/// shrank it to a centered 216pt square; the ppd badge circle that
/// motivated uniform scaling sits in a square box, where both readings
/// agree). A degenerate axis (0-height rules) borrows the other axis'
/// ratio, keeping the 24_Briefing canonical-141.42 line at 46.8pt.
fn normalize_path(mut p: CurvePath, natural: Option<&Size>) -> CurvePath {
    let Some(n) = natural else { return p };
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for el in &p.elements {
        let pts: &[f64] = match el {
            CurveElement::Move { points }
            | CurveElement::Line { points }
            | CurveElement::Quad { points }
            | CurveElement::Cubic { points }
            | CurveElement::Close { points } => points,
        };
        for xy in pts.as_chunks::<2>().0 {
            min.0 = min.0.min(xy[0]);
            min.1 = min.1.min(xy[1]);
            max.0 = max.0.max(xy[0]);
            max.1 = max.1.max(xy[1]);
        }
    }
    if !min.0.is_finite() {
        return p;
    }
    let bw = max.0 - min.0;
    let bh = max.1 - min.1;
    let eps = 1e-6;
    let rx = if n.width > eps && bw > eps {
        Some(n.width / bw)
    } else {
        None
    };
    let ry = if n.height > eps && bh > eps {
        Some(n.height / bh)
    } else {
        None
    };
    // A degenerate axis (0-height rules, 0-width vertical rules) contributes
    // no usable ratio — borrow the other axis' scale (24_Briefing rules:
    // canonical 141.42 line, naturalSize 46.8x0 → 46.8pt long). Both
    // degenerate: keep the path as-is.
    let (Some(sx), Some(sy)) = (rx.or(ry), ry.or(rx)) else {
        return p;
    };
    for el in p.elements.iter_mut() {
        let pts: &mut Vec<f64> = match el {
            CurveElement::Move { points }
            | CurveElement::Line { points }
            | CurveElement::Quad { points }
            | CurveElement::Cubic { points }
            | CurveElement::Close { points } => points,
        };
        for xy in pts.as_chunks_mut::<2>().0 {
            xy[0] = (xy[0] - min.0) * sx;
            xy[1] = (xy[1] - min.1) * sy;
        }
    }
    p
}

pub fn shape_geometry(pathsource: &Msg) -> ShapeGeometry {
    // 1. explicit bezier / editable bezier
    if let Some(b) = pathsource.msg(5) {
        let natural = b.size(2).map(|(w, h)| Size {
            width: w,
            height: h,
        });
        if let Some(p) = b.msg(3).as_ref().and_then(tsp_path) {
            // TSP.Path coordinates are unit-less (fixture-verified: the ppd
            // badge circle is r=35.36 units in a 285.77pt box; Apple fits the
            // path's tight bounds to naturalSize). Normalize so the emitted
            // path lives in the shape's point space; paths already authored at
            // natural scale come through unchanged (scale ≈ 1).
            let p = normalize_path(p, natural.as_ref());
            return ShapeGeometry {
                preset: None,
                scalar: None,
                natural_size: natural,
                path: Some(p),
                callout: None,
                point: None,
            };
        }
        // deprecated path_string: opaque, fall through
    }
    if let Some(e) = pathsource.msg(8) {
        let natural = e.size(2).map(|(w, h)| Size {
            width: w,
            height: h,
        });
        return editable_bezier(&e, natural);
    }
    // 2. callout
    if let Some(c) = pathsource.msg(6) {
        let natural = c.size(1).map(|(w, h)| Size {
            width: w,
            height: h,
        });
        let tail_position = c
            .point(2)
            .map(|(x, y)| Point { x, y })
            .unwrap_or(Point { x: 0.0, y: 0.0 });
        let tail_size_f = c.f32v(3).unwrap_or(0.0) as f64;
        return ShapeGeometry {
            preset: Some("callout".into()),
            scalar: None,
            natural_size: natural,
            path: None,
            callout: Some(CalloutParams {
                tail_position,
                tail_size: Size {
                    width: tail_size_f,
                    height: tail_size_f,
                },
                corner_radius: c.f32v(4).map(|v| v as f64),
                center_tail: c.boolean(5),
            }),
            point: None,
        };
    }
    // 3. scalar presets
    if let Some(s) = pathsource.msg(4) {
        let preset = match s.varint(1) {
            Some(0) => Some("rounded-rect".to_string()),
            Some(1) => Some("regular-polygon".to_string()),
            Some(2) => Some("chevron".to_string()),
            Some(other) => Some(format!("scalar-{other}")),
            None => None,
        };
        let natural = s.size(3).map(|(w, h)| Size {
            width: w,
            height: h,
        });
        return ShapeGeometry {
            preset,
            scalar: s.f32v(2).map(|v| v as f64),
            natural_size: natural,
            path: None,
            callout: None,
            point: None,
        };
    }
    // 4. point presets (arrows/star/plus)
    if let Some(p) = pathsource.msg(3) {
        let preset = match p.varint(1) {
            Some(0) => Some("left-arrow".to_string()),
            Some(1) => Some("right-arrow".to_string()),
            Some(10) => Some("double-arrow".to_string()),
            Some(100) => Some("star".to_string()),
            Some(200) => Some("plus".to_string()),
            Some(other) => Some(format!("point-{other}")),
            None => None,
        };
        let natural = p.size(3).map(|(w, h)| Size {
            width: w,
            height: h,
        });
        let point = p.point(2).map(|(x, y)| Point { x, y });
        return ShapeGeometry {
            preset,
            scalar: None,
            natural_size: natural,
            path: None,
            callout: None,
            point,
        };
    }
    ShapeGeometry {
        preset: None,
        scalar: None,
        natural_size: None,
        path: None,
        callout: None,
        point: None,
    }
}

// ---------------------------------------------------------------------------
// Fill / Stroke / Shadow / Reflection / LineEnd
// ---------------------------------------------------------------------------

pub fn fill_of(ctx: &mut Ctx, m: &Msg) -> Option<Fill> {
    if let Some(c) = m.msg(1) {
        let mut warnings = Vec::new();
        if let Some(hex) = crate::colors::color_hex(&c, &mut |r| warnings.push(r)) {
            for w in warnings {
                ctx.warn(WarningCode::ColorDegraded, w);
            }
            return Some(Fill::Solid { color: hex });
        }
    }
    if let Some(g) = m.msg(2) {
        return gradient_fill(ctx, &g);
    }
    if let Some(img) = m.msg(3) {
        // imagedata = 6 (DataReference); database_imagedata = 1 (legacy ref)
        let data_id = img.reference(6).or_else(|| img.reference(1))?;
        let image = ctx.media_ref(data_id);
        let technique = match img.varint(2).unwrap_or(0) {
            0 => ImageFillTechnique::NaturalSize,
            1 => ImageFillTechnique::Stretch,
            2 => ImageFillTechnique::Tile,
            3 => ImageFillTechnique::ScaleToFill,
            _ => ImageFillTechnique::ScaleToFit,
        };
        let tint = color_of(ctx, &img, 3);
        return Some(Fill::Image {
            image,
            technique,
            tint,
        });
    }
    None
}

fn gradient_fill(ctx: &mut Ctx, g: &Msg) -> Option<Fill> {
    let kind = match g.varint(1).unwrap_or(0) {
        0 => GradientKind::Linear,
        _ => GradientKind::Radial,
    };
    let mut stops = Vec::new();
    for s in g.msgs(2) {
        let color = color_of(ctx, &s, 1)?;
        stops.push(GradientStop {
            color,
            fraction: s.f32v(2).unwrap_or(0.0) as f64,
            inflection: s.f32v(3).map(|v| v as f64),
        });
    }
    if stops.is_empty() {
        return None;
    }
    let mut gradient = Gradient {
        kind,
        stops,
        angle_deg: None,
        start_point: None,
        end_point: None,
    };
    if let Some(a) = g.msg(5) {
        // TSD.AngleGradientArchive.gradientangle is RADIANS (fixture-verified:
        // 22_ColorGradient.key stores exactly 3π/2 = 4.71239 for its
        // top-to-bottom backdrop) — unlike TSD.Geometry.angle, which is
        // degrees. The model field is degrees.
        gradient.angle_deg = a.f32v(2).map(|v| (v as f64).to_degrees());
    }
    if let Some(t) = g.msg(6) {
        gradient.start_point = t.point(1).map(|(x, y)| Point { x, y });
        gradient.end_point = t.point(2).map(|(x, y)| Point { x, y });
    }
    Some(Fill::Gradient { gradient })
}

pub fn stroke_of(ctx: &mut Ctx, m: &Msg) -> Option<Stroke> {
    // StrokePatternArchive.type = 2 (TSDEmptyPattern) means NO stroke at all
    // — theme preset styles (G2 "captions-0-shapestyle" textbox presets) carry
    // a full 1pt black stroke with an empty pattern; Apple draws no border.
    if m.msg(6).and_then(|p| p.varint(1)) == Some(2) {
        return None;
    }
    let color = color_of(ctx, m, 1).unwrap_or_else(|| "#000000".to_string());
    let cap = match m.varint(3).unwrap_or(0) {
        1 => StrokeCap::Round,
        2 => StrokeCap::Square,
        _ => StrokeCap::Butt,
    };
    let join = match m.varint(4).unwrap_or(0) {
        1 => StrokeJoin::Round,
        2 => StrokeJoin::Bevel,
        _ => StrokeJoin::Miter,
    };
    let width_pt = m.f32v(2).unwrap_or(1.0) as f64;
    let (dash, dash_phase) = match m.msg(6) {
        Some(p) => {
            // Pattern entries are multiples of the stroke width, not points:
            // Keynote's "dotted" preset stores [1, 1] on a 2pt stroke and its
            // PDF export draws 2pt dashes with 2pt gaps (kcsrk deck, slide 8,
            // measured 2.4/1.44pt on/off at 150dpi with anti-aliasing). The
            // archive pads the array to six entries; `count` (field 3) says
            // how many are real.
            let mut dash: Vec<f64> = p.packed_f32s(4).into_iter().map(|v| v as f64).collect();
            if let Some(n) = p.varint(3).map(|n| n as usize) {
                if n > 0 && n < dash.len() {
                    dash.truncate(n);
                }
            }
            // All-zero patterns are Apple's "solid" placeholder, not a real
            // dash — emitting them renders invisible zero-length dashes.
            let dash = if p.varint(1) == Some(2) || !dash.iter().any(|d| *d > 0.0) {
                None
            } else {
                Some(dash.into_iter().map(|d| d * width_pt).collect())
            };
            (dash, p.f32v(2).map(|v| v as f64 * width_pt))
        }
        None => (None, None),
    };
    // Picture frame (field 8, TSD.FrameArchive): the frame asset replaces
    // the plain stroke look — 10a06959's "Formal Shadow" textbox stores a
    // 2pt black stroke underneath, and Pages draws a white mat + shadow.
    let frame = m.msg(8).and_then(|f| {
        let name = f.string(2)?;
        (!name.is_empty()).then(|| StrokeFrame {
            name,
            asset_scale: f.f32v(3).map(|v| v as f64),
        })
    });
    Some(Stroke {
        color,
        width_pt,
        cap,
        join,
        miter_limit: m.f32v(5).map(|v| v as f64),
        dash,
        dash_phase,
        frame,
    })
}

pub fn shadow_of(ctx: &mut Ctx, m: &Msg) -> Option<Shadow> {
    // is_enabled = 6 [default = true] (TSDArchives.proto:229): preset styles
    // ship a fully-parameterized shadow with is_enabled=0 — not a shadow.
    if m.boolean(6) == Some(false) {
        return None;
    }
    let color = color_of(ctx, m, 1)?;
    let kind = match m.varint(7).unwrap_or(0) {
        1 => ShadowKind::Contact,
        2 => ShadowKind::Curved,
        _ => ShadowKind::Drop,
    };
    let contact = m.msg(9).map(|c| ContactShadow {
        height: c.f32v(2).map(|v| v as f64),
        offset: c.f32v(4).map(|v| v as f64),
    });
    let curved = m.msg(10).map(|c| CurvedShadow {
        curve: c.f32v(1).map(|v| v as f64),
    });
    Some(Shadow {
        color,
        angle_deg: m.f32v(2).unwrap_or(315.0) as f64,
        offset_pt: m.f32v(3).unwrap_or(5.0) as f64,
        radius_pt: m.int(4).unwrap_or(1) as f64,
        opacity: m.f32v(5).unwrap_or(1.0) as f64,
        kind,
        contact,
        curved,
    })
}

pub fn reflection_of(m: &Msg) -> Option<Reflection> {
    // Preset styles carry an EMPTY ReflectionArchive for "no reflection";
    // a real reflection stores its opacity explicitly even at the 0.5
    // default (fixture: G2 pentagon writes {1: 0.5}, the caption presets
    // write a 0-byte message and Apple paints nothing).
    Some(Reflection {
        opacity: m.f32v(1)? as f64,
    })
}

pub fn line_end_of(_ctx: &mut Ctx, m: &Msg) -> Option<LineEnd> {
    let identifier = m.string(5);
    // identifier "none" is Apple's explicit no-decoration preset — it still
    // carries an (empty) path message, and Apple draws nothing (fixture:
    // cdx-00243-21 stores tail={identifier:"none", path:[]} on all 124 arrow
    // lines; its PDF export decorates only the heads).
    if identifier.as_deref() == Some("none") {
        return None;
    }
    let path = m
        .msg(1)
        .as_ref()
        .and_then(tsp_path)
        .filter(|p| !p.elements.is_empty());
    let le = LineEnd {
        identifier,
        is_filled: m.boolean(4),
        path,
    };
    // Empty archive (preset "no line end"): nothing to draw.
    if le.identifier.is_none() && le.is_filled.is_none() && le.path.is_none() {
        return None;
    }
    Some(le)
}
