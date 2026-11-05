//! Animation editor. Drafts become one command/Undo step on Apply; playback is render-only.
use crate::VectorcraftApp;
use egui::Ui;
use serde_json::{Value, json};
use vectorcraft_doc::animation::{Animation, Keyframe};

mod editor;
pub use editor::{canvas_input, edit_command, paint_handles};

/// Per-window preview controls, bound to the document's unique identity rather than its tab index.
#[derive(Clone, Debug, Default)]
pub struct Playback {
    pub document: u64,
    pub enabled: bool,
    pub playing: bool,
    pub time: f64,
    pub repeat: bool,
    last: Option<f64>,
    editor: Option<editor::Editor>,
    generation: u64,
    draft_epoch: u64,
}

/// Cache paused/scrubbed poses by source revision. Static frames keep their shared tree.
pub fn document(st: &vectorcraft_engine::DocState, state: &Playback, ctx: &egui::Context) -> std::sync::Arc<vectorcraft_doc::Document> {
    if !state.enabled || state.document != st.uid {
        return st.doc.clone();
    }
    type Cached = (u64, u64, f64, u64, std::sync::Arc<vectorcraft_doc::Document>);
    let id = egui::Id::unique("animation-render-document");
    if let Some((uid, revision, time, generation, doc)) = ctx.data_mut(|d| d.get_temp::<Cached>(id))
        && uid == st.uid
        && revision == st.revision
        && time == state.time
        && generation == state.generation
    {
        return doc;
    }
    let mut source = (*st.doc).clone();
    if let Some(edit) = &state.editor
        && let Some(n) = source.node_mut(edit.id)
        && n.extras().animation.as_deref() == edit.source.as_ref()
    {
        let mut animation = edit.animation.clone();
        if edit.on_canvas
            && let Some(pose) = animation.keyframes.get(edit.frame).cloned()
        {
            // Show the chosen pose exactly, including the last keyframe of a repeating track.
            for k in &mut animation.keyframes {
                let time = k.time;
                *k = pose.clone();
                k.time = time;
            }
        }
        n.extras_mut().animation = Some(Box::new(animation));
    }
    let doc = std::sync::Arc::new(vectorcraft_doc::animation::preview(&source, state.time));
    ctx.data_mut(|d| d.insert_temp(id, (st.uid, st.revision, state.time, state.generation, doc.clone())));
    doc
}

/// UI command used by the controls and the control channel/MCP.
pub fn control(app: &mut VectorcraftApp, p: &Value) -> Result<Value, String> {
    let st = app.session.active().ok_or("open a document first")?;
    let mut state = app.ui.animation.clone();
    if state.document != st.uid {
        state = Playback { document: st.uid, ..Default::default() };
    }
    let end = editor::duration(&st.doc, &state);
    for key in ["enabled", "playing", "restart", "repeat"] {
        if p.get(key).is_some_and(|v| !v.is_boolean()) {
            return Err(format!("{key} must be a boolean"));
        }
    }
    if let Some(v) = p.get("time") {
        let time = v.as_f64().filter(|t| t.is_finite() && (0.0..=7200.0).contains(t)).ok_or("time must be 0–7200 seconds")?;
        state.time = time.min(end);
        state.enabled = true;
        state.playing = false;
    }
    if p.get("restart").and_then(Value::as_bool) == Some(true) {
        state.time = 0.0;
        state.enabled = true;
    }
    if let Some(b) = p.get("enabled").and_then(Value::as_bool) {
        state.enabled = b;
    }
    if let Some(b) = p.get("playing").and_then(Value::as_bool) {
        state.playing = b && end > 0.0;
        if state.playing {
            state.enabled = true;
            if state.time >= end {
                state.time = 0.0;
            }
        }
    }
    if let Some(b) = p.get("repeat").and_then(Value::as_bool) {
        state.repeat = b;
    }
    if !state.enabled {
        state.playing = false;
    }
    if (state.playing || p.get("time").is_some() || !state.enabled)
        && let Some(edit) = &mut state.editor
    {
        edit.on_canvas = false;
        edit.drag = None;
    }
    state.last = None;
    app.ui.animation = state;
    app.canvas.key = None;
    Ok(json!({"enabled":app.ui.animation.enabled,"playing":app.ui.animation.playing,"time":app.ui.animation.time,"duration":end}))
}

