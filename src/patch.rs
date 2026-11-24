//! "Patch DMX" view: the 512 channels of the universe drawn as a grid.
//! Every light is a coloured block over the channels it uses; dragging a block
//! changes its DMX address (all selected lights move together).

use crate::model::*;
use egui::{pos2, vec2, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, StrokeKind};
use std::collections::BTreeSet;

const COLS: i32 = 32;
const ROWS: i32 = 16;
const UNI: i32 = 512;

/// A drag in progress: the channel grabbed and where the dragged lights started.
pub struct PatchDrag {
    grab: i32,
    /// (id, original address, channel count)
    items: Vec<(u32, u16, u16)>,
    delta: i32,
}

struct Block {
    id: u32,
    name: String,
    group: Group,
    /// 0-based first channel
    start: i32,
    len: i32,
}

fn group_color(g: Group) -> Color32 {
    match g {
        Group::FrontWash => Color32::from_rgb(70, 130, 200),
        Group::BackWash => Color32::from_rgb(160, 80, 190),
        Group::Blinders => Color32::from_rgb(220, 160, 60),
        Group::HeadsFront => Color32::from_rgb(60, 170, 120),
        Group::HeadsBack => Color32::from_rgb(40, 140, 150),
        Group::Strobes => Color32::from_rgb(190, 190, 210),
        Group::Bars => Color32::from_rgb(210, 100, 120),
        Group::Extra => Color32::from_rgb(130, 130, 130),
    }
}

