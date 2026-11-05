//! A pose is edited on a render-only copy; Apply changes only animation metadata.
use super::{Animation, Keyframe, Playback, Value, VectorcraftApp, default_animation, json};
use serde::Deserialize;
use vectorcraft_doc::{Document, NodeId};
use vectorcraft_geom::{Affine, Point, Rect};

mod gizmo;
pub use gizmo::{canvas_input, paint_handles};

#[derive(Clone, Debug)]
pub(super) struct Editor {
    pub document: u64,
    pub id: NodeId,
    pub source: Option<Animation>,
    pub animation: Animation,
    pub frame: usize,
    pub on_canvas: bool,
    pub drag: Option<gizmo::Drag>,
}

impl Editor {
    pub fn valid(&self, st: &vectorcraft_engine::DocState) -> bool {
        st.uid == self.document
            && st.selection.objects.first() == Some(&self.id)
            && st.doc.node(self.id).is_some_and(|n| n.extras().animation.as_deref() == self.source.as_ref())
    }
    fn time(&self) -> Option<f64> {
        Some(self.animation.delay + self.animation.duration * self.animation.keyframes.get(self.frame)?.time)
    }
}

pub(super) fn duration(doc: &Document, state: &Playback) -> f64 {
    let edit = state.editor.as_ref().filter(|_| state.document != 0);
    let mut end: f64 = edit.map_or(0.0, |e| e.animation.delay + e.animation.duration);
    doc.walk(|n| {
        if edit.is_none_or(|e| e.id != n.id)
            && let Some(a) = &n.extras().animation
        {
            end = end.max(a.delay + a.duration);
        }
    });
    end.clamp(0.0, 7200.0)
}

fn changed(app: &mut VectorcraftApp) {
    app.ui.animation.generation = app.ui.animation.generation.wrapping_add(1);
    app.canvas.key = None;
}

pub(super) fn sync_draft(app: &mut VectorcraftApp, id: NodeId, animation: &Animation, frame: usize) {
    if let Some(edit) = &mut app.ui.animation.editor
        && edit.id == id
        && (edit.animation != *animation || edit.frame != frame)
        && animation.validate().is_ok()
    {
        edit.animation = animation.clone();
        edit.frame = frame;
        if edit.on_canvas {
            app.ui.animation.time = edit.time().unwrap_or(app.ui.animation.time);
        }
        changed(app);
    }
}

pub fn edit_command(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    match p.get("action").and_then(Value::as_str).unwrap_or("begin") {
        "begin" => begin(app, p),
        "pose" => pose(app, p),
        "apply" => apply(app),
        "cancel" => {
            app.ui.animation.editor = None;
            app.ui.animation.enabled = false;
            app.ui.animation.playing = false;
            app.ui.animation.draft_epoch = app.ui.animation.draft_epoch.wrapping_add(1);
            changed(app);
            Ok(json!({"editing":false}))
        }
        _ => Err("action must be begin, pose, apply or cancel".into()),
    }
}

fn begin(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("open a document first")?;
    let id = match p.get("id") {
        Some(v) => NodeId(v.as_u64().ok_or("id must be an object id")?),
        None => st.selection.objects.first().copied().ok_or("select an object first")?,
    };
    if st.selection.objects.first() != Some(&id) {
        return Err("select the object to edit its keyframe".into());
    }
    if !st.doc.is_editable(id) {
        return Err("select visible, unlocked artwork".into());
    }
    let node = st.doc.node(id).ok_or("object is missing")?;
    let source = node.extras().animation.as_deref().cloned();
    let animation = match p.get("animation") {
        Some(v) => Animation::deserialize(v).map_err(|e| e.to_string())?,
        None => source.clone().unwrap_or_else(default_animation),
    };
    let frame = p.get("frame").map(|v| v.as_u64().ok_or("frame must be an index")).transpose()?.unwrap_or(0);
    let frame = usize::try_from(frame).map_err(|_| "invalid keyframe index")?;
    let edit = Editor { document: st.uid, id, source, animation, frame, on_canvas: true, drag: None };
    let time = edit.time().ok_or("keyframe is missing")?;
    geometry(&st.doc, &edit).ok_or("this object has no editable bounds, or a parent has a zero scale")?;
    app.ui.animation.document = st.uid;
    app.ui.animation.enabled = true;
    app.ui.animation.playing = false;
    app.ui.animation.time = time;
    app.ui.animation.last = None;
    app.ui.animation.editor = Some(edit);
    changed(app);
    Ok(json!({"editing":true,"id":id.0,"frame":frame,"time":time}))
}

