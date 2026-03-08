use serde_json::Value;

/// Escape HTML special characters in text content.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn css_len(value: &Value) -> Option<String> {
    if let Some(v) = value.as_u64() {
        return Some(format!("{v}px"));
    }
    if let Some(v) = value.as_i64() {
        return Some(format!("{v}px"));
    }
    if let Some(v) = value.as_f64() {
        if v.fract() == 0.0 {
            return Some(format!("{}px", v as i64));
        }
        return Some(format!("{v}px"));
    }
    value.as_str().map(|s| s.to_string())
}

fn align_to_css(value: &str) -> Option<&'static str> {
    match value {
        "start" => Some("flex-start"),
        "center" => Some("center"),
        "end" => Some("flex-end"),
        _ => None,
    }
}

fn append_common_styles(props: Option<&serde_json::Map<String, Value>>, styles: &mut Vec<String>) {
    let Some(props) = props else {
        return;
    };

    if let Some(v) = props.get("padding").and_then(css_len) {
        styles.push(format!("padding:{v}"));
    }
    if let Some(v) = props.get("margin").and_then(css_len) {
        styles.push(format!("margin:{v}"));
    }
    if let Some(v) = props.get("width").and_then(css_len) {
        styles.push(format!("width:{v}"));
    }
    if let Some(v) = props.get("height").and_then(css_len) {
        styles.push(format!("height:{v}"));
    }
    if let Some(v) = props.get("color").and_then(|v| v.as_str()) {
        styles.push(format!("color:{v}"));
    }
    if let Some(v) = props.get("bg").and_then(|v| v.as_str()) {
        styles.push(format!("background:{v}"));
    }
    if let Some(v) = props.get("border") {
        if let Some(enabled) = v.as_bool() {
            if enabled {
                styles.push("border:1px solid currentColor".to_string());
            }
        } else if let Some(border_str) = v.as_str() {
            styles.push(format!("border:{border_str}"));
        }
    }
}

fn append_text_styles(props: Option<&serde_json::Map<String, Value>>, styles: &mut Vec<String>) {
    let Some(props) = props else {
        return;
    };
    if let Some(v) = props.get("size").and_then(css_len) {
        styles.push(format!("font-size:{v}"));
    }
    if props.get("bold").and_then(|v| v.as_bool()).unwrap_or(false) {
        styles.push("font-weight:bold".to_string());
    }
    if props.get("italic").and_then(|v| v.as_bool()).unwrap_or(false) {
        styles.push("font-style:italic".to_string());
    }
}

fn append_custom_styles(props: Option<&serde_json::Map<String, Value>>, styles: &mut Vec<String>) {
    let Some(props) = props else {
        return;
    };

    for (key, value) in props {
        let is_known = matches!(
            key.as_str(),
            "value"
                | "label"
                | "placeholder"
                | "spacing"
                | "padding"
                | "margin"
                | "align"
                | "width"
                | "height"
                | "color"
                | "bg"
                | "border"
                | "size"
                | "bold"
                | "italic"
        );
        if is_known {
            continue;
        }

        if let Some(v) = value.as_str() {
            styles.push(format!("{key}:{v}"));
            continue;
        }
        if let Some(v) = value.as_u64() {
            styles.push(format!("{key}:{v}"));
            continue;
        }
        if let Some(v) = value.as_i64() {
            styles.push(format!("{key}:{v}"));
            continue;
        }
        if let Some(v) = value.as_f64() {
            styles.push(format!("{key}:{v}"));
        }
    }
}

fn style_attr(styles: &[String]) -> String {
    if styles.is_empty() {
        String::new()
    } else {
        format!(" style=\"{}\"", styles.join(";"))
    }
}