/// Called by the canvas even with the panel closed. No wall-clock/calendar calculations.
pub fn tick(app: &mut VectorcraftApp, ctx: &egui::Context) {
    let Some(st) = app.session.active() else {
        app.ui.animation = Playback::default();
        return;
    };
    let state = &mut app.ui.animation;
    if state.document != st.uid {
        *state = Playback { document: st.uid, ..Default::default() };
    }
    if state.editor.as_ref().is_some_and(|e| !e.valid(st)) {
        state.editor = None;
        state.draft_epoch = state.draft_epoch.wrapping_add(1);
        state.generation = state.generation.wrapping_add(1);
        app.canvas.key = None;
    }
    if !state.enabled {
        return;
    }
    let end = editor::duration(&st.doc, state);
    let now = ctx.input(|i| i.time);
    if state.playing {
        if let Some(last) = state.last {
            state.time += (now - last).max(0.0);
        }
        if state.time >= end {
            if state.repeat && end > 0.0 {
                state.time %= end;
            } else {
                state.time = end;
                state.playing = false;
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
    state.time = state.time.min(end);
    state.last = Some(now);
}

#[derive(Clone)]
struct Draft {
    source: Option<Animation>,
    animation: Animation,
    frame: usize,
    epoch: u64,
}

fn default_animation() -> Animation {
    Animation {
        duration: 1.0,
        delay: 0.0,
        easing: [0.0, 0.0, 1.0, 1.0],
        repeat: false,
        keyframes: vec![Keyframe::default(), Keyframe { time: 1.0, ..Default::default() }],
    }
}

fn number(ui: &mut Ui, label: &str, v: &mut f64, range: std::ops::RangeInclusive<f64>) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(v).speed(0.01).range(range));
    });
}

