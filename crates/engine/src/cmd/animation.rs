//! Object animation editing, shared by the panel and automation clients.
use super::overprint::edit_counted;
use super::{CommandSpec, Result, Session, bad, cmd, has_doc, targets};
use serde::Deserialize;
use serde_json::{Value, json};
use vectorcraft_doc::{NodeId, animation::Animation};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "animation.info", "Animation", [], None,
            "{ids?|id?} → {objects: [{id, name, animation}], duration} editable tracks on the selection; duration includes all document tracks and delays",
            has_doc, info),
        cmd!(
            "animation.set",
            "Set Animation",
            [],
            None,
            "{ids?|id?, animation: {duration: seconds 0.001–3600, delay: seconds 0–3600, easing: [x1,y1,x2,y2], repeat: bool, keyframes: [{time: 0–1, x?, y?, xPercent?, yPercent?, rotation?: clockwise degrees, scaleX?: 1, scaleY?: 1, opacity?: 1}]}} replace each object's animation as one undo step; 2–256 increasing keyframes including 0 and 1, relative to the original artwork, opacity a multiplier → {changed}",
            has_doc,
            set
        ),
        cmd!(
            "animation.remove",
            "Remove Animation",
            [],
            None,
            "{ids?|id?} remove animation from the selection or given objects as one undo step → {changed}",
            has_doc,
            remove
        ),
    ]
}

fn info(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = targets(s, p)?;
    let d = &s.doc()?.doc;
    let objects: Vec<_> =
        ids.iter().filter_map(|id| d.node(*id)).map(|n| json!({"id": n.id.0, "name": n.name, "animation": n.extras().animation})).collect();
    Ok(json!({"objects":objects, "duration": vectorcraft_doc::animation::duration(d)}))
}

fn edit(s: &mut Session, p: &Value, command: &str, animation: Option<Animation>) -> Result<Value> {
    let ids = targets(s, p)?;
    if ids.is_empty() {
        return Err(bad(command, "select an object or supply ids"));
    }
    if ids.len() > 10000 {
        return Err(bad(command, "at most 10000 animation targets per command"));
    }
    for id in &ids {
        if s.doc()?.doc.node(*id).is_none() {
            return Err(bad(command, format!("object {} is missing", id.0)));
        }
        if !s.doc()?.doc.is_editable(*id) {
            return Err(bad(command, "unlock the object and its parents first"));
        }
    }
    let label = if animation.is_some() { "Set Animation" } else { "Remove Animation" };
    let changed = edit_counted(s, label, |d| {
        let mut changed = 0;
        for NodeId(id) in &ids {
            if let Some(n) = d.node_mut(NodeId(*id))
                && n.extras().animation.as_deref() != animation.as_ref()
            {
                n.extras_mut().animation = animation.clone().map(Box::new);
                changed += 1;
            }
        }
        Ok(changed)
    })?;
    Ok(json!({"changed":changed}))
}

fn set(s: &mut Session, p: &Value) -> Result<Value> {
    let a = p.get("animation").ok_or_else(|| bad("animation.set", "supply animation"))?;
    let a = Animation::deserialize(a).map_err(|e| bad("animation.set", e.to_string()))?;
    edit(s, p, "animation.set", Some(a))
}

fn remove(s: &mut Session, p: &Value) -> Result<Value> {
    edit(s, p, "animation.remove", None)
}
