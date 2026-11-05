//! CSS animations on wrappers, so authored geometry and existing transforms stay intact.
use super::Writer;
use vectorcraft_doc::Node;

impl Writer<'_> {
    /// Open a wrapper and define its CSS. A generated identifier avoids selector escaping and
    /// follows Unique object IDs, even when authored names contain punctuation or Unicode.
    pub(super) fn animation_start(&mut self, n: &Node) -> bool {
        let Some(a) = n.extras().animation.as_deref() else { return false };
        if a.validate().is_err() {
            self.warn("Invalid object animation was left out of SVG");
            return false;
        }
        if self.tiny() {
            self.warn("CSS object animation is not supported by SVG Tiny 1.2; exported static artwork");
            return false;
        }
        if self.anonymous {
            return false;
        }
        let Some(bounds) = n.geometric_bounds() else { return false };
        let center = self.xf * bounds.center();
        let metadata = match serde_json::to_string(a) {
            Ok(data) => super::output::xml_attr_escape(&data),
            Err(_) => {
                self.warn("Object animation could not be serialized; exported static artwork");
                return false;
            }
        };
        let name = self.fresh_id("vc-motion");
        // Timing and normalized keyframe positions must survive even zero-decimal art export.
        let number = |v: f64| v.to_string();
        let mut rule = format!("@keyframes {name}{{");
        for k in &a.keyframes {
            rule.push_str(&format!(
                "{}%{{transform:translate({}px,{}px) rotate({}deg) scale({},{});opacity:{};}}",
                number(k.time * 100.0),
                number(k.x + k.x_percent * bounds.width() / 100.0),
                number(k.y + k.y_percent * bounds.height() / 100.0),
                number(k.rotation),
                number(k.scale_x),
                number(k.scale_y),
                number(k.opacity)
            ));
        }
        rule.push('}');
        self.def(1, &format!("<style>{rule}</style>"));
        let [x1, y1, x2, y2] = a.easing;
        let repeat = if a.repeat { "infinite" } else { "1" };
        // Fix the reference box to the source artwork. An animated child must not move its
        // parent's pivot or change the meaning of its percentage translations mid-animation.
        self.line(&format!("<g data-vectorcraft-animation=\"{metadata}\" style=\"transform-box:view-box;transform-origin:{}px {}px;animation:{name} {}s cubic-bezier({},{},{},{}) {}s {repeat} both\">",
            number(center.x),number(center.y),number(a.duration),number(x1),number(y1),number(x2),number(y2),number(a.delay)));
        self.depth += 1;
        true
    }
}
