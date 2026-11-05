//! Direct pose manipulation in the canvas, using the unanimated geometry as the reference.
use super::{Editor, Geometry, Keyframe, Point, VectorcraftApp, corners, geometry, inverse, json};
use crate::canvas::Xf;
use egui::{Pos2, Vec2};
use vectorcraft_geom::Affine;

#[derive(Clone, Copy, Debug)]
enum Handle {
    Move,
    Scale(usize),
    Rotate,
}

#[derive(Clone, Debug)]
pub(crate) struct Drag {
    handle: Handle,
    start: Keyframe,
    pointer: Point,
    inverse_parent: Affine,
    pivot: Point,
    bounds: vectorcraft_geom::Rect,
    angle: f64,
    turn: f64,
}

struct Handles {
    corners: Vec<Pos2>,
    center: Pos2,
    top: Pos2,
    rotation: Pos2,
}

fn handles(g: Geometry, xf: &Xf) -> Handles {
    let transform = g.parent * g.pose;
    let center = xf.to_screen(transform * g.bounds.center());
    let top = xf.to_screen(transform * Point::new(g.bounds.center().x, g.bounds.y0));
    let direction = top - center;
    let direction = if direction.length() > 0.01 { direction.normalized() } else { Vec2::new(0.0, -1.0) };
    Handles { corners: corners(g.bounds).into_iter().map(|p| xf.to_screen(transform * p)).collect(), center, top, rotation: top + direction * 30.0 }
}

fn hit(g: Geometry, h: &Handles, xf: &Xf, pointer: Pos2) -> Option<Handle> {
    if pointer.distance(h.rotation) <= 10.0 {
        return Some(Handle::Rotate);
    }
    if let Some(i) = h.corners.iter().position(|p| pointer.distance(*p) <= 9.0) {
        return Some(Handle::Scale(i));
    }
    let inside = inverse(g.parent * g.pose).is_some_and(|inv| g.bounds.contains(inv * xf.to_doc(pointer)));
    (inside || pointer.distance(h.center) < 10.0).then_some(Handle::Move)
}

fn editor(app: &VectorcraftApp) -> Option<(&Editor, Geometry)> {
    let state = &app.ui.animation;
    let st = app.session.active()?;
    let edit = state.editor.as_ref()?;
    if !state.enabled || state.playing || state.document != st.uid || !edit.on_canvas || !edit.valid(st) {
        return None;
    }
    Some((edit, geometry(&st.doc, edit)?))
}

fn begin_drag(g: Geometry, k: Keyframe, handle: Handle, pointer: Point) -> Option<Drag> {
    let inv = inverse(g.parent)?;
    let pointer = inv * pointer;
    let pivot = g.pose * g.bounds.center();
    let v = pointer - pivot;
    Some(Drag { handle, start: k, pointer, inverse_parent: inv, pivot, bounds: g.bounds, angle: v.y.atan2(v.x), turn: 0.0 })
}

