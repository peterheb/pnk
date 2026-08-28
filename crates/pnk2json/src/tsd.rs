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
        let points: Vec<Point> = el
            .msgs(2)
            .into_iter()
            .filter_map(|p| {
                let (x, y) = (p.f32v(1)? as f64, p.f32v(2)? as f64);
                Some(Point { x, y })
            })
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
                elements.push(CurveElement::Move { points: vec![Point { x: cur.0, y: cur.1 }] });
                first = false;
            }
            let Some(next) = pts.get(i + 1) else { continue };
            let next_pt = Point { x: next.0, y: next.1 };
            let node_type = node.varint(4).unwrap_or(1);
            let out = node.msg(3).and_then(|m| Some((m.f32v(1)? as f64, m.f32v(2)? as f64)));
            let next_in = nodes[i + 1]
                .msg(1)
                .and_then(|m| Some((m.f32v(1)? as f64, m.f32v(2)? as f64)));
            match (node_type, out, next_in) {
                // Sharp nodes carry no real curvature: straight line.
                (1, _, _) => elements.push(CurveElement::Line { points: vec![next_pt] }),
                (_, Some(o), Some(nin)) => elements.push(CurveElement::Cubic {
                    points: vec![Point { x: o.0, y: o.1 }, Point { x: nin.0, y: nin.1 }, next_pt],
                }),
                (_, Some(o), None) => elements.push(CurveElement::Quad {
                    points: vec![Point { x: o.0, y: o.1 }, next_pt],
                }),
                _ => elements.push(CurveElement::Line { points: vec![next_pt] }),
            }
        }
        if closed {
            if let Some(start) = pts.first() {
                elements.push(CurveElement::Line { points: vec![Point { x: start.0, y: start.1 }] });
            }
            elements.push(CurveElement::Close { points: vec![] });
        }
    }
    ShapeGeometry {
        preset: None,
        scalar: None,
        natural_size: natural,
        path: Some(CurvePath { elements }),
        callout: None,
    }
}

// ---------------------------------------------------------------------------
// ShapeGeometry — the six PathSourceArchive variants, priority per §2.5
// ---------------------------------------------------------------------------

pub fn shape_geometry(pathsource: &Msg) -> ShapeGeometry {
    // 1. explicit bezier / editable bezier
    if let Some(b) = pathsource.msg(5) {
        let natural = b.size(2).map(|(w, h)| Size { width: w, height: h });
        if let Some(p) = b.msg(3).as_ref().and_then(tsp_path) {
            return ShapeGeometry {
                preset: None,
                scalar: None,
                natural_size: natural,
                path: Some(p),
                callout: None,
            };
        }
        // deprecated path_string: opaque, fall through
    }
    if let Some(e) = pathsource.msg(8) {
        let natural = e.size(2).map(|(w, h)| Size { width: w, height: h });
        return editable_bezier(&e, natural);
    }
    // 2. callout
    if let Some(c) = pathsource.msg(6) {
        let natural = c.size(1).map(|(w, h)| Size { width: w, height: h });
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
                tail_size: Size { width: tail_size_f, height: tail_size_f },
                corner_radius: c.f32v(4).map(|v| v as f64),
                center_tail: c.boolean(5),
            }),
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
        let natural = s.size(3).map(|(w, h)| Size { width: w, height: h });
        return ShapeGeometry {
            preset,
            scalar: s.f32v(2).map(|v| v as f64),
            natural_size: natural,
            path: None,
            callout: None,
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
        let natural = p.size(3).map(|(w, h)| Size { width: w, height: h });
        return ShapeGeometry {
            preset,
            scalar: None,
            natural_size: natural,
            path: None,
            callout: None,
        };
    }
    ShapeGeometry { preset: None, scalar: None, natural_size: None, path: None, callout: None }
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
        return Some(Fill::Image { image, technique, tint });
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
    let mut gradient = Gradient { kind, stops, angle_deg: None, start_point: None, end_point: None };
    if let Some(a) = g.msg(5) {
        gradient.angle_deg = a.f32v(2).map(|v| v as f64);
    }
    if let Some(t) = g.msg(6) {
        gradient.start_point = t.point(1).map(|(x, y)| Point { x, y });
        gradient.end_point = t.point(2).map(|(x, y)| Point { x, y });
    }
    Some(Fill::Gradient { gradient })
}

pub fn stroke_of(ctx: &mut Ctx, m: &Msg) -> Option<Stroke> {
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
    let (dash, dash_phase) = match m.msg(6) {
        Some(p) => {
            let dash: Vec<f64> = p.packed_f32s(4).into_iter().map(|v| v as f64).collect();
            let dash = if p.varint(1) == Some(2) || dash.is_empty() { None } else { Some(dash) };
            (dash, p.f32v(2).map(|v| v as f64))
        }
        None => (None, None),
    };
    Some(Stroke {
        color,
        width_pt: m.f32v(2).unwrap_or(1.0) as f64,
        cap,
        join,
        miter_limit: m.f32v(5).map(|v| v as f64),
        dash,
        dash_phase,
    })
}

pub fn shadow_of(ctx: &mut Ctx, m: &Msg) -> Option<Shadow> {
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
    let curved = m.msg(10).map(|c| CurvedShadow { curve: c.f32v(1).map(|v| v as f64) });
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
    Some(Reflection { opacity: m.f32v(1).unwrap_or(0.5) as f64 })
}

pub fn line_end_of(_ctx: &mut Ctx, m: &Msg) -> Option<LineEnd> {
    Some(LineEnd {
        identifier: m.string(5),
        is_filled: m.boolean(4),
        path: m.msg(1).as_ref().and_then(tsp_path),
    })
}