/// Returns true when addresses were changed.
#[allow(clippy::too_many_arguments)]
pub fn draw_patch(
    ui: &mut egui::Ui,
    size: egui::Vec2,
    project: &mut Project,
    catalog: &[FixtureDef],
    universe: &[u8; 512],
    sel: &mut Option<u32>,
    set: &mut BTreeSet<u32>,
    drag: &mut Option<PatchDrag>,
) -> bool {
    let (outer, _) = ui.allocate_exact_size(size, Sense::hover());
    let label_w = 34.0;
    let cw = ((outer.width() - label_w) / COLS as f32).max(8.0);
    let chh = (outer.height() / ROWS as f32).clamp(16.0, cw * 1.6);
    let grid = Rect::from_min_size(pos2(outer.left() + label_w, outer.top()), vec2(cw * COLS as f32, chh * ROWS as f32));
    let resp = ui.interact(grid, ui.id().with("patch_grid"), Sense::click_and_drag());

    let cell = |c: i32| -> Rect {
        let (r, k) = (c / COLS, c % COLS);
        Rect::from_min_size(pos2(grid.left() + k as f32 * cw, grid.top() + r as f32 * chh), vec2(cw, chh))
    };
    let ch_at = |p: Pos2| -> i32 {
        let k = (((p.x - grid.left()) / cw).floor() as i32).clamp(0, COLS - 1);
        let r = (((p.y - grid.top()) / chh).floor() as i32).clamp(0, ROWS - 1);
        r * COLS + k
    };

    let blocks: Vec<Block> = project
        .fixtures
        .iter()
        .map(|f| Block {
            id: f.id,
            name: f.name.clone(),
            group: f.group,
            start: f.address as i32 - 1,
            len: project.def(catalog, &f.def_id).map(|d| d.footprint()).unwrap_or(1) as i32,
        })
        .collect();
    let hit = |c: i32, set: &BTreeSet<u32>| -> Option<u32> {
        let all: Vec<&Block> = blocks.iter().filter(|b| c >= b.start && c < b.start + b.len).collect();
        all.iter().rev().find(|b| set.contains(&b.id)).or(all.last()).map(|b| b.id)
    };

    // ---- interaction
    let mods = ui.input(|i| i.modifiers);
    let additive = mods.ctrl || mods.command || mods.shift;
    if resp.drag_started() {
        if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
            let c = ch_at(p);
            if let Some(id) = hit(c, set) {
                if !set.contains(&id) {
                    if !additive {
                        set.clear();
                    }
                    set.insert(id);
                }
                *sel = Some(id);
                let items = blocks.iter().filter(|b| set.contains(&b.id)).map(|b| (b.id, (b.start + 1) as u16, b.len as u16)).collect();
                *drag = Some(PatchDrag { grab: c, items, delta: 0 });
            }
        }
    }
    if let Some(d) = drag.as_mut() {
        if let Some(p) = resp.interact_pointer_pos() {
            let lo = -d.items.iter().map(|i| i.1 as i32 - 1).min().unwrap_or(0);
            let hi = UNI - d.items.iter().map(|i| i.1 as i32 + i.2 as i32 - 1).max().unwrap_or(0);
            d.delta = (ch_at(p) - d.grab).max(lo).min(hi.max(lo));
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    let mut changed = false;
    if resp.drag_stopped() {
        if let Some(d) = drag.take() {
            if d.delta != 0 {
                for (id, a, _) in &d.items {
                    if let Some(f) = project.fixtures.iter_mut().find(|f| f.id == *id) {
                        f.address = (*a as i32 + d.delta).clamp(1, UNI) as u16;
                    }
                }
                changed = true;
            }
        }
    }
    if resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            match hit(ch_at(p), set) {
                Some(id) if mods.ctrl || mods.command => {
                    if !set.remove(&id) {
                        set.insert(id);
                        *sel = Some(id);
                    } else if *sel == Some(id) {
                        *sel = set.iter().next().copied();
                    }
                }
                Some(id) => {
                    if !mods.shift {
                        set.clear();
                    }
                    set.insert(id);
                    *sel = Some(id);
                }
                None if !additive => {
                    set.clear();
                    *sel = None;
                }
                None => {}
            }
        }
    }

    // ---- positions to draw (dragged lights at their preview position)
    let moving: BTreeSet<u32> = drag.as_ref().map(|d| d.items.iter().map(|i| i.0).collect()).unwrap_or_default();
    let delta = drag.as_ref().map(|d| d.delta).unwrap_or(0);
    let pos = |b: &Block| if moving.contains(&b.id) { b.start + delta } else { b.start };
    let mut cover = [0u8; 512];
    for b in &blocks {
        for c in pos(b).max(0)..(pos(b) + b.len).min(UNI) {
            cover[c as usize] = cover[c as usize].saturating_add(1);
        }
    }

    // ---- drawing
    let painter = ui.painter_at(outer);
    let small = FontId::proportional((chh * 0.36).clamp(8.0, 11.0));
    let name_font = FontId::proportional((chh * 0.42).clamp(9.0, 13.0));
    for r in 0..ROWS {
        let y = grid.top() + (r as f32 + 0.5) * chh;
        painter.text(pos2(grid.left() - 6.0, y), Align2::RIGHT_CENTER, format!("{}", r * COLS + 1), small.clone(), Color32::GRAY);
    }
    for c in 0..UNI {
        let rc = cell(c).shrink(0.5);
        painter.rect_filled(rc, 2.0, Color32::from_gray(32));
        if cover[c as usize] == 0 {
            painter.text(rc.center(), Align2::CENTER_CENTER, format!("{}", c + 1), small.clone(), Color32::from_gray(80));
        }
    }

    let order = blocks.iter().filter(|b| !moving.contains(&b.id)).chain(blocks.iter().filter(|b| moving.contains(&b.id)));
    for b in order {
        let start = pos(b);
        let end = (start + b.len).min(UNI);
        let selected = set.contains(&b.id);
        let base = group_color(b.group);
        let fill = if selected { base.gamma_multiply(0.85) } else { base.gamma_multiply(0.5) };
        let stroke = if selected { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::new(1.0_f32, base) };
        let mut c = start.max(0);
        let mut first = true;
        while c < end {
            let row_end = ((c / COLS + 1) * COLS).min(end);
            let seg = cell(c).union(cell(row_end - 1)).shrink2(vec2(1.0, 1.5));
            painter.rect_filled(seg, 3.0, fill);
            // live level of each channel, as a thin bar at the bottom of the cell
            for k in c..row_end {
                let v = universe[k as usize] as f32 / 255.0;
                if v > 0.0 {
                    let rc = cell(k).shrink2(vec2(2.0, 2.0));
                    let bar = Rect::from_min_max(pos2(rc.left(), rc.bottom() - 3.0), pos2(rc.left() + rc.width() * v, rc.bottom()));
                    painter.rect_filled(bar, 0.0, Color32::from_white_alpha(170));
                }
                if k > c {
                    let x = cell(k).left();
                    painter.line_segment([pos2(x, seg.top() + 3.0), pos2(x, seg.bottom() - 3.0)], Stroke::new(1.0_f32, Color32::from_black_alpha(60)));
                }
            }
            painter.rect_stroke(seg, 3.0, stroke, StrokeKind::Inside);
            let txt = if first { format!("{} · {}", b.name, start + 1) } else { format!("… {}", b.name) };
            painter.with_clip_rect(seg.shrink(2.0)).text(
                pos2(seg.left() + 4.0, seg.center().y - 2.0),
                Align2::LEFT_CENTER,
                txt,
                name_font.clone(),
                if first { Color32::WHITE } else { Color32::from_gray(200) },
            );
            first = false;
            c = row_end;
        }
    }

    // overlaps
    for c in 0..UNI {
        if cover[c as usize] > 1 {
            let rc = cell(c).shrink(1.0);
            let red = Color32::from_rgb(255, 70, 60);
            painter.rect_stroke(rc, 2.0, Stroke::new(2.0_f32, red), StrokeKind::Inside);
            painter.line_segment([rc.left_bottom(), rc.right_top()], Stroke::new(1.5_f32, red));
        }
    }

    // ---- hover info
    if drag.is_none() {
        if let Some(p) = resp.hover_pos() {
            let c = ch_at(p);
            let txt = match hit(c, set).and_then(|id| project.fixtures.iter().find(|f| f.id == id)) {
                Some(f) => {
                    let off = c - (f.address as i32 - 1);
                    let chn = project.def(catalog, &f.def_id).and_then(|d| d.channels.get(off as usize)).map(|x| x.name.clone()).unwrap_or_default();
                    ui.ctx().set_cursor_icon(CursorIcon::Grab);
                    crate::i18n::t(&format!("Canale {} · {} · {}: {} · valore {}", c + 1, f.name, off + 1, chn, universe[c as usize]))
                }
                None => crate::i18n::t(&format!("Canale {} · libero", c + 1)),
            };
            resp.on_hover_text_at_pointer(txt);
        }
    }
    changed
}

/// One line of info for under the grid: used channels, or the drag in progress.
pub fn summary(project: &Project, catalog: &[FixtureDef], drag: &Option<PatchDrag>) -> String {
    if let Some(d) = drag {
        let (lo, hi) = d.items.iter().fold((i32::MAX, 0), |(lo, hi), i| (lo.min(i.1 as i32), hi.max(i.1 as i32 + i.2 as i32 - 1)));
        return crate::i18n::t(&format!("Sposto {} fari → DMX {}-{}", d.items.len(), lo + d.delta, hi + d.delta));
    }
    let mut used = [false; 512];
    for f in &project.fixtures {
        let n = project.def(catalog, &f.def_id).map(|d| d.footprint()).unwrap_or(1) as usize;
        let a = (f.address as usize).max(1) - 1;
        for c in a..(a + n).min(512) {
            used[c] = true;
        }
    }
    let n = used.iter().filter(|x| **x).count();
    crate::i18n::t(&format!("{} canali usati, {} liberi · trascina un faro per cambiarne l'indirizzo (si spostano tutti i selezionati) · Ctrl/Maiusc+clic: aggiungi alla selezione", n, 512 - n))
}
