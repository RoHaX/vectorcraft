//! Editable CSS-style object animation. Times are seconds; poses are relative to the artwork.
//! Semantics: https://www.w3.org/TR/css-animations-1/ and CSS Transforms Level 1.

use crate::{Document, Node};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use vectorcraft_geom::{Affine, Rect};

/// A keyframe's normalized time, translation (points and percent of the original geometry box),
/// clockwise rotation, scale and opacity multiplier. Rotation and scale use the box centre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Keyframe {
    pub time: f64,
    pub x: f64,
    pub y: f64,
    pub x_percent: f64,
    pub y_percent: f64,
    pub rotation: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub opacity: f64,
}

impl Default for Keyframe {
    fn default() -> Self {
        Self { time: 0.0, x: 0.0, y: 0.0, x_percent: 0.0, y_percent: 0.0, rotation: 0.0, scale_x: 1.0, scale_y: 1.0, opacity: 1.0 }
    }
}

/// One animation per object, with CSS `both` fill and optional infinite repetition.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "AnimationWire")]
pub struct Animation {
    pub duration: f64,
    pub delay: f64,
    pub easing: [f64; 4],
    pub repeat: bool,
    pub keyframes: Vec<Keyframe>,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct AnimationWire {
    duration: f64,
    delay: f64,
    easing: [f64; 4],
    repeat: bool,
    #[serde(deserialize_with = "read_keyframes")]
    keyframes: Vec<Keyframe>,
}

impl Default for AnimationWire {
    fn default() -> Self {
        Self {
            duration: 1.0,
            delay: 0.0,
            easing: [0.0, 0.0, 1.0, 1.0],
            repeat: false,
            keyframes: vec![Keyframe::default(), Keyframe { time: 1.0, ..Default::default() }],
        }
    }
}

// Bound allocation while reading native files as well as command parameters.
fn read_keyframes<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<Keyframe>, D::Error> {
    struct Frames;
    impl<'de> serde::de::Visitor<'de> for Frames {
        type Value = Vec<Keyframe>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("2 to 256 keyframes")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut frames = Vec::new();
            while let Some(frame) = seq.next_element()? {
                if frames.len() >= 256 {
                    return Err(serde::de::Error::custom("at most 256 keyframes"));
                }
                frames.push(frame);
            }
            Ok(frames)
        }
    }
    d.deserialize_seq(Frames)
}

impl TryFrom<AnimationWire> for Animation {
    type Error = String;
    fn try_from(w: AnimationWire) -> Result<Self, String> {
        let a = Self { duration: w.duration, delay: w.delay, easing: w.easing, repeat: w.repeat, keyframes: w.keyframes };
        a.validate()?;
        // Include defaulted frames too, and charge native files across all their tracks.
        for _ in &a.keyframes {
            crate::node::count_read_item::<serde::de::value::Error>().map_err(|e| e.to_string())?;
        }
        Ok(a)
    }
}

impl Animation {
    /// Check every number, ordering and supported limit before evaluating or exporting.
    pub fn validate(&self) -> Result<(), String> {
        let bounded = |x: f64, low: f64, high: f64| x.is_finite() && (low..=high).contains(&x);
        if !bounded(self.duration, 0.001, 3600.0) || !bounded(self.delay, 0.0, 3600.0) {
            return Err("duration must be 0.001–3600 seconds; delay 0–3600".into());
        }
        let [x1, y1, x2, y2] = self.easing;
        if !bounded(x1, 0.0, 1.0) || !bounded(x2, 0.0, 1.0) || !bounded(y1, -10.0, 10.0) || !bounded(y2, -10.0, 10.0) {
            return Err("invalid cubic-bezier control points".into());
        }
        if !(2..=256).contains(&self.keyframes.len())
            || self.keyframes.first().is_none_or(|k| k.time != 0.0)
            || self.keyframes.last().is_none_or(|k| k.time != 1.0)
        {
            return Err("use 2–256 keyframes, starting at 0 and ending at 1".into());
        }
        let mut previous = -1.0;
        for k in &self.keyframes {
            if !bounded(k.time, 0.0, 1.0)
                || k.time <= previous
                || ![k.x, k.y, k.x_percent, k.y_percent, k.rotation].iter().all(|v| bounded(*v, -100_000.0, 100_000.0))
                || ![k.scale_x, k.scale_y].iter().all(|v| bounded(*v, -100.0, 100.0))
                || !bounded(k.opacity, 0.0, 1.0)
            {
                return Err("keyframes need increasing times and finite, bounded pose values".into());
            }
            previous = k.time;
        }
        Ok(())
    }

