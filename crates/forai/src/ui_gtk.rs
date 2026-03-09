use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub struct GtkStyle {
    pub values: BTreeMap<String, String>,
}

impl GtkStyle {
    fn from_props(props: &serde_json::Map<String, Value>) -> Self {
        let mut values = BTreeMap::new();

        let put_len = |values: &mut BTreeMap<String, String>, key: &str, v: &Value| {
            if let Some(n) = v.as_i64() {
                values.insert(key.to_string(), format!("{n}px"));
            } else if let Some(s) = v.as_str() {
                values.insert(key.to_string(), s.to_string());
            }
        };

        if let Some(v) = props.get("padding") {
            put_len(&mut values, "padding", v);
        }
        if let Some(v) = props.get("margin") {
            put_len(&mut values, "margin", v);
        }
        if let Some(v) = props.get("width") {
            put_len(&mut values, "width", v);
        }
        if let Some(v) = props.get("height") {
            put_len(&mut values, "height", v);
        }
        if let Some(v) = props.get("spacing").and_then(|v| v.as_i64()) {
            values.insert("spacing".into(), format!("{v}px"));
        }
        if let Some(v) = props.get("color").and_then(|v| v.as_str()) {
            values.insert("color".into(), v.to_string());
        }
        if let Some(v) = props.get("bg").and_then(|v| v.as_str()) {
            values.insert("bg".into(), v.to_string());
        }
        if let Some(v) = props.get("align").and_then(|v| v.as_str()) {
            values.insert("align".into(), v.to_string());
        }
        if props.get("bold").and_then(|v| v.as_bool()).unwrap_or(false) {
            values.insert("bold".into(), "true".into());
        }
        if props
            .get("italic")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            values.insert("italic".into(), "true".into());
        }

        Self { values }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GtkNode {
    pub node_type: String,
    pub text: Option<String>,
    pub props: serde_json::Map<String, Value>,
    pub style: GtkStyle,
    pub events: serde_json::Map<String, Value>,
    pub children: Vec<GtkNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GtkPatchOp {
    Replace,
    UpdateProps,
    UpdateEvents,
    UpdateText,
    AppendChild,
    RemoveChild,
}

#[derive(Debug, Default, Clone)]
pub struct GtkUiState {
    current: Option<GtkNode>,
}

impl GtkUiState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn mount(&mut self, tree: &Value) -> Result<(), String> {
        self.current = Some(parse_node(tree)?);
        Ok(())
    }

    pub fn update(&mut self, tree: &Value) -> Result<Vec<GtkPatchOp>, String> {
        let next = parse_node(tree)?;
        let ops = if let Some(prev) = &self.current {
            diff_nodes(prev, &next)
        } else {
            vec![GtkPatchOp::Replace]
        };
        self.current = Some(next);
        Ok(ops)
    }
}

fn node_text(node_type: &str, props: &serde_json::Map<String, Value>) -> Option<String> {
    match node_type {
        "text" => props
            .get("value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "html" => props
            .get("html")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "button" => props
            .get("label")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        "input" => props
            .get("value")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                props
                    .get("placeholder")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }),
        "toggle" => Some(
            props
                .get("value")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
                .to_string(),
        ),
        _ => None,
    }
}

pub fn parse_node(v: &Value) -> Result<GtkNode, String> {
    let obj = v
        .as_object()
        .ok_or_else(|| "ui.gtk: expected object node".to_string())?;
    let node_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "ui.gtk: node missing `type`".to_string())?;

    let props = obj
        .get("props")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let events = obj
        .get("events")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();

    let children_raw = obj
        .get("children")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut children = Vec::with_capacity(children_raw.len());
    for child in children_raw {
        children.push(parse_node(&child)?);
    }

    Ok(GtkNode {
        node_type: node_type.to_string(),
        text: node_text(node_type, &props),
        style: GtkStyle::from_props(&props),
        props,
        events,
        children,
    })
}