/// Render a UiNode tree to an HTML string.
///
/// Each node type maps to semantic HTML with inline CSS:
/// - `screen`  → `<div class="forai-screen">` with reset styles
/// - `vstack`  → `<div>` with `flex-direction:column`
/// - `hstack`  → `<div>` with `flex-direction:row`
/// - `text`    → `<span>` with optional font-size
/// - `button`  → `<button>`
/// - `input`   → `<input type="text">`
/// - `toggle`  → `<input type="checkbox">`
pub fn render_html(node: &Value) -> String {
    let Some(obj) = node.as_object() else {
        return String::new();
    };
    let node_type = obj
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let props = obj.get("props").and_then(|v| v.as_object());
    let children = obj
        .get("children")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    match node_type {
        "screen" => {
            let inner = render_children(children);
            let mut styles = vec![
                "font-family:system-ui,sans-serif".to_string(),
                "margin:0".to_string(),
                "padding:0".to_string(),
            ];
            append_common_styles(props, &mut styles);
            append_custom_styles(props, &mut styles);
            format!("<div class=\"forai-screen\"{}>{inner}</div>", style_attr(&styles))
        }
        "vstack" => {
            let spacing = props
                .and_then(|p| p.get("spacing"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let inner = render_children(children);
            let mut styles = vec![
                "display:flex".to_string(),
                "flex-direction:column".to_string(),
                format!("gap:{spacing}px"),
            ];
            if let Some(align) = props
                .and_then(|p| p.get("align"))
                .and_then(|v| v.as_str())
                .and_then(align_to_css)
            {
                styles.push(format!("align-items:{align}"));
            }
            append_common_styles(props, &mut styles);
            append_custom_styles(props, &mut styles);
            format!("<div{}>{inner}</div>", style_attr(&styles))
        }
        "hstack" => {
            let spacing = props
                .and_then(|p| p.get("spacing"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let inner = render_children(children);
            let mut styles = vec![
                "display:flex".to_string(),
                "flex-direction:row".to_string(),
                format!("gap:{spacing}px"),
            ];
            if let Some(align) = props
                .and_then(|p| p.get("align"))
                .and_then(|v| v.as_str())
                .and_then(align_to_css)
            {
                styles.push(format!("align-items:{align}"));
            }
            append_common_styles(props, &mut styles);
            append_custom_styles(props, &mut styles);
            format!("<div{}>{inner}</div>", style_attr(&styles))
        }
        "zstack" => {
            let inner = render_children(children);
            let mut styles = vec!["position:relative".to_string()];
            append_common_styles(props, &mut styles);
            append_custom_styles(props, &mut styles);
            format!("<div{}>{inner}</div>", style_attr(&styles))
        }
        "text" => {
            let value = props
                .and_then(|p| p.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let escaped = escape_html(value);
            let mut styles = Vec::new();
            append_common_styles(props, &mut styles);
            append_text_styles(props, &mut styles);
            append_custom_styles(props, &mut styles);
            format!("<span{}>{escaped}</span>", style_attr(&styles))
        }
        "button" => {
            let label = props
                .and_then(|p| p.get("label"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let escaped = escape_html(label);
            let mut styles = Vec::new();
            append_common_styles(props, &mut styles);
            append_text_styles(props, &mut styles);
            append_custom_styles(props, &mut styles);
            format!("<button{}>{escaped}</button>", style_attr(&styles))
        }
        "input" => {
            let placeholder = props
                .and_then(|p| p.get("placeholder"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let value = props
                .and_then(|p| p.get("value"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let ph_escaped = escape_html(placeholder);
            let val_escaped = escape_html(value);
            let mut styles = Vec::new();
            append_common_styles(props, &mut styles);
            append_text_styles(props, &mut styles);
            append_custom_styles(props, &mut styles);
            format!(
                "<input type=\"text\" placeholder=\"{ph_escaped}\" value=\"{val_escaped}\"{}>",
                style_attr(&styles)
            )
        }
        "toggle" => {
            let checked = props
                .and_then(|p| p.get("value"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut styles = Vec::new();
            append_common_styles(props, &mut styles);
            append_custom_styles(props, &mut styles);
            if checked {
                format!("<input type=\"checkbox\" checked{}>", style_attr(&styles))
            } else {
                format!("<input type=\"checkbox\"{}>", style_attr(&styles))
            }
        }
        _ => {
            // Unknown node type — render children only
            render_children(children)
        }
    }
}

fn render_children(children: &[Value]) -> String {
    children.iter().map(|c| render_html(c)).collect::<String>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_node_renders_span() {
        let node = json!({"type": "text", "props": {"value": "Hello"}, "children": []});
        assert_eq!(render_html(&node), "<span>Hello</span>");
    }

    #[test]
    fn text_node_with_size() {
        let node = json!({"type": "text", "props": {"value": "Big", "size": 24}, "children": []});
        assert_eq!(
            render_html(&node),
            "<span style=\"font-size:24px\">Big</span>"
        );
    }

    #[test]
    fn html_entities_escaped() {
        let node = json!({"type": "text", "props": {"value": "<script>alert(1)</script>"}, "children": []});
        let html = render_html(&node);
        assert_eq!(
            html,
            "<span>&lt;script&gt;alert(1)&lt;/script&gt;</span>"
        );
    }

    #[test]
    fn vstack_with_children() {
        let node = json!({
            "type": "vstack",
            "props": {"spacing": 10},
            "children": [
                {"type": "text", "props": {"value": "A"}, "children": []},
                {"type": "text", "props": {"value": "B"}, "children": []}
            ]
        });
        assert_eq!(
            render_html(&node),
            "<div style=\"display:flex;flex-direction:column;gap:10px\"><span>A</span><span>B</span></div>"
        );
    }

    #[test]
    fn hstack_with_children() {
        let node = json!({
            "type": "hstack",
            "props": {"spacing": 5},
            "children": [
                {"type": "button", "props": {"label": "+"}, "children": []},
                {"type": "button", "props": {"label": "-"}, "children": []}
            ]
        });
        assert_eq!(
            render_html(&node),
            "<div style=\"display:flex;flex-direction:row;gap:5px\"><button>+</button><button>-</button></div>"
        );
    }

    #[test]
    fn screen_wrapper() {
        let node = json!({
            "type": "screen",
            "props": {},
            "children": [
                {"type": "text", "props": {"value": "Hi"}, "children": []}
            ]
        });
        let html = render_html(&node);
        assert!(html.starts_with("<div class=\"forai-screen\""));
        assert!(html.contains("<span>Hi</span>"));
    }

    #[test]
    fn button_renders() {
        let node = json!({"type": "button", "props": {"label": "Click"}, "children": []});
        assert_eq!(render_html(&node), "<button>Click</button>");
    }

    #[test]
    fn input_renders() {
        let node = json!({"type": "input", "props": {"placeholder": "Name", "value": "Jo"}, "children": []});
        assert_eq!(
            render_html(&node),
            "<input type=\"text\" placeholder=\"Name\" value=\"Jo\">"
        );
    }

    #[test]
    fn toggle_checked() {
        let node = json!({"type": "toggle", "props": {"value": true}, "children": []});
        assert_eq!(render_html(&node), "<input type=\"checkbox\" checked>");
    }

    #[test]
    fn toggle_unchecked() {
        let node = json!({"type": "toggle", "props": {"value": false}, "children": []});
        assert_eq!(render_html(&node), "<input type=\"checkbox\">");
    }

    #[test]
    fn counter_view_integration() {
        // Simulate the counter-ui CounterView output
        let tree = json!({
            "type": "screen",
            "props": {},
            "children": [{
                "type": "vstack",
                "props": {"spacing": 20},
                "children": [
                    {"type": "text", "props": {"value": "Current Count: 42", "size": 24}, "children": []},
                    {
                        "type": "hstack",
                        "props": {"spacing": 10},
                        "children": [
                            {"type": "button", "props": {"label": "+"}, "children": []},
                            {"type": "button", "props": {"label": "-"}, "children": []}
                        ]
                    }
                ]
            }]
        });
        let html = render_html(&tree);
        // Correct structure
        assert!(html.contains("forai-screen"));
        assert!(html.contains("flex-direction:column"));
        assert!(html.contains("gap:20px"));
        assert!(html.contains("<span style=\"font-size:24px\">Current Count: 42</span>"));
        assert!(html.contains("<button>+</button>"));
        assert!(html.contains("<button>-</button>"));
        assert!(html.contains("gap:10px"));
    }

    #[test]
    fn zstack_renders() {
        let node = json!({
            "type": "zstack",
            "props": {},
            "children": [
                {"type": "text", "props": {"value": "Bottom"}, "children": []},
                {"type": "text", "props": {"value": "Top"}, "children": []}
            ]
        });
        assert_eq!(
            render_html(&node),
            "<div style=\"position:relative\"><span>Bottom</span><span>Top</span></div>"
        );
    }

    #[test]
    fn styled_vstack_css_mapping() {
        let node = json!({
            "type": "vstack",
            "props": {
                "spacing": 8,
                "padding": 12,
                "margin": 4,
                "align": "center",
                "width": 480,
                "height": 200,
                "bg": "#333",
                "border": true
            },
            "children": [{"type":"text","props":{"value":"x"},"children":[]}]
        });
        let html = render_html(&node);
        assert!(html.contains("display:flex"));
        assert!(html.contains("flex-direction:column"));
        assert!(html.contains("gap:8px"));
        assert!(html.contains("padding:12px"));
        assert!(html.contains("margin:4px"));
        assert!(html.contains("align-items:center"));
        assert!(html.contains("width:480px"));
        assert!(html.contains("height:200px"));
        assert!(html.contains("background:#333"));
        assert!(html.contains("border:1px solid currentColor"));
    }

    #[test]
    fn styled_text_css_mapping() {
        let node = json!({
            "type": "text",
            "props": {
                "value": "Hello",
                "size": 20,
                "color": "#eee",
                "bg": "#111",
                "bold": true,
                "italic": true
            },
            "children": []
        });
        let html = render_html(&node);
        assert!(html.contains("font-size:20px"));
        assert!(html.contains("color:#eee"));
        assert!(html.contains("background:#111"));
        assert!(html.contains("font-weight:bold"));
        assert!(html.contains("font-style:italic"));
    }

    #[test]
    fn button_label_escaped() {
        let node = json!({"type": "button", "props": {"label": "<b>Bold</b>"}, "children": []});
        assert_eq!(
            render_html(&node),
            "<button>&lt;b&gt;Bold&lt;/b&gt;</button>"
        );
    }

    #[test]
    fn custom_style_props_are_rendered() {
        let node = json!({
            "type": "text",
            "props": {
                "value": "Hello",
                "text-decoration": "underline",
                "line-height": 1.4
            },
            "children": []
        });
        let html = render_html(&node);
        assert!(html.contains("text-decoration:underline"));
        assert!(html.contains("line-height:1.4"));
    }
}