fn pose(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let edit = app.ui.animation.editor.as_ref().ok_or("choose a keyframe for canvas editing first")?;
    if !edit.valid(app.session.doc().map_err(|e| e.to_string())?) {
        return Err("the object changed; choose its keyframe again".into());
    }
    let original = edit.animation.keyframes.get(edit.frame).ok_or("keyframe is missing")?;
    let mut value = serde_json::to_value(original).map_err(|e| e.to_string())?;
    let patch = p.get("pose").and_then(Value::as_object).ok_or("supply a pose object")?;
    if patch.len() > 10 {
        return Err("too many pose fields".into());
    }
    if let Some(fields) = value.as_object_mut() {
        for (key, v) in patch {
            if !fields.contains_key(key) || !v.as_f64().is_some_and(f64::is_finite) {
                return Err("unknown pose field or non-finite number".into());
            }
            fields.insert(key.clone(), v.clone());
        }
    }
    let keyframe = Keyframe::deserialize(&value).map_err(|e| e.to_string())?;
    if keyframe.time != original.time {
        return Err("pose editing keeps the keyframe's time".into());
    }
    let mut animation = edit.animation.clone();
    let slot = animation.keyframes.get_mut(edit.frame).ok_or("keyframe is missing")?;
    *slot = keyframe;
    animation.validate()?;
    if let Some(edit) = &mut app.ui.animation.editor {
        edit.animation = animation;
    }
    changed(app);
    Ok(json!({"editing":true}))
}

fn apply(app: &mut VectorcraftApp) -> Result<Value, String> {
    let edit = app.ui.animation.editor.as_ref().ok_or("choose a keyframe first")?;
    if !edit.valid(app.session.doc().map_err(|e| e.to_string())?) {
        return Err("the object changed; choose its keyframe again".into());
    }
    let (id, animation) = (edit.id, edit.animation.clone());
    let result = app.run("animation.set", json!({"id":id.0,"animation":animation}))?;
    if let Some(edit) = &mut app.ui.animation.editor {
        edit.source = Some(animation);
        edit.drag = None;
    }
    changed(app);
    Ok(result)
}

#[derive(Clone, Copy)]
struct Geometry {
    bounds: Rect,
    parent: Affine,
    pose: Affine,
}

fn inverse(a: Affine) -> Option<Affine> {
    (a.as_coeffs().iter().all(|v| v.is_finite()) && a.determinant().abs() > 1e-12).then(|| a.inverse())
}

fn geometry(doc: &Document, edit: &Editor) -> Option<Geometry> {
    let bounds = doc.node(edit.id)?.geometric_bounds()?;
    let keyframe = edit.animation.keyframes.get(edit.frame)?;
    let time = edit.time()?;
    let mut parent = Affine::IDENTITY;
    for id in doc.ancestry(edit.id)? {
        if id == edit.id {
            break;
        }
        let node = doc.node(id)?;
        if let Some(a) = &node.extras().animation {
            parent *= a.sample(time)?.transform(node.geometric_bounds()?);
        }
    }
    inverse(parent)?;
    Some(Geometry { bounds, parent, pose: keyframe.transform(bounds) })
}