fn pose(drag: &mut Drag, pointer: Point, shift: bool) -> Keyframe {
    let pointer = drag.inverse_parent * pointer;
    let mut k = drag.start.clone();
    match drag.handle {
        Handle::Move => {
            let mut delta = pointer - drag.pointer;
            if shift {
                if delta.x.abs() > delta.y.abs() {
                    delta.y = 0.0;
                } else {
                    delta.x = 0.0;
                }
            }
            k.x = (k.x + delta.x).clamp(-100000.0, 100000.0);
            k.y = (k.y + delta.y).clamp(-100000.0, 100000.0);
        }
        Handle::Rotate => {
            let v = pointer - drag.pivot;
            if v.hypot() > 1e-9 {
                let angle = v.y.atan2(v.x);
                drag.turn += (angle - drag.angle + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
                drag.angle = angle;
                k.rotation = (k.rotation + drag.turn.to_degrees()).clamp(-100000.0, 100000.0);
                if shift {
                    k.rotation = (k.rotation / 15.0).round() * 15.0;
                }
            }
        }
        Handle::Scale(index) => scale(drag, index, pointer, shift, &mut k),
    }
    k
}

fn scale(drag: &Drag, index: usize, pointer: Point, shift: bool, k: &mut Keyframe) {
    let Some(corner) = corners(drag.bounds).get(index).copied() else { return };
    let reference = corner - drag.bounds.center();
    let rotation = Affine::rotate(-k.rotation.to_radians());
    let delta = rotation * Point::new(pointer.x - drag.pointer.x, pointer.y - drag.pointer.y);
    if reference.x.abs() > 1e-9 {
        k.scale_x += delta.x / reference.x;
    }
    if reference.y.abs() > 1e-9 {
        k.scale_y += delta.y / reference.y;
    }
    if shift && drag.start.scale_x.abs() > 1e-9 && drag.start.scale_y.abs() > 1e-9 {
        let (x, y) = (k.scale_x / drag.start.scale_x, k.scale_y / drag.start.scale_y);
        let factor = if (x - 1.0).abs() > (y - 1.0).abs() { x } else { y };
        k.scale_x = drag.start.scale_x * factor;
        k.scale_y = drag.start.scale_y * factor;
    }
    k.scale_x = k.scale_x.clamp(-100.0, 100.0);
    k.scale_y = k.scale_y.clamp(-100.0, 100.0);
}

pub fn canvas_input(app: &mut VectorcraftApp, ui: &egui::Ui, response: &egui::Response, xf: &Xf) {
    let Some((edit, g)) = editor(app) else { return };
    let key = edit.animation.keyframes.get(edit.frame).cloned();
    let Some(key) = key else { return };
    let h = handles(g, xf);
    let (pointer, pressed, down, shift, escape) = ui.input(|i| {
        (i.pointer.interact_pos(), i.pointer.primary_pressed(), i.pointer.primary_down(), i.modifiers.shift, i.key_pressed(egui::Key::Escape))
    });
    if escape {
        cancel_gesture(app);
        return;
    }
    let Some(pointer) = pointer else { return };
    let hover = response.contains_pointer().then(|| hit(g, &h, xf, pointer)).flatten();
    let mut drag = app.ui.animation.editor.as_mut().and_then(|e| e.drag.take());
    if drag.is_none()
        && pressed
        && let Some(handle) = hover
    {
        drag = begin_drag(g, key.clone(), handle, xf.to_doc(pointer));
    }
    let active = drag.as_ref().map(|d| d.handle).or(hover);
    if let Some(handle) = active {
        ui.ctx().set_cursor_icon(match handle {
            Handle::Move if down => egui::CursorIcon::Grabbing,
            Handle::Move => egui::CursorIcon::Grab,
            Handle::Rotate => egui::CursorIcon::Crosshair,
            Handle::Scale(_) => egui::CursorIcon::ResizeNwSe,
        });
    }
    update_drag(app, drag, xf.to_doc(pointer), shift, down, &key);
}

fn update_drag(app: &mut VectorcraftApp, mut drag: Option<Drag>, pointer: Point, shift: bool, down: bool, current: &Keyframe) {
    if !pointer.is_finite() {
        return;
    }
    if let Some(d) = &mut drag {
        let k = pose(d, pointer, shift);
        if k != *current {
            // The command reports validation errors in the status UI and leaves the draft intact.
            app.run("ui.animationEdit", json!({"action":"pose","pose":k})).ok();
        }
    }
    if let Some(edit) = &mut app.ui.animation.editor {
        edit.drag = if down { drag } else { None };
    }
}

fn cancel_gesture(app: &mut VectorcraftApp) {
    if let Some(drag) = app.ui.animation.editor.as_mut().and_then(|e| e.drag.take()) {
        // Escape rolls back just this drag; a second Escape discards the pending pose edit.
        app.run("ui.animationEdit", json!({"action":"pose","pose":drag.start})).ok();
    } else {
        app.run("ui.animationEdit", json!({"action":"cancel"})).ok();
    }
}

pub fn paint_handles(app: &VectorcraftApp, painter: &egui::Painter, xf: &Xf) {
    let Some((_, g)) = editor(app) else { return };
    let h = handles(g, xf);
    let t = crate::theme::Tokens::get(painter.ctx());
    let stroke = egui::Stroke::new(1.5, t.accent);
    painter.add(egui::Shape::closed_line(h.corners.clone(), stroke));
    painter.line_segment([h.top, h.rotation], stroke);
    painter.circle_filled(h.rotation, 5.0, t.panel);
    painter.circle_stroke(h.rotation, 5.0, stroke);
    for corner in h.corners {
        let r = egui::Rect::from_center_size(corner, egui::vec2(8.0, 8.0));
        painter.rect_filled(r, 0.0, t.panel);
        painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
    }
    painter.line_segment([h.center - egui::vec2(4.0, 0.0), h.center + egui::vec2(4.0, 0.0)], stroke);
    painter.line_segment([h.center - egui::vec2(0.0, 4.0), h.center + egui::vec2(0.0, 4.0)], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_geom::Rect;

    fn geometry(k: &Keyframe, parent: Affine) -> Geometry {
        let b = Rect::new(10.0, 20.0, 90.0, 60.0);
        Geometry { bounds: b, parent, pose: k.transform(b) }
    }

    #[test]
    fn moving_a_pose_keeps_its_existing_percent_translation_and_handles_transformed_parents() {
        let k = Keyframe { x_percent: 40.0, rotation: 30.0, ..Default::default() };
        let parent = Affine::translate((100.0, 50.0)) * Affine::rotate(0.5) * Affine::scale(2.0);
        let g = geometry(&k, parent);
        let start = g.pose * g.bounds.center();
        let mut d = begin_drag(g, k, Handle::Move, parent * start).unwrap();
        let moved = pose(&mut d, parent * Point::new(start.x + 25.0, start.y - 10.0), false);
        assert!((moved.x - 25.0).abs() < 1e-9);
        assert!((moved.y + 10.0).abs() < 1e-9);
        assert_eq!(moved.x_percent, 40.0);
        assert_eq!(moved.rotation, 30.0);
    }

    #[test]
    fn corner_scale_uses_rotated_axes_and_shift_preserves_the_original_aspect_ratio() {
        let k = Keyframe { rotation: 90.0, scale_x: 2.0, scale_y: 1.0, ..Default::default() };
        let g = geometry(&k, Affine::IDENTITY);
        let start = g.pose * Point::new(g.bounds.x1, g.bounds.y1);
        let mut d = begin_drag(g, k.clone(), Handle::Scale(2), start).unwrap();
        let rotated_delta = Affine::rotate(k.rotation.to_radians()) * Point::new(40.0, 0.0);
        let end = Point::new(start.x + rotated_delta.x, start.y + rotated_delta.y);
        let independent = pose(&mut d, end, false);
        assert!((independent.scale_x - 3.0).abs() < 1e-9);
        assert!((independent.scale_y - 1.0).abs() < 1e-9);
        let constrained = pose(&mut d, end, true);
        assert!((constrained.scale_x - 3.0).abs() < 1e-9);
        assert!((constrained.scale_y - 1.5).abs() < 1e-9);
    }

    #[test]
    fn rotation_is_continuous_across_the_angle_boundary_and_shift_snaps() {
        let k = Keyframe::default();
        let g = geometry(&k, Affine::IDENTITY);
        let pivot = g.bounds.center();
        let point = |a: f64| {
            let a = a.to_radians();
            Point::new(pivot.x + 50.0 * a.cos(), pivot.y + 50.0 * a.sin())
        };
        let mut d = begin_drag(g, k, Handle::Rotate, point(170.0)).unwrap();
        assert!((pose(&mut d, point(-170.0), false).rotation - 20.0).abs() < 1e-9);
        assert!((pose(&mut d, point(-155.0), false).rotation - 35.0).abs() < 1e-9);
        assert_eq!(pose(&mut d, point(-155.0), true).rotation, 30.0);
    }
}
