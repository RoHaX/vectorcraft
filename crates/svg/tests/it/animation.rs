use vectorcraft_doc::animation::Animation;
use vectorcraft_svg::{ExportOptions, ObjectIds, Profile, export_full, import};

fn doc() -> vectorcraft_doc::Document {
    let mut doc = import(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><g id="motion"><rect x="10" y="20" width="30" height="20"/></g></svg>"#,
    )
    .unwrap();
    let mut id = None;
    doc.walk(|n| {
        if n.name.as_deref() == Some("motion") {
            id = Some(n.id);
        }
    });
    let a: Animation = serde_json::from_value(serde_json::json!({"duration":2.5,"delay":0.4,"easing":[0.25,0.1,0.25,1],
        "keyframes":[{"time":0,"xPercent":50,"rotation":20,"scaleX":0.8,"opacity":0},{"time":0.5,"x":5},{"time":1}]}))
    .unwrap();
    doc.node_mut(id.unwrap()).unwrap().extras_mut().animation = Some(Box::new(a));
    doc
}

#[test]
fn export_writes_valid_css_on_a_wrapper_and_preserves_object_ids() {
    for object_ids in [ObjectIds::Minimal, ObjectIds::Unique] {
        let out = export_full(&doc(), &ExportOptions { object_ids, ..Default::default() }, None);
        let xml = usvg::roxmltree::Document::parse(&out.svg).unwrap();
        let wrapper = xml.descendants().find(|n| n.attribute("style").is_some_and(|s| s.contains("animation:"))).unwrap();
        assert!(wrapper.attribute("style").unwrap().contains("2.5s cubic-bezier(0.25,0.1,0.25,1) 0.4s 1 both"));
        assert!(wrapper.attribute("style").unwrap().contains("transform-origin:25px 30px"));
        let css = xml.descendants().filter(|n| n.has_tag_name("style")).filter_map(|n| n.text()).collect::<String>();
        assert!(css.contains("@keyframes"));
        assert!(css.contains("translate(15px,0px)"));
        assert!(css.contains("rotate(20deg) scale(0.8,1);opacity:0"));
        assert!(css.contains("50%{transform:"));
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
        assert_eq!(xml.descendants().filter(|n| n.has_tag_name("rect")).count(), 1);
    }
}

#[test]
fn tiny_profile_warns_and_keeps_static_art() {
    let out = export_full(&doc(), &ExportOptions { profile: Profile::Tiny12, ..Default::default() }, None);
    assert!(!out.svg.contains("@keyframes"));
    assert!(out.warnings.iter().any(|w| w.contains("CSS object animation")));
}

#[test]
fn exported_animation_reopens_without_accumulating_wrappers_or_tracks() {
    let original = doc();
    let mut expected = None;
    original.walk(|n| {
        if let Some(a) = &n.extras().animation {
            expected = Some(a.clone());
        }
    });
    let mut current = original;
    for _ in 0..3 {
        let out = export_full(&current, &ExportOptions::default(), None);
        current = import(&out.svg).unwrap();
        let mut tracks = Vec::new();
        current.walk(|n| {
            if let Some(a) = &n.extras().animation {
                tracks.push(a.clone());
            }
        });
        assert_eq!(tracks, vec![expected.clone().unwrap()]);
    }
}

#[test]
fn geometry_precision_does_not_round_away_animation_timing() {
    let mut d = doc();
    let mut id = None;
    d.walk(|n| {
        if n.extras().animation.is_some() {
            id = Some(n.id);
        }
    });
    d.node_mut(id.unwrap()).unwrap().extras_mut().animation.as_mut().unwrap().duration = 0.001;
    let out = export_full(&d, &ExportOptions { decimals: 0, ..Default::default() }, None);
    assert!(out.svg.contains("0.001s cubic-bezier"));
}