pub fn show(app: &mut VectorcraftApp, ui: &mut Ui) {
    // app.run reports command failures in the app's status UI; the panel has no further recovery.
    let Some(st) = app.session.active() else {
        ui.label(tl!("Open a document first."));
        return;
    };
    let uid = st.uid;
    let end = editor::duration(&st.doc, &app.ui.animation);
    let selected = st.selection.objects.first().and_then(|id| st.doc.node(*id)).cloned();
    ui.horizontal(|ui| {
        let play = app.ui.animation.playing && app.ui.animation.document == uid;
        if ui.button(if play { tl!("Pause") } else { tl!("Play") }).clicked() {
            app.run("ui.animationPreview", json!({"playing": !play})).ok();
        }
        if ui.button(tl!("Restart")).clicked() {
            app.run("ui.animationPreview", json!({"restart":true,"playing":true})).ok();
        }
    });
    let mut enabled = app.ui.animation.enabled && app.ui.animation.document == uid;
    if ui.checkbox(&mut enabled, tl!("Preview")).changed() {
        app.run("ui.animationPreview", json!({"enabled":enabled})).ok();
    }
    let mut repeat = app.ui.animation.repeat;
    if ui.checkbox(&mut repeat, tl!("Loop preview")).changed() {
        app.run("ui.animationPreview", json!({"repeat":repeat})).ok();
    }
    let mut time = if app.ui.animation.document == uid { app.ui.animation.time } else { 0.0 };
    if ui.add(egui::Slider::new(&mut time, 0.0..=end.max(0.001)).text(tl!("Time (s)"))).changed() {
        app.run("ui.animationPreview", json!({"time":time})).ok();
    }
    ui.separator();
    let Some(node) = selected else {
        ui.label(tl!("Select an object to animate."));
        return;
    };
    ui.label(node.name.as_deref().unwrap_or(tl!("Selected object")));
    let id = egui::Id::unique(("animation-draft", uid, node.id.0));
    let source = node.extras().animation.as_deref().cloned();
    let epoch = app.ui.animation.draft_epoch;
    let mut draft = ui.ctx().data_mut(|d| d.get_temp::<Draft>(id)).filter(|d| d.source == source && d.epoch == epoch).unwrap_or_else(|| Draft {
        source: source.clone(),
        animation: source.clone().unwrap_or_else(default_animation),
        frame: 0,
        epoch,
    });
    if let Some(edit) = &app.ui.animation.editor
        && edit.id == node.id
        && edit.source == source
    {
        draft.animation = edit.animation.clone();
        draft.frame = edit.frame;
    }
    let mut begin_pose = false;
    let a = &mut draft.animation;
    number(ui, tl!("Duration (s)"), &mut a.duration, 0.001..=3600.0);
    number(ui, tl!("Delay (s)"), &mut a.delay, 0.0..=3600.0);
    ui.checkbox(&mut a.repeat, tl!("Repeat animation"));
    craft_ui::ComboBox::from_id_salt("animation-ease").selected_text(tl!("Easing presets")).show_ui(ui, |ui| {
        for (name, value) in [
            (tl!("Linear"), [0.0, 0.0, 1.0, 1.0]),
            (tl!("Ease"), [0.25, 0.1, 0.25, 1.0]),
            (tl!("Ease out"), [0.0, 0.0, 0.58, 1.0]),
            (tl!("Overshoot"), [0.2, 0.7, 0.2, 1.15]),
        ] {
            if ui.button(name).clicked() {
                a.easing = value;
                ui.close();
            }
        }
    });
    ui.collapsing(tl!("Cubic Bézier"), |ui| {
        for (i, label) in ["x1", "y1", "x2", "y2"].into_iter().enumerate() {
            if let Some(v) = a.easing.get_mut(i) {
                number(ui, label, v, if i % 2 == 0 { 0.0..=1.0 } else { -10.0..=10.0 });
            }
        }
    });
    ui.separator();
    craft_ui::ComboBox::from_id_salt("animation-keyframe").selected_text(format!("{} {}", tl!("Keyframe"), draft.frame + 1)).show_ui(ui, |ui| {
        for (index, k) in a.keyframes.iter().enumerate() {
            if ui.selectable_value(&mut draft.frame, index, format!("{} — {:.1}%", index + 1, k.time * 100.0)).clicked() {
                begin_pose = true;
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(a.keyframes.len() < 256, egui::Button::new(tl!("Add keyframe"))).clicked() {
            let t = ((time - a.delay) / a.duration).clamp(0.001, 0.999);
            if a.keyframes.iter().all(|k| (k.time - t).abs() > 0.00001) {
                let mut k = a.sample(a.delay + t * a.duration).unwrap_or_default();
                k.time = t;
                a.keyframes.push(k);
                a.keyframes.sort_by(|a, b| a.time.total_cmp(&b.time));
                draft.frame = a.keyframes.iter().position(|k| k.time == t).unwrap_or(0);
                begin_pose = true;
            }
        }
        if ui.add_enabled(draft.frame > 0 && draft.frame + 1 < a.keyframes.len(), egui::Button::new(tl!("Delete keyframe"))).clicked() {
            a.keyframes.remove(draft.frame);
            draft.frame = draft.frame.saturating_sub(1);
            begin_pose = true;
        }
    });
    if ui.button(tl!("Edit keyframe on canvas")).clicked() {
        begin_pose = true;
    }
    if app.ui.animation.editor.is_some() {
        ui.label(tl!("Drag inside to move, corners to scale, the round handle to rotate. Shift constrains."));
    }
    if let Some(k) = a.keyframes.get_mut(draft.frame) {
        let mut opacity = k.opacity * 100.0;
        if ui.add(egui::Slider::new(&mut opacity, 0.0..=100.0).suffix("%").text(tl!("Opacity"))).changed() {
            k.opacity = opacity / 100.0;
            if app.ui.animation.editor.is_none() {
                begin_pose = true;
            }
        }
    }
    let low = a.keyframes.get(draft.frame.wrapping_sub(1)).map(|k| k.time).unwrap_or(0.0);
    let high = a.keyframes.get(draft.frame + 1).map(|k| k.time).unwrap_or(1.0);
    let interior = draft.frame > 0 && draft.frame + 1 < a.keyframes.len();
    ui.collapsing(tl!("Numeric pose values"), |ui| {
        if let Some(k) = a.keyframes.get_mut(draft.frame) {
            if interior {
                number(ui, tl!("Keyframe time (0–1)"), &mut k.time, low..=high);
            }
            number(ui, tl!("X (pt)"), &mut k.x, -100000.0..=100000.0);
            number(ui, tl!("Y (pt)"), &mut k.y, -100000.0..=100000.0);
            number(ui, tl!("X (%)"), &mut k.x_percent, -100000.0..=100000.0);
            number(ui, tl!("Y (%)"), &mut k.y_percent, -100000.0..=100000.0);
            number(ui, tl!("Rotation (°)"), &mut k.rotation, -100000.0..=100000.0);
            number(ui, tl!("Scale X"), &mut k.scale_x, -100.0..=100.0);
            number(ui, tl!("Scale Y"), &mut k.scale_y, -100.0..=100.0);
            number(ui, tl!("Opacity (0–1)"), &mut k.opacity, 0.0..=1.0);
        }
    });
    if begin_pose {
        app.run("ui.animationEdit", json!({"action":"begin","id":node.id.0,"frame":draft.frame,"animation":a})).ok();
    } else {
        editor::sync_draft(app, node.id, a, draft.frame);
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button(tl!("Apply")).clicked() {
            let result = if app.ui.animation.editor.as_ref().is_some_and(|e| e.id == node.id) {
                app.run("ui.animationEdit", json!({"action":"apply"}))
            } else {
                app.run("animation.set", json!({"id":node.id.0,"animation":a}))
            };
            if result.is_ok() {
                draft.source = Some(a.clone());
            }
        }
        if ui.add_enabled(app.ui.animation.editor.is_some(), egui::Button::new(tl!("Cancel pose changes"))).clicked() {
            app.run("ui.animationEdit", json!({"action":"cancel"})).ok();
        }
        if ui.add_enabled(source.is_some(), egui::Button::new(tl!("Remove Animation"))).clicked() {
            app.run("animation.remove", json!({"id":node.id.0})).ok();
        }
    });
    if ui.button(tl!("Export animated SVG…")).clicked() {
        app.run("file.exportAs", json!({"format":"svg"})).ok();
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, draft));
}