    /// Pose at `seconds`, including the first pose during the delay and the last after completion.
    pub fn sample(&self, seconds: f64) -> Option<Keyframe> {
        if !seconds.is_finite() || self.validate().is_err() {
            return None;
        }
        let elapsed = (seconds - self.delay).max(0.0) / self.duration;
        let time = if self.repeat { elapsed % 1.0 } else { elapsed.min(1.0) };
        let first = self.keyframes.first()?;
        let last = self.keyframes.last()?;
        if time <= 0.0 {
            return Some(first.clone());
        }
        if time >= 1.0 {
            return Some(last.clone());
        }
        let pair = self.keyframes.windows(2).find(|p| p.first().is_some_and(|a| a.time <= time) && p.get(1).is_some_and(|b| time <= b.time))?;
        let (a, b) = (pair.first()?, pair.get(1)?);
        let t = bezier((time - a.time) / (b.time - a.time), self.easing);
        let mix = |a: f64, b: f64| a + (b - a) * t;
        Some(Keyframe {
            time,
            x: mix(a.x, b.x),
            y: mix(a.y, b.y),
            x_percent: mix(a.x_percent, b.x_percent),
            y_percent: mix(a.y_percent, b.y_percent),
            rotation: mix(a.rotation, b.rotation),
            scale_x: mix(a.scale_x, b.scale_x),
            scale_y: mix(a.scale_y, b.scale_y),
            opacity: mix(a.opacity, b.opacity).clamp(0.0, 1.0),
        })
    }
}

/// Solve the monotone x curve, then evaluate y (which may overshoot).
fn bezier(t: f64, [x1, y1, x2, y2]: [f64; 4]) -> f64 {
    let curve = |u: f64, a: f64, b: f64| 3.0 * (1.0 - u).powi(2) * u * a + 3.0 * (1.0 - u) * u * u * b + u * u * u;
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..40 {
        let u = (low + high) * 0.5;
        if curve(u, x1, x2) < t {
            low = u;
        } else {
            high = u;
        }
    }
    curve((low + high) * 0.5, y1, y2)
}

impl Keyframe {
    /// Relative transform in document coordinates, using the unanimated geometric bounds.
    pub fn transform(&self, b: Rect) -> Affine {
        let c = b.center().to_vec2();
        Affine::translate((self.x + self.x_percent * b.width() / 100.0, self.y + self.y_percent * b.height() / 100.0))
            * Affine::translate(c)
            * Affine::rotate(self.rotation.to_radians())
            * Affine::scale_non_uniform(self.scale_x, self.scale_y)
            * Affine::translate(-c)
    }
}

/// Render-only copy. Children animate before their parent's transform; source geometry, history
/// and dirty state are never touched. A depth bound also protects callers constructing raw data.
pub fn preview(doc: &Document, seconds: f64) -> Document {
    fn pose(n: &Arc<Node>, time: f64, depth: usize) -> Option<Node> {
        if depth > crate::MAX_DEPTH {
            return None;
        }
        let key = n.extras().animation.as_ref().and_then(|a| a.sample(time));
        let mut copy = None;
        if let Some(children) = n.children() {
            for (index, c) in children.iter().enumerate() {
                if let Some(child) = pose(c, time, depth + 1) {
                    let parent = copy.get_or_insert_with(|| n.as_ref().clone());
                    if let Some(slot) = parent.children_mut().and_then(|v| v.get_mut(index)) {
                        *slot = Arc::new(child);
                    }
                }
            }
        }
        if let Some(k) = key {
            let copy = copy.get_or_insert_with(|| n.as_ref().clone());
            if let Some(b) = n.geometric_bounds() {
                copy.transform(k.transform(b), crate::node::Scaling { strokes: true, patterns: true, ..Default::default() });
            }
            copy.opacity *= k.opacity as f32;
        }
        copy
    }
    let mut copy = doc.clone();
    for layer in &mut copy.layers {
        if let Some(n) = pose(layer, seconds, 0) {
            *layer = Arc::new(n);
        }
    }
    copy
}

/// Duration of one full preview pass, including delays (repeating tracks play one iteration).
pub fn duration(doc: &Document) -> f64 {
    let mut end: f64 = 0.0;
    doc.walk(|n| {
        if let Some(a) = &n.extras().animation {
            end = end.max(a.delay + a.duration);
        }
    });
    end.clamp(0.0, 7200.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_keyframes_share_the_document_allocation_budget() {
        let mut node = Node::group(crate::NodeId(1), Vec::new());
        node.extras_mut().animation = Some(Box::new(serde_json::from_str::<Animation>("{}").unwrap()));
        let bytes = serde_json::to_vec(&node).unwrap();
        assert!(crate::node::with_node_budget(2, || serde_json::from_slice::<Node>(&bytes)).is_err());
        assert!(crate::node::with_node_budget(3, || serde_json::from_slice::<Node>(&bytes)).is_ok());
    }

    #[test]
    fn animation_keyframes_count_towards_the_undo_memory_budget() {
        let mut small = Document::new(100.0, 100.0);
        let mut node = Node::group(crate::NodeId(100), Vec::new());
        node.extras_mut().animation = Some(Box::new(serde_json::from_str::<Animation>("{}").unwrap()));
        small.layers = vec![Arc::new(node)];
        let mut large = small.clone();
        let node = Arc::make_mut(large.layers.first_mut().unwrap());
        node.extras_mut().animation.as_mut().unwrap().keyframes =
            (0..256).map(|i| Keyframe { time: f64::from(i) / 255.0, ..Default::default() }).collect();
        assert!(large.retained_bytes(&small) >= 256 * std::mem::size_of::<Keyframe>());
    }
}