pub fn diff_nodes(prev: &GtkNode, next: &GtkNode) -> Vec<GtkPatchOp> {
    let mut ops = Vec::new();

    if prev.node_type != next.node_type {
        ops.push(GtkPatchOp::Replace);
        return ops;
    }
    if prev.text != next.text {
        ops.push(GtkPatchOp::UpdateText);
    }
    if prev.props != next.props || prev.style != next.style {
        ops.push(GtkPatchOp::UpdateProps);
    }
    if prev.events != next.events {
        ops.push(GtkPatchOp::UpdateEvents);
    }

    if prev.children.len() < next.children.len() {
        for _ in 0..(next.children.len() - prev.children.len()) {
            ops.push(GtkPatchOp::AppendChild);
        }
    } else if prev.children.len() > next.children.len() {
        for _ in 0..(prev.children.len() - next.children.len()) {
            ops.push(GtkPatchOp::RemoveChild);
        }
    }

    let n = prev.children.len().min(next.children.len());
    for i in 0..n {
        ops.extend(diff_nodes(&prev.children[i], &next.children[i]));
    }

    ops
}

#[cfg_attr(not(feature = "linux-gtk"), allow(dead_code))]
pub fn normalize_event(node_type: &str, action: &str, value: Value) -> Value {
    let event_type = match node_type {
        "input" => "input",
        "toggle" => "toggle",
        _ => "action",
    };
    json!({
        "type": event_type,
        "action": action,
        "value": value
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use forai_core::ui_html;
    use serde_json::json;

    #[test]
    fn parse_basic_node() {
        let n = parse_node(&json!({
            "type": "vstack",
            "props": {"spacing": 8, "padding": 12},
            "children": [{"type":"text","props":{"value":"Hi"},"children":[]}]
        }))
        .unwrap();
        assert_eq!(n.node_type, "vstack");
        assert_eq!(n.children.len(), 1);
        assert_eq!(n.style.values.get("spacing"), Some(&"8px".to_string()));
        assert_eq!(n.style.values.get("padding"), Some(&"12px".to_string()));
    }

    #[test]
    fn diff_replace_on_type_change() {
        let a = parse_node(&json!({"type":"text","props":{"value":"A"},"children":[]})).unwrap();
        let b = parse_node(&json!({"type":"button","props":{"label":"A"},"children":[]})).unwrap();
        let ops = diff_nodes(&a, &b);
        assert_eq!(ops, vec![GtkPatchOp::Replace]);
    }

    #[test]
    fn diff_update_remove_append() {
        let a = parse_node(&json!({
            "type":"vstack","props":{"spacing":4},"children":[
                {"type":"text","props":{"value":"A"},"children":[]},
                {"type":"button","props":{"label":"Old"},"children":[]}
            ]
        }))
        .unwrap();
        let b = parse_node(&json!({
            "type":"vstack","props":{"spacing":10},"children":[
                {"type":"text","props":{"value":"B"},"children":[]},
                {"type":"input","props":{"value":"x"},"children":[]},
                {"type":"button","props":{"label":"New"},"children":[]}
            ]
        }))
        .unwrap();
        let ops = diff_nodes(&a, &b);
        assert!(ops.contains(&GtkPatchOp::UpdateProps));
        assert!(ops.contains(&GtkPatchOp::AppendChild));
        assert!(ops.contains(&GtkPatchOp::UpdateText) || ops.contains(&GtkPatchOp::Replace));
    }

    #[test]
    fn state_mount_update() {
        let mut s = GtkUiState::new();
        s.mount(&json!({"type":"text","props":{"value":"A"},"children":[]}))
            .unwrap();
        let ops = s
            .update(&json!({"type":"text","props":{"value":"B"},"children":[]}))
            .unwrap();
        assert!(ops.contains(&GtkPatchOp::UpdateText));
    }

    #[test]
    fn parse_node_extracts_widget_text_and_events() {
        let button = parse_node(&json!({
            "type":"button",
            "props":{"label":"About"},
            "events":{"on_about":true},
            "children":[]
        }))
        .unwrap();
        assert_eq!(button.text.as_deref(), Some("About"));
        assert_eq!(button.events.get("on_about"), Some(&json!(true)));

        let input = parse_node(&json!({
            "type":"input",
            "props":{"placeholder":"Name","value":"Ada"},
            "events":{"on_name":true},
            "children":[]
        }))
        .unwrap();
        assert_eq!(input.text.as_deref(), Some("Ada"));
        assert_eq!(input.events.get("on_name"), Some(&json!(true)));

        let toggle = parse_node(&json!({
            "type":"toggle",
            "props":{"value":true},
            "events":{"on_enabled":true},
            "children":[]
        }))
        .unwrap();
        assert_eq!(toggle.text.as_deref(), Some("true"));
        assert_eq!(toggle.events.get("on_enabled"), Some(&json!(true)));
    }

    #[test]
    fn style_mapping_covers_common_properties() {
        let n = parse_node(&json!({
            "type":"text",
            "props":{
                "padding":12,
                "margin":"4px",
                "width":320,
                "height":40,
                "spacing":8,
                "color":"#eee",
                "bg":"#111",
                "align":"center",
                "bold":true,
                "italic":true,
                "value":"x"
            },
            "children":[]
        }))
        .unwrap();

        let s = &n.style.values;
        assert_eq!(s.get("padding"), Some(&"12px".to_string()));
        assert_eq!(s.get("margin"), Some(&"4px".to_string()));
        assert_eq!(s.get("width"), Some(&"320px".to_string()));
        assert_eq!(s.get("height"), Some(&"40px".to_string()));
        assert_eq!(s.get("spacing"), Some(&"8px".to_string()));
        assert_eq!(s.get("color"), Some(&"#eee".to_string()));
        assert_eq!(s.get("bg"), Some(&"#111".to_string()));
        assert_eq!(s.get("align"), Some(&"center".to_string()));
        assert_eq!(s.get("bold"), Some(&"true".to_string()));
        assert_eq!(s.get("italic"), Some(&"true".to_string()));
    }

    #[test]
    fn normalize_events_for_button_input_toggle() {
        assert_eq!(
            normalize_event("button", "on_about", json!(true)),
            json!({"type":"action","action":"on_about","value":true})
        );
        assert_eq!(
            normalize_event("input", "on_name", json!("ada")),
            json!({"type":"input","action":"on_name","value":"ada"})
        );
        assert_eq!(
            normalize_event("toggle", "on_enabled", json!(true)),
            json!({"type":"toggle","action":"on_enabled","value":true})
        );
    }

    #[test]
    fn diff_reorder_same_type_children_produces_updates() {
        let a = parse_node(&json!({
            "type":"hstack","props":{"spacing":6},"children":[
                {"type":"text","props":{"value":"A"},"children":[]},
                {"type":"text","props":{"value":"B"},"children":[]}
            ]
        }))
        .unwrap();
        let b = parse_node(&json!({
            "type":"hstack","props":{"spacing":6},"children":[
                {"type":"text","props":{"value":"B"},"children":[]},
                {"type":"text","props":{"value":"A"},"children":[]}
            ]
        }))
        .unwrap();
        let ops = diff_nodes(&a, &b);
        let update_text_count = ops
            .iter()
            .filter(|op| **op == GtkPatchOp::UpdateText)
            .count();
        assert_eq!(update_text_count, 2);
    }

    #[test]
    fn parity_counter_snapshot_matches_ssr_browser_conventions() {
        let tree = json!({
            "type":"screen",
            "props":{},
            "children":[{
                "type":"vstack",
                "props":{"spacing":20},
                "children":[
                    {"type":"text","props":{"value":"Current Count: 42","size":24},"children":[]},
                    {"type":"hstack","props":{"spacing":10},"children":[
                        {"type":"button","props":{"label":"+"},"children":[]},
                        {"type":"button","props":{"label":"-"},"children":[]}
                    ]}
                ]
            }]
        });

        let gtk = parse_node(&tree).unwrap();
        assert_eq!(gtk.node_type, "screen");
        assert_eq!(gtk.children[0].node_type, "vstack");
        assert_eq!(
            gtk.children[0].style.values.get("spacing"),
            Some(&"20px".to_string())
        );
        assert_eq!(
            gtk.children[0].children[0].text.as_deref(),
            Some("Current Count: 42")
        );

        let html = ui_html::render_html(&tree);
        assert!(html.contains("class=\"forai-screen\""));
        assert!(html.contains("flex-direction:column"));
        assert!(html.contains("gap:20px"));
        assert!(html.contains("Current Count: 42"));
    }
}
