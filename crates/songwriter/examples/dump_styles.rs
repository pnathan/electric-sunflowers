//! Prints the style and form tables as JSON, for the browser page that
//! rebuilds the songwriter prompt (crates/wasm/page.tpl.html).
use serde_json::json;
use songwriter::styles::{FormId, StyleId, WORLD_FLAVOURS};

fn main() {
    let styles: Vec<_> = StyleId::ALL
        .iter()
        .map(|id| {
            let s = id.style();
            json!({
                "id": id.as_str(),
                "label": s.label,
                "idiom": s.idiom,
                "meters": s.meters.iter().map(|m| m.to_string()).collect::<Vec<_>>(),
                "tempo": s.tempo.iter().map(|(m, lo, hi)| json!([m.to_string(), lo, hi])).collect::<Vec<_>>(),
                "modes": s.modes.iter().map(|m| m.to_string()).collect::<Vec<_>>(),
                "guitar": s.guitar.to_string(),
                "band": s.band,
                "forms": s.forms.iter().map(|f| f.as_str()).collect::<Vec<_>>(),
                "duet": s.duet.as_str(),
                "delivery": s.phrasing.delivery.to_string(),
                "endings": s.phrasing.endings.to_string(),
            })
        })
        .collect();
    let forms: Vec<_> = FormId::ALL
        .iter()
        .map(|id| {
            let f = id.form();
            json!({"id": id.as_str(), "label": f.label, "note": f.note, "plan": f.plan_text()})
        })
        .collect();
    println!("{}", json!({"styles": styles, "forms": forms, "world": WORLD_FLAVOURS}));
}