fn corners(b: Rect) -> [Point; 4] {
    [Point::new(b.x0, b.y0), Point::new(b.x1, b.y0), Point::new(b.x1, b.y1), Point::new(b.x0, b.y1)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn app() -> VectorcraftApp {
        let mut s = vectorcraft_engine::Session::new();
        s.execute("file.new", &json!({"width":300,"height":200})).unwrap();
        s.execute("shape.rectangle", &json!({"x":20,"y":30,"width":80,"height":40})).unwrap();
        s.execute(
            "animation.set",
            &json!({"animation":{"duration":2,"delay":1,"repeat":true,
            "keyframes":[{"time":0,"x":10},{"time":1,"rotation":30,"scaleX":1.2,"scaleY":1.2,"opacity":0.25}]}}),
        )
        .unwrap();
        VectorcraftApp::new(s, Default::default())
    }

    #[test]
    fn pose_editing_previews_the_selected_repeating_endpoint_and_only_apply_is_undoable() {
        let mut app = app();
        let original = app.session.doc().unwrap().doc.clone();
        let id = app.session.doc().unwrap().selection.objects[0];
        let base = original.node(id).unwrap().geometric_bounds().unwrap();
        edit_command(&mut app, &json!({"frame":1})).unwrap();
        assert_eq!(app.ui.animation.time, 3.0);
        let rendered = super::super::document(app.session.doc().unwrap(), &app.ui.animation, &egui::Context::default());
        assert_eq!(rendered.node(id).unwrap().opacity, 0.25, "show the last pose, not the next repetition");
        assert_ne!(rendered.node(id).unwrap().geometric_bounds().unwrap(), base, "last keyframe must not wrap to the first");
        edit_command(&mut app, &json!({"action":"pose","pose":{"x":50,"rotation":60,"scaleX":2,"opacity":0.4}})).unwrap();
        assert!(Arc::ptr_eq(&original, &app.session.doc().unwrap().doc));
        edit_command(&mut app, &json!({"action":"apply"})).unwrap();
        let stored = app.session.doc().unwrap().doc.node(id).unwrap().extras().animation.as_deref().unwrap();
        assert_eq!(stored.keyframes[1].rotation, 60.0);
        assert_eq!(stored.keyframes[0].x, 10.0);
        assert_eq!(app.session.doc().unwrap().doc.node(id).unwrap().geometric_bounds().unwrap(), base);
        app.session.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.node(id).unwrap().extras().animation, original.node(id).unwrap().extras().animation);
        app.session.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(app.session.doc().unwrap().doc.node(id).unwrap().extras().animation.as_deref().unwrap().keyframes[1].rotation, 60.0);
    }

    #[test]
    fn cancel_and_invalid_pose_changes_leave_stored_artwork_intact() {
        let mut app = app();
        let original = app.session.doc().unwrap().doc.clone();
        edit_command(&mut app, &json!({"frame":0})).unwrap();
        for pose in [json!({"time":0.5}), json!({"scaleX":101}), json!({"opacity":-1}), json!({"x":"wrong"}), json!({"unknown":5})] {
            let before = app.ui.animation.editor.as_ref().unwrap().animation.clone();
            assert!(edit_command(&mut app, &json!({"action":"pose","pose":pose})).is_err());
            assert_eq!(app.ui.animation.editor.as_ref().unwrap().animation, before);
        }
        edit_command(&mut app, &json!({"action":"pose","pose":{"x":70}})).unwrap();
        edit_command(&mut app, &json!({"action":"cancel"})).unwrap();
        assert!(app.ui.animation.editor.is_none());
        assert!(Arc::ptr_eq(&original, &app.session.doc().unwrap().doc));
    }

    #[test]
    fn a_pose_edit_cannot_apply_to_a_different_document_with_the_same_object_ids() {
        let mut app = app();
        edit_command(&mut app, &json!({"frame":1})).unwrap();
        app.session.execute("file.new", &json!({})).unwrap();
        app.session.execute("shape.rectangle", &json!({"x":20,"y":30,"width":80,"height":40})).unwrap();
        let doc = app.session.doc().unwrap().doc.clone();
        assert!(edit_command(&mut app, &json!({"action":"apply"})).is_err());
        assert!(Arc::ptr_eq(&doc, &app.session.doc().unwrap().doc));
    }
}