#[cfg(test)]
mod tests {
    use super::*;
    use vectorcraft_engine::Session;

    fn app() -> VectorcraftApp {
        let mut s = Session::new();
        s.execute("file.new", &json!({"width":200,"height":200})).unwrap();
        s.execute("shape.rectangle", &json!({"x":20,"y":20,"width":40,"height":30})).unwrap();
        s.execute("animation.set", &json!({"animation":{"duration":2,"delay":1,"keyframes":[{"time":0,"x":50},{"time":1}]}})).unwrap();
        VectorcraftApp::new(s, Default::default())
    }

    #[test]
    fn panel_opens_and_draws_selected_track_headlessly() {
        let mut app = app();
        app.run("window.panel", json!({"panel":"animation"})).unwrap();
        assert!(app.ui.docking.as_ref().unwrap().contains(&"animation".to_string()));
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| show(&mut app, ui));
        assert!(!out.shapes.is_empty());
        assert!(out.platform_output.commands.is_empty());
        // This frame checks layout, without a backend to upload the font texture.
        out.textures_delta.clear();
    }

    #[test]
    fn controls_scrub_pause_and_disable_without_editing_and_reset_on_tab_change() {
        let mut app = app();
        let original = app.session.active().unwrap().doc.clone();
        let rev = app.session.active().unwrap().revision;
        assert!(control(&mut app, &json!({"time":-1})).is_err());
        assert!(control(&mut app, &json!({"playing":"yes"})).is_err());
        crate::menus::invoke(&mut app, "ui.animationPreview", json!({"time":2}));
        assert_eq!(app.ui.animation.time, 2.0);
        assert!(app.ui.animation.enabled);
        control(&mut app, &json!({"playing":true})).unwrap();
        assert!(app.ui.animation.playing);
        control(&mut app, &json!({"playing":false})).unwrap();
        assert_eq!(app.ui.animation.time, 2.0);
        control(&mut app, &json!({"enabled":false})).unwrap();
        assert!(!app.ui.animation.enabled);
        assert!(std::sync::Arc::ptr_eq(&original, &app.session.active().unwrap().doc));
        assert_eq!(app.session.active().unwrap().revision, rev);
        app.session.execute("file.new", &json!({})).unwrap();
        tick(&mut app, &egui::Context::default());
        assert!(!app.ui.animation.enabled);
    }

    #[test]
    fn paused_canvas_pose_is_cached_and_scrubbing_changes_its_geometry_only() {
        let mut app = app();
        let ctx = egui::Context::default();
        control(&mut app, &json!({"time":0})).unwrap();
        let st = app.session.active().unwrap();
        let a = document(st, &app.ui.animation, &ctx);
        let again = document(st, &app.ui.animation, &ctx);
        assert!(std::sync::Arc::ptr_eq(&a, &again));
        let id = st.selection.objects[0];
        let source = st.doc.node(id).unwrap().geometric_bounds().unwrap();
        assert_eq!(a.node(id).unwrap().geometric_bounds().unwrap().x0, source.x0 + 50.0);
        control(&mut app, &json!({"time":3})).unwrap();
        let st = app.session.active().unwrap();
        let end = document(st, &app.ui.animation, &ctx);
        assert_eq!(end.node(id).unwrap().geometric_bounds().unwrap(), source);
        control(&mut app, &json!({"enabled":false})).unwrap();
        let st = app.session.active().unwrap();
        assert!(std::sync::Arc::ptr_eq(&document(st, &app.ui.animation, &ctx), &st.doc));
    }

    /// Optional local visual fixture, kept outside the repository's asset corpus.
    #[test]
    fn capture_animation_panel() {
        let Some(path) = std::env::var_os("VECTORCRAFT_ANIMATION_CAPTURE") else { return };
        let mut app = app();
        if let Some(file) = std::env::var_os("VECTORCRAFT_ANIMATION_DOCUMENT") {
            app.session.execute("document.open", &json!({"path":file.to_string_lossy()})).unwrap();
            if let Ok(name) = std::env::var("VECTORCRAFT_ANIMATION_OBJECT") {
                app.session.execute("document.find", &json!({"name":name,"select":true})).unwrap();
            }
        }
        let mut ready = false;
        let mut harness = egui_kittest::Harness::builder().with_size(egui::vec2(340.0, 1100.0)).wgpu().build_ui_state(
            move |ui, app: &mut VectorcraftApp| {
                if !ready {
                    crate::theme::install_fonts(ui.ctx());
                    crate::theme::apply(ui.ctx(), app.ui.brightness);
                    ready = true;
                    return;
                }
                show(app, ui);
            },
            app,
        );
        harness.input_mut().max_texture_side = Some(8192);
        harness.run_steps(4);
        harness.render().unwrap().save(std::path::PathBuf::from(path)).unwrap();
    }
}
