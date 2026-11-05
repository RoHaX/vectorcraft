use super::*;
use vectorcraft_doc::animation::{Animation, preview};

fn track() -> Value {
    json!({"duration":2.0,"delay":1.0,"easing":[0.0,0.0,1.0,1.0],"repeat":false,
        "keyframes":[{"time":0.0,"x":80.0,"rotation":30.0,"scaleX":0.5,"opacity":0.0},{"time":1.0}]})
}

fn setup() -> (Session, u64) {
    let mut s = Session::new();
    s.execute("file.new", &json!({"width":300,"height":200})).unwrap();
    let id = s.execute("shape.rectangle", &json!({"x":10,"y":20,"width":40,"height":20})).unwrap()["id"].as_u64().unwrap();
    (s, id)
}

#[test]
fn animation_undo_redo_native_round_trip_and_noop() {
    let (mut s, id) = setup();
    s.execute("animation.set", &json!({"animation":track()})).unwrap();
    let before = s.doc().unwrap().doc.clone();
    assert_eq!(s.execute("animation.set", &json!({"animation":track()})).unwrap()["changed"], 0);
    assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    let bytes = serde_json::to_vec(before.as_ref()).unwrap();
    let loaded: Document = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(loaded.node(NodeId(id)).unwrap().extras().animation.as_deref().unwrap().delay, 1.0);
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.node(NodeId(id)).unwrap().extras().animation.is_none());
    s.execute("edit.redo", &json!({})).unwrap();
    assert_eq!(s.execute("animation.info", &json!({})).unwrap()["duration"], 3.0);
    s.execute("animation.remove", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.node(NodeId(id)).unwrap().extras().animation.is_none());
    s.execute("edit.undo", &json!({})).unwrap();
    assert!(s.doc().unwrap().doc.node(NodeId(id)).unwrap().extras().animation.is_some());
}

#[test]
fn preview_is_relative_and_preserves_document_and_history() {
    let (mut s, id) = setup();
    s.execute("animation.set", &json!({"animation":track()})).unwrap();
    let original = s.doc().unwrap().doc.clone();
    let revision = s.doc().unwrap().revision;
    let bytes = serde_json::to_vec(original.as_ref()).unwrap();
    let posed = preview(&original, 3.0);
    let b = posed.node(NodeId(id)).unwrap().geometric_bounds().unwrap();
    assert_eq!(b, original.node(NodeId(id)).unwrap().geometric_bounds().unwrap());
    assert_eq!(preview(&original, 0.0).node(NodeId(id)).unwrap().opacity, 0.0);
    let middle = preview(&original, 2.0);
    assert!((middle.node(NodeId(id)).unwrap().opacity - 0.5).abs() < 1e-6);
    assert_eq!(serde_json::to_vec(original.as_ref()).unwrap(), bytes);
    assert_eq!(s.doc().unwrap().revision, revision);
}

#[test]
fn invalid_tracks_and_locked_targets_are_atomic() {
    let (mut s, id) = setup();
    for invalid in [
        json!({"duration":0}),
        json!({"keyframes":[{"time":0},{"time":0}]}),
        json!({"easing":[-1,0,1,1]}),
        json!({"keyframes":[{"time":0,"opacity":2},{"time":1}]}),
        json!({"keyframes":vec![json!({"time":0});257]}),
        json!({"delay":-1}),
    ] {
        let before = s.doc().unwrap().doc.clone();
        assert!(s.execute("animation.set", &json!({"animation":invalid})).is_err());
        assert!(Arc::ptr_eq(&before, &s.doc().unwrap().doc));
    }
    assert!(s.execute("animation.set", &json!({"ids":[id,999999],"animation":track()})).is_err());
    assert!(s.doc().unwrap().doc.node(NodeId(id)).unwrap().extras().animation.is_none());
    s.execute("object.lock", &json!({})).unwrap();
    assert!(s.execute("animation.set", &json!({"id":id,"animation":track()})).is_err());
}

#[test]
fn percentage_translation_pivot_and_easing_follow_css() {
    let a: Animation = serde_json::from_value(json!({"duration":1,"delay":0.2,"easing":[0.2,0.7,0.2,1.15],
        "keyframes":[{"time":0,"xPercent":40,"rotation":90,"scaleX":2,"scaleY":2},{"time":1}]}))
    .unwrap();
    let k = a.sample(0.0).unwrap();
    let b = vectorcraft_geom::Rect::new(10.0, 20.0, 50.0, 40.0);
    assert!((k.transform(b) * b.center() - Point::new(46.0, 30.0)).hypot() < 1e-9);
    assert!(a.sample(0.7).unwrap().x_percent < 20.0, "ease reaches farther than linear at midpoint");
    let mut repeated = a.clone();
    repeated.repeat = true;
    assert_eq!(repeated.sample(1.2).unwrap().x_percent, 40.0);
    assert!(a.sample(f64::NAN).is_none());
}
