//! Stage drawing: a 2.5D perspective preview with light beams, and a
//! top-down plan where fixtures are placed by dragging.

use crate::engine::FxState;
#[allow(unused_imports)]
use crate::i18n::{t, ts};
use crate::model::*;
use egui::{pos2, vec2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind};

pub fn beam_dir(st: &FxState, hanging: bool) -> [f32; 3] {
    let t = st.tilt.to_radians();
    let p = st.pan.to_radians();
    let z = if hanging { -t.cos() } else { t.cos() };
    [t.sin() * p.sin(), -t.sin() * p.cos(), z]
}

struct Cam {
    cx: f32,
    horizon: f32,
    f: f32,
    dist: f32,
    h: f32,
}

impl Cam {
    fn depth(&self, y: f32) -> f32 {
        (y + self.dist).max(0.3)
    }
    fn p(&self, x: f32, y: f32, z: f32) -> Pos2 {
        let s = self.f / self.depth(y);
        pos2(self.cx + x * s, self.horizon - (z - self.h) * s)
    }
    fn scale_at(&self, y: f32) -> f32 {
        self.f / self.depth(y)
    }
}

fn rgba(c: [f32; 3], a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c[0].clamp(0.0, 1.0) * 255.0) as u8,
        (c[1].clamp(0.0, 1.0) * 255.0) as u8,
        (c[2].clamp(0.0, 1.0) * 255.0) as u8,
        (a.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

fn ellipse(center: Pos2, rx: f32, ry: f32, fill: Color32) -> Shape {
    let pts: Vec<Pos2> = (0..28)
        .map(|i| {
            let a = i as f32 / 28.0 * std::f32::consts::TAU;
            pos2(center.x + rx * a.cos(), center.y + ry * a.sin())
        })
        .collect();
    Shape::convex_polygon(pts, fill, Stroke::NONE)
}

/// Visible intensity after strobe flicker.
pub fn visible_dim(st: &FxState, time: f64) -> f32 {
    if st.strobe > 0.0 {
        let hz = 4.0 + 16.0 * st.strobe as f64;
        if (time * hz).fract() < 0.3 {
            st.dim
        } else {
            0.0
        }
    } else {
        st.dim
    }
}

/// Draw the 2.5D stage. Returns the id of a clicked fixture.
pub fn draw_perspective(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    project: &Project,
    catalog: &[FixtureDef],
    states: &[FxState],
    selected: Option<u32>,
    time: f64,
) -> Option<u32> {
    let (resp, painter) = ui.allocate_painter(size, Sense::click());
    let rect = resp.rect;
    let st = &project.stage;
    let w = st.width.max(2.0);
    let d = st.depth.max(1.0);
    let zmax = st.truss_front.max(st.truss_back).max(3.0) + 0.6;
    let dist = 9.0f32;
    let cam_h = 1.7f32;
    let scale = (rect.width() * 0.9 / w).min((rect.height() - 34.0) / (zmax + 0.2));
    let cam = Cam {
        cx: rect.center().x,
        horizon: rect.top() + 10.0 + (zmax - cam_h) * scale,
        f: scale * dist,
        dist,
        h: cam_h,
    };

    painter.rect_filled(rect, 4.0, Color32::from_rgb(7, 7, 12));
    // back wall & floor
    let hw = w / 2.0;
    let wall_h = zmax + 1.0;
    painter.add(Shape::convex_polygon(
        vec![cam.p(-hw, d, 0.0), cam.p(hw, d, 0.0), cam.p(hw, d, wall_h), cam.p(-hw, d, wall_h)],
        Color32::from_rgb(13, 13, 20),
        Stroke::NONE,
    ));
    painter.add(Shape::convex_polygon(
        vec![cam.p(-hw, 0.0, 0.0), cam.p(hw, 0.0, 0.0), cam.p(hw, d, 0.0), cam.p(-hw, d, 0.0)],
        Color32::from_rgb(24, 24, 32),
        Stroke::new(1.0, Color32::from_rgb(50, 50, 64)),
    ));
    let grid = Stroke::new(0.5, Color32::from_rgb(34, 34, 44));
    let mut x = -hw.floor();
    while x <= hw {
        painter.line_segment([cam.p(x, 0.0, 0.0), cam.p(x, d, 0.0)], grid);
        x += 1.0;
    }
    let mut y = 1.0;
    while y < d {
        painter.line_segment([cam.p(-hw, y, 0.0), cam.p(hw, y, 0.0)], grid);
        y += 1.0;
    }
    // stage front face
    painter.add(Shape::convex_polygon(
        vec![cam.p(-hw, 0.0, 0.0), cam.p(hw, 0.0, 0.0), cam.p(hw, -0.01, -0.8), cam.p(-hw, -0.01, -0.8)],
        Color32::from_rgb(16, 16, 22),
        Stroke::NONE,
    ));

    // drum riser + simple band silhouettes, lit by the front wash
    let front_light: [f32; 3] = {
        let mut c = [0.0f32; 3];
        let mut n = 0.0;
        for (f, s) in project.fixtures.iter().zip(states) {
            if f.group == Group::FrontWash {
                let v = visible_dim(s, time);
                for i in 0..3 {
                    c[i] += s.rgb[i] * v;
                }
                n += 1.0;
            }
        }
        if n > 0.0 {
            [c[0] / n, c[1] / n, c[2] / n]
        } else {
            [0.0; 3]
        }
    };
    let body = |k: f32| {
        Color32::from_rgb(
            (18.0 + front_light[0] * 120.0 * k) as u8,
            (18.0 + front_light[1] * 120.0 * k) as u8,
            (22.0 + front_light[2] * 120.0 * k) as u8,
        )
    };
    let ry = d * 0.62;
    painter.add(Shape::convex_polygon(
        vec![cam.p(-1.3, ry - 0.9, 0.5), cam.p(1.3, ry - 0.9, 0.5), cam.p(1.3, ry - 0.9, 0.0), cam.p(-1.3, ry - 0.9, 0.0)],
        body(0.4),
        Stroke::NONE,
    ));
    for (bx, by, h) in [(0.0f32, ry - 0.2, 1.75f32), (-2.6, d * 0.32, 1.8), (2.6, d * 0.32, 1.85), (0.0, d * 0.18, 1.8)] {
        let z0 = if (by - (ry - 0.2)).abs() < 0.01 { 0.5 } else { 0.0 };
        let s = cam.scale_at(by);
        let foot = cam.p(bx, by, z0);
        let head = cam.p(bx, by, z0 + h - 0.15);
        painter.rect_filled(Rect::from_min_max(pos2(foot.x - 0.2 * s, head.y + 0.12 * s), pos2(foot.x + 0.2 * s, foot.y)), 3.0, body(0.8));
        painter.circle_filled(head, 0.13 * s, body(0.9));
    }

    // trusses
    let truss = Stroke::new(2.0, Color32::from_rgb(90, 90, 100));
    for (ty, tz) in [(0.4f32, st.truss_front), (d - 0.5, st.truss_back)] {
        painter.line_segment([cam.p(-hw - 0.3, ty, tz), cam.p(hw + 0.3, ty, tz)], truss);
        painter.line_segment([cam.p(-hw - 0.3, ty, tz + 0.3), cam.p(hw + 0.3, ty, tz + 0.3)], Stroke::new(1.0, Color32::from_rgb(70, 70, 80)));
        for sx in [-hw - 0.3, hw + 0.3] {
            painter.line_segment([cam.p(sx, ty, 0.0), cam.p(sx, ty, tz + 0.3)], truss);
        }
    }

    // beams, from back to front
    let mut order: Vec<usize> = (0..project.fixtures.len()).collect();
    order.sort_by(|a, b| project.fixtures[*b].y.total_cmp(&project.fixtures[*a].y));
    for &i in &order {
        let f = &project.fixtures[i];
        let Some(s) = states.get(i) else { continue };
        let def = project.def(catalog, &f.def_id);
        let dim = visible_dim(s, time);
        if dim < 0.02 {
            continue;
        }
        let kind = def.map(|d| d.kind).unwrap_or(FixtureKind::Par);
        let angle = def.map(|d| d.beam_angle).unwrap_or(25.0).clamp(2.0, 70.0);
        let dir = beam_dir(s, f.hanging);
        let max_len = if kind.is_moving() { 14.0 } else { 6.0 };
        let mut len = max_len;
        let mut hits_floor = false;
        if dir[2] < -0.05 {
            let l = f.z / -dir[2];
            if l < len {
                len = l;
                hits_floor = true;
            }
        }
        let start = [f.x, f.y, f.z];
        let end = [start[0] + dir[0] * len, start[1] + dir[1] * len, start[2] + dir[2] * len];
        if end[1] < -2.5 {
            // towards the audience: shorten so it stays in the picture
            let k = (start[1] + 2.5) / (start[1] - end[1]).max(0.01);
            len *= k.clamp(0.1, 1.0);
        }
        let end = [start[0] + dir[0] * len, start[1] + dir[1] * len, start[2] + dir[2] * len];
        let s0 = cam.p(start[0], start[1], start[2]);
        let s1 = cam.p(end[0], end[1], end[2]);
        let dv = s1 - s0;
        if dv.length() < 1.0 {
            continue;
        }
        let perp = vec2(-dv.y, dv.x).normalized();
        let half = (angle * 0.5).to_radians().tan();
        let w0 = 0.08 * cam.scale_at(start[1]);
        let w1 = (len * half).max(0.08) * cam.scale_at(end[1]);
        let narrow = angle < 12.0;
        let base_a = if narrow { 0.42 } else if kind == FixtureKind::Blinder { 0.12 } else { 0.2 } * dim;
        let mut mesh = egui::Mesh::default();
        let col0 = rgba(s.rgb, base_a);
        let col1 = rgba(s.rgb, base_a * 0.08);
        let idx = mesh.vertices.len() as u32;
        mesh.colored_vertex(s0 + perp * w0, col0);
        mesh.colored_vertex(s0 - perp * w0, col0);
        mesh.colored_vertex(s1 + perp * w1, col1);
        mesh.colored_vertex(s1 - perp * w1, col1);
        mesh.add_triangle(idx, idx + 1, idx + 2);
        mesh.add_triangle(idx + 1, idx + 3, idx + 2);
        // brighter core
        let wc = w1 * 0.35;
        let c0 = rgba(s.rgb, (base_a * 1.4).min(0.9));
        let c1 = rgba(s.rgb, base_a * 0.15);
        let j = mesh.vertices.len() as u32;
        mesh.colored_vertex(s0 + perp * w0 * 0.5, c0);
        mesh.colored_vertex(s0 - perp * w0 * 0.5, c0);
        mesh.colored_vertex(s1 + perp * wc, c1);
        mesh.colored_vertex(s1 - perp * wc, c1);
        mesh.add_triangle(j, j + 1, j + 2);
        mesh.add_triangle(j + 1, j + 3, j + 2);
        painter.add(Shape::mesh(mesh));
        if hits_floor {
            let r = (len * half).max(0.1);
            let c = cam.p(end[0], end[1], 0.0);
            let sc = cam.scale_at(end[1]);
            painter.add(ellipse(c, r * sc, r * sc * 0.28, rgba(s.rgb, 0.35 * dim)));
        }
    }

    // fixture bodies
    let mut clicked = None;
    let pointer = resp.interact_pointer_pos();
    let mut best_d = f32::MAX;
    for &i in &order {
        let f = &project.fixtures[i];
        let s = states.get(i).copied().unwrap_or_default();
        let def = project.def(catalog, &f.def_id);
        let kind = def.map(|d| d.kind).unwrap_or(FixtureKind::Other);
        let p = cam.p(f.x, f.y, f.z);
        let sc = cam.scale_at(f.y);
        let dim = visible_dim(&s, time);
        let lit = rgba([s.rgb[0] * 0.3 + s.rgb[0] * 0.7 * dim, s.rgb[1] * 0.3 + s.rgb[1] * 0.7 * dim, s.rgb[2] * 0.3 + s.rgb[2] * 0.7 * dim], 1.0);
        let lens = if dim > 0.02 { lit } else { Color32::from_rgb(40, 40, 46) };
        let case = Color32::from_rgb(30, 30, 36);
        let edge = Stroke::new(1.0, Color32::from_rgb(85, 85, 95));
        if dim > 0.05 {
            painter.circle_filled(p, 0.45 * sc, rgba(s.rgb, 0.18 * dim));
        }
        match kind {
            FixtureKind::Blinder => {
                let r = Rect::from_center_size(p, vec2(0.6 * sc, 0.6 * sc));
                painter.rect_filled(r, 2.0, case);
                for dx in [-0.15f32, 0.15] {
                    for dy in [-0.15f32, 0.15] {
                        painter.circle_filled(p + vec2(dx * sc, dy * sc), 0.11 * sc, lens);
                    }
                }
                painter.rect_stroke(r, 2.0, edge, StrokeKind::Inside);
            }
            FixtureKind::Strobe => {
                let r = Rect::from_center_size(p, vec2(0.7 * sc, 0.25 * sc));
                painter.rect_filled(r, 2.0, lens);
                painter.rect_stroke(r, 2.0, edge, StrokeKind::Inside);
            }
            FixtureKind::LedBar => {
                let r = Rect::from_center_size(p, vec2(1.0 * sc, 0.12 * sc));
                painter.rect_filled(r, 1.0, lens);
                painter.rect_stroke(r, 1.0, edge, StrokeKind::Inside);
            }
            k if k.is_moving() => {
                let base_y = if f.hanging { -0.15 } else { 0.15 };
                let base = Rect::from_center_size(p + vec2(0.0, base_y * sc), vec2(0.4 * sc, 0.12 * sc));
                painter.rect_filled(base, 2.0, case);
                painter.circle_filled(p - vec2(0.0, base_y * sc), 0.17 * sc, case);
                painter.circle_filled(p - vec2(0.0, base_y * sc), 0.1 * sc, lens);
                painter.circle_stroke(p - vec2(0.0, base_y * sc), 0.17 * sc, edge);
            }
            _ => {
                painter.circle_filled(p, 0.17 * sc, case);
                painter.circle_filled(p, 0.12 * sc, lens);
                painter.circle_stroke(p, 0.17 * sc, edge);
            }
        }
        if Some(f.id) == selected {
            painter.circle_stroke(p, 0.4 * sc, Stroke::new(2.0, Color32::from_rgb(255, 210, 0)));
        }
        if let Some(pp) = pointer {
            let dd = pp.distance(p);
            if dd < (0.45 * sc).max(10.0) && dd < best_d {
                best_d = dd;
                clicked = Some(f.id);
            }
        }
        if let Some(h) = resp.hover_pos() {
            if h.distance(p) < (0.35 * sc).max(8.0) {
                painter.text(p + vec2(0.0, 0.45 * sc), egui::Align2::CENTER_TOP, &f.name, egui::FontId::proportional(11.0), Color32::WHITE);
            }
        }
    }
    painter.text(
        pos2(rect.center().x, rect.bottom() - 6.0),
        egui::Align2::CENTER_BOTTOM,
        ts("▼  PUBBLICO  ▼"),
        egui::FontId::proportional(11.0),
        Color32::from_rgb(110, 110, 125),
    );
    if resp.clicked() {
        clicked
    } else {
        None
    }
}

pub enum PlanDrag {
    Move { last: (f32, f32), moved: bool },
    Rect { start: Pos2 },
}

/// Top-down plan. Fixtures can be selected (Ctrl/Shift = add, drag on empty
/// space = box selection) and dragged together. Returns true if something moved.
pub fn draw_plan(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    project: &mut Project,
    catalog: &[FixtureDef],
    states: &[FxState],
    selected: &mut Option<u32>,
    selset: &mut std::collections::BTreeSet<u32>,
    drag: &mut Option<PlanDrag>,
) -> bool {
    let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
    let rect = resp.rect;
    let w = project.stage.width.max(2.0);
    let d = project.stage.depth.max(1.0);
    let sc = ((rect.width() - 20.0) / (w + 2.0)).min((rect.height() - 20.0) / (d + 2.2));
    let cx = rect.center().x;
    let top = rect.top() + 10.0 + (rect.height() - 20.0 - (d + 2.2) * sc) / 2.0;
    let to_s = |x: f32, y: f32| pos2(cx + x * sc, top + (d + 0.5 - y) * sc);
    let from_s = |p: Pos2| ((p.x - cx) / sc, d + 0.5 - (p.y - top) / sc);

    painter.rect_filled(rect, 4.0, Color32::from_rgb(12, 12, 18));
    let hw = w / 2.0;
    let stage = Rect::from_two_pos(to_s(-hw, 0.0), to_s(hw, d));
    painter.rect_filled(stage, 2.0, Color32::from_rgb(28, 28, 38));
    painter.rect_stroke(stage, 2.0, Stroke::new(1.0, Color32::from_rgb(70, 70, 90)), StrokeKind::Inside);
    let grid = Stroke::new(0.5, Color32::from_rgb(40, 40, 52));
    let mut x = -hw.floor();
    while x <= hw {
        painter.line_segment([to_s(x, 0.0), to_s(x, d)], grid);
        x += 1.0;
    }
    let mut y = 1.0;
    while y < d {
        painter.line_segment([to_s(-hw, y), to_s(hw, y)], grid);
        y += 1.0;
    }
    let truss = Stroke::new(3.0, Color32::from_rgb(85, 85, 95));
    painter.line_segment([to_s(-hw - 0.3, 0.4), to_s(hw + 0.3, 0.4)], truss);
    painter.line_segment([to_s(-hw - 0.3, d - 0.5), to_s(hw + 0.3, d - 0.5)], truss);
    let small = egui::FontId::proportional(11.0);
    let gray = Color32::from_rgb(120, 120, 135);
    painter.text(to_s(hw + 0.35, 0.4), egui::Align2::LEFT_CENTER, ts("americana\nfronte"), small.clone(), gray);
    painter.text(to_s(hw + 0.35, d - 0.5), egui::Align2::LEFT_CENTER, ts("americana\nfondo"), small.clone(), gray);
    painter.text(to_s(0.0, -0.9), egui::Align2::CENTER_CENTER, ts("▼  PUBBLICO  ▼"), small.clone(), gray);
    painter.text(to_s(0.0, d + 0.25), egui::Align2::CENTER_CENTER, ts("fondale"), small.clone(), gray);

    // hit testing
    let pick = |p: Pos2, project: &Project| -> Option<u32> {
        let mut best = (f32::MAX, None);
        for f in &project.fixtures {
            let dd = to_s(f.x, f.y).distance(p);
            if dd < 14.0 && dd < best.0 {
                best = (dd, Some(f.id));
            }
        }
        best.1
    };
    let mut changed = false;
    let additive = ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift);
    let pressed = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed());
    if pressed {
        if let Some(p) = resp.interact_pointer_pos() {
            match pick(p, project) {
                Some(id) => {
                    if additive {
                        if !selset.remove(&id) {
                            selset.insert(id);
                        }
                    } else if !selset.contains(&id) {
                        selset.clear();
                        selset.insert(id);
                    }
                    *selected = if selset.contains(&id) { Some(id) } else { selset.iter().next().copied() };
                    let (x, y) = from_s(p);
                    *drag = Some(PlanDrag::Move { last: (x, y), moved: false });
                }
                None => {
                    if !additive {
                        selset.clear();
                        *selected = None;
                    }
                    *drag = Some(PlanDrag::Rect { start: p });
                }
            }
        }
    }
    if let (Some(dr), Some(p)) = (drag.as_mut(), resp.interact_pointer_pos()) {
        match dr {
            PlanDrag::Move { last, moved } => {
                if resp.dragged() {
                    let (x, y) = from_s(p);
                    let (dx, dy) = (x - last.0, y - last.1);
                    if dx != 0.0 || dy != 0.0 {
                        for f in project.fixtures.iter_mut().filter(|f| selset.contains(&f.id)) {
                            f.x = (f.x + dx).clamp(-hw - 0.5, hw + 0.5);
                            f.y = (f.y + dy).clamp(-0.5, d + 0.3);
                        }
                        *last = (x, y);
                        *moved = true;
                        changed = true;
                    }
                }
            }
            PlanDrag::Rect { start } => {
                let r = Rect::from_two_pos(*start, p);
                painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 200, 80, 25));
                painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(255, 200, 80)), StrokeKind::Inside);
            }
        }
    }
    let released = drag.is_some() && !ui.input(|i| i.pointer.primary_down());
    if resp.drag_stopped() || released {
        match drag.take() {
            Some(PlanDrag::Move { moved: true, .. }) => {
                let snap = |v: f32| (v * 20.0).round() / 20.0;
                for f in project.fixtures.iter_mut().filter(|f| selset.contains(&f.id)) {
                    f.x = snap(f.x);
                    f.y = snap(f.y);
                }
                changed = true;
            }
            Some(PlanDrag::Move { moved: false, .. }) => {
                // simple click on an already selected fixture: select only that one
                if !additive {
                    if let Some(p) = resp.interact_pointer_pos().or(resp.hover_pos()) {
                        if let Some(id) = pick(p, project) {
                            selset.clear();
                            selset.insert(id);
                            *selected = Some(id);
                        }
                    }
                }
            }
            Some(PlanDrag::Rect { start }) => {
                if let Some(p) = resp.interact_pointer_pos().or(resp.hover_pos()) {
                    let r = Rect::from_two_pos(start, p);
                    if r.width() > 3.0 || r.height() > 3.0 {
                        for f in &project.fixtures {
                            if r.contains(to_s(f.x, f.y)) {
                                selset.insert(f.id);
                            }
                        }
                        if selected.is_none() {
                            *selected = selset.iter().next().copied();
                        }
                    }
                }
            }
            None => {}
        }
    }

    for (i, f) in project.fixtures.iter().enumerate() {
        let s = states.get(i).copied().unwrap_or_default();
        let def = project.def(catalog, &f.def_id);
        let kind = def.map(|d| d.kind).unwrap_or(FixtureKind::Other);
        let p = to_s(f.x, f.y);
        let col = if s.dim > 0.02 {
            rgba([s.rgb[0] * (0.3 + 0.7 * s.dim), s.rgb[1] * (0.3 + 0.7 * s.dim), s.rgb[2] * (0.3 + 0.7 * s.dim)], 1.0)
        } else {
            Color32::from_rgb(55, 55, 62)
        };
        let edge = if Some(f.id) == *selected {
            Stroke::new(2.5, Color32::from_rgb(255, 210, 0))
        } else if selset.contains(&f.id) {
            Stroke::new(2.0, Color32::from_rgb(255, 160, 40))
        } else {
            Stroke::new(1.5, Color32::from_rgb(150, 150, 160))
        };
        match kind {
            FixtureKind::Blinder => {
                let r = Rect::from_center_size(p, vec2(16.0, 16.0));
                painter.rect_filled(r, 2.0, col);
                painter.rect_stroke(r, 2.0, edge, StrokeKind::Outside);
            }
            FixtureKind::Strobe | FixtureKind::LedBar => {
                let r = Rect::from_center_size(p, vec2(22.0, 8.0));
                painter.rect_filled(r, 2.0, col);
                painter.rect_stroke(r, 2.0, edge, StrokeKind::Outside);
            }
            k if k.is_moving() => {
                painter.circle_filled(p, 9.0, col);
                painter.circle_stroke(p, 9.0, edge);
                let dir = beam_dir(&s, f.hanging);
                let tip = p + vec2(dir[0], -dir[1]) * 18.0 * (dir[0].hypot(dir[1])).max(0.2) / (dir[0].hypot(dir[1])).max(0.2);
                painter.line_segment([p, tip], Stroke::new(1.5, col));
            }
            _ => {
                painter.circle_filled(p, 7.0, col);
                painter.circle_stroke(p, 7.0, edge);
            }
        }
        let tag = if f.hanging { crate::i18n::t(&format!("{} ↑{:.1}m", f.name, f.z)) } else { crate::i18n::t(&format!("{} {:.1}m", f.name, f.z)) };
        painter.text(p + vec2(0.0, 11.0), egui::Align2::CENTER_TOP, tag, egui::FontId::proportional(9.5), Color32::from_rgb(170, 170, 185));
    }
    changed
}
