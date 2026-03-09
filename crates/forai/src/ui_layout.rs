use serde_json::Value;

// ---------------------------------------------------------------------------
// Layout types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedNode {
    pub rect: Rect,
    pub node_type: String,
    pub props: serde_json::Map<String, Value>,
    pub text: Option<String>,
}

// ---------------------------------------------------------------------------
// Layout engine — recursive, pure (no I/O)
// ---------------------------------------------------------------------------

/// Recursively lay out a UiNode tree into positioned nodes.
pub fn layout_node(node: &Value, available: Rect) -> Vec<PositionedNode> {
    let obj = match node.as_object() {
        Some(o) => o,
        None => return vec![],
    };
    let node_type = match obj.get("type").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return vec![],
    };
    let props = obj
        .get("props")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let children = obj
        .get("children")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    match node_type {
        "screen" => {
            let mut result = vec![PositionedNode {
                rect: available,
                node_type: "screen".into(),
                props: props.clone(),
                text: None,
            }];
            for child in &children {
                result.extend(layout_node(child, available));
            }
            result
        }

        "vstack" => {
            let spacing = props.get("spacing").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            let align = props.get("align").and_then(|v| v.as_str()).unwrap_or("");
            let n = children.len() as u16;
            if n == 0 {
                return vec![];
            }

            if align == "start" {
                // Pack children at the top, each getting height=1
                let mut result = Vec::new();
                let mut y = available.y;
                for child in &children {
                    if y >= available.y + available.height {
                        break;
                    }
                    let child_rect = Rect {
                        x: available.x,
                        y,
                        width: available.width,
                        height: 1,
                    };
                    result.extend(layout_node(child, child_rect));
                    y = y.saturating_add(1).saturating_add(spacing);
                }
                return result;
            }

            let total_spacing = spacing.saturating_mul(n.saturating_sub(1));
            let usable = available.height.saturating_sub(total_spacing);

            // Collect fixed heights from children's props
            let mut fixed_total: u16 = 0;
            let mut flex_count: u16 = 0;
            let child_heights: Vec<Option<u16>> = children
                .iter()
                .map(|c| {
                    let h = c
                        .as_object()
                        .and_then(|o| o.get("props"))
                        .and_then(|p| p.get("height"))
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u16);
                    if let Some(h) = h {
                        fixed_total = fixed_total.saturating_add(h);
                    } else {
                        flex_count += 1;
                    }
                    h
                })
                .collect();
            let flex_h = if flex_count > 0 {
                usable.saturating_sub(fixed_total) / flex_count
            } else {
                0
            };

            let mut result = Vec::new();
            let mut y = available.y;
            for (i, child) in children.iter().enumerate() {
                let child_h = child_heights[i].unwrap_or(flex_h);
                let child_rect = Rect {
                    x: available.x,
                    y,
                    width: available.width,
                    height: child_h,
                };
                result.extend(layout_node(child, child_rect));
                y = y.saturating_add(child_h).saturating_add(spacing);
            }
            result
        }

        "hstack" => {
            let spacing = props.get("spacing").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            let align = props.get("align").and_then(|v| v.as_str()).unwrap_or("");
            let n = children.len() as u16;
            if n == 0 {
                return vec![];
            }

            if align == "start" {
                // Pack children left, each getting its natural width
                let mut result = Vec::new();
                let mut x = available.x;
                for child in &children {
                    if x >= available.x + available.width {
                        break;
                    }
                    let remaining_w = (available.x + available.width).saturating_sub(x);
                    let child_rect = Rect {
                        x,
                        y: available.y,
                        width: remaining_w,
                        height: available.height,
                    };
                    let child_nodes = layout_node(child, child_rect);
                    // Measure the max width actually used by child nodes
                    let used_w = child_nodes.iter().map(|n| n.rect.width).max().unwrap_or(1);
                    result.extend(child_nodes);
                    x = x.saturating_add(used_w).saturating_add(spacing);
                }
                return result;
            }

            let total_spacing = spacing.saturating_mul(n.saturating_sub(1));
            let usable = available.width.saturating_sub(total_spacing);

            // Collect fixed widths from children's props
            let mut fixed_total: u16 = 0;
            let mut flex_count: u16 = 0;
            let child_widths: Vec<Option<u16>> = children
                .iter()
                .map(|c| {
                    let w = c
                        .as_object()
                        .and_then(|o| o.get("props"))
                        .and_then(|p| p.get("width"))
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u16);
                    if let Some(w) = w {
                        fixed_total = fixed_total.saturating_add(w);
                    } else {
                        flex_count += 1;
                    }
                    w
                })
                .collect();
            let flex_w = if flex_count > 0 {
                usable.saturating_sub(fixed_total) / flex_count
            } else {
                0
            };

            let mut result = Vec::new();
            let mut x = available.x;
            for (i, child) in children.iter().enumerate() {
                let child_w = child_widths[i].unwrap_or(flex_w);
                let child_rect = Rect {
                    x,
                    y: available.y,
                    width: child_w,
                    height: available.height,
                };
                result.extend(layout_node(child, child_rect));
                x = x.saturating_add(child_w).saturating_add(spacing);
            }
            result
        }

        "text" => {
            let value = props.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let width = available.width as usize;
            let height = if width == 0 {
                1
            } else {
                let len = value.len();
                std::cmp::max(1, (len + width - 1) / width) as u16
            };
            vec![PositionedNode {
                rect: Rect {
                    x: available.x,
                    y: available.y,
                    width: available.width,
                    height,
                },
                node_type: "text".into(),
                props: props.clone(),
                text: Some(value.to_string()),
            }]
        }

        "html" => {
            let raw = props.get("html").and_then(|v| v.as_str()).unwrap_or("");
            // Strip HTML tags for terminal layout sizing
            let plain = regex::Regex::new(r"<[^>]+>")
                .unwrap()
                .replace_all(raw, "")
                .to_string();
            let width = available.width as usize;
            let height = if width == 0 {
                1
            } else {
                std::cmp::max(1, (plain.len() + width - 1) / width) as u16
            };
            vec![PositionedNode {
                rect: Rect {
                    x: available.x,
                    y: available.y,
                    width: available.width,
                    height,
                },
                node_type: "html".into(),
                props: props.clone(),
                text: Some(plain),
            }]
        }

        "button" => {
            let label = props.get("label").and_then(|v| v.as_str()).unwrap_or("");
            let width = (label.len() as u16).saturating_add(4);
            vec![PositionedNode {
                rect: Rect {
                    x: available.x,
                    y: available.y,
                    width,
                    height: 1,
                },
                node_type: "button".into(),
                props: props.clone(),
                text: Some(format!("[ {label} ]")),
            }]
        }

        "input" => {
            let placeholder = props
                .get("placeholder")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            vec![PositionedNode {
                rect: Rect {
                    x: available.x,
                    y: available.y,
                    width: available.width,
                    height: 1,
                },
                node_type: "input".into(),
                props: props.clone(),
                text: Some(placeholder),
            }]
        }

        "toggle" => {
            let on = props.get("on").and_then(|v| v.as_bool()).unwrap_or(false);
            let text = if on { "[x]" } else { "[ ]" };
            vec![PositionedNode {
                rect: Rect {
                    x: available.x,
                    y: available.y,
                    width: 5,
                    height: 1,
                },
                node_type: "toggle".into(),
                props: props.clone(),
                text: Some(text.to_string()),
            }]
        }

        "zstack" => {
            let mut result = Vec::new();
            for child in &children {
                result.extend(layout_node(child, available));
            }
            result
        }

        _ => vec![], // unknown types are skipped
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_text(value: &str) -> Value {
        json!({
            "type": "text",
            "props": { "value": value },
            "children": [],
            "events": {}
        })
    }

    fn make_button(label: &str) -> Value {
        json!({
            "type": "button",
            "props": { "label": label },
            "children": [],
            "events": {}
        })
    }

    fn rect(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn vstack_3_text_children() {
        let tree = json!({
            "type": "vstack",
            "props": {},
            "children": [
                make_text("Hello"),
                make_text("World"),
                make_text("!")
            ],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        assert_eq!(nodes.len(), 3);
        // 24 / 3 = 8 each
        assert_eq!(nodes[0].rect.y, 0);
        assert_eq!(nodes[1].rect.y, 8);
        assert_eq!(nodes[2].rect.y, 16);
        for n in &nodes {
            assert_eq!(n.rect.width, 80);
        }
    }

    #[test]
    fn hstack_3_text_children() {
        let tree = json!({
            "type": "hstack",
            "props": {},
            "children": [
                make_text("A"),
                make_text("B"),
                make_text("C")
            ],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        assert_eq!(nodes.len(), 3);
        // 80 / 3 = 26 each
        assert_eq!(nodes[0].rect.x, 0);
        assert_eq!(nodes[1].rect.x, 26);
        assert_eq!(nodes[2].rect.x, 52);
        // text nodes measure their content height (1 line each)
        for n in &nodes {
            assert_eq!(n.rect.height, 1);
        }
    }

    #[test]
    fn text_wrapping() {
        // 10 chars in width=5 => 2 lines
        let tree = make_text("HelloWorld");
        let nodes = layout_node(&tree, rect(0, 0, 5, 10));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].rect.height, 2);
    }

    #[test]
    fn button_sizing() {
        let tree = make_button("OK");
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].rect.width, 6); // "[ OK ]" = 6
        assert_eq!(nodes[0].rect.height, 1);
        assert_eq!(nodes[0].text.as_deref(), Some("[ OK ]"));
    }

    #[test]
    fn nested_screen_vstack_hstack() {
        let tree = json!({
            "type": "screen",
            "props": {},
            "children": [{
                "type": "vstack",
                "props": {},
                "children": [
                    make_text("Title"),
                    {
                        "type": "hstack",
                        "props": {},
                        "children": [
                            make_button("Yes"),
                            make_button("No")
                        ],
                        "events": {}
                    }
                ],
                "events": {}
            }],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        // screen + text("Title") + button("Yes") + button("No") = 4
        assert_eq!(nodes.len(), 4);
        assert_eq!(nodes[0].node_type, "screen");
        assert_eq!(nodes[1].node_type, "text");
        assert_eq!(nodes[2].node_type, "button");
        assert_eq!(nodes[3].node_type, "button");
        // The hstack children should have different x positions
        assert!(nodes[2].rect.x < nodes[3].rect.x);
    }

    #[test]
    fn empty_children_no_nodes() {
        let tree = json!({
            "type": "vstack",
            "props": {},
            "children": [],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        assert_eq!(nodes.len(), 0);
    }

    #[test]
    fn vstack_with_spacing() {
        let tree = json!({
            "type": "vstack",
            "props": { "spacing": 1 },
            "children": [
                make_text("A"),
                make_text("B"),
                make_text("C")
            ],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        // 24 - 2 spacing = 22, 22/3 = 7 each
        assert_eq!(nodes[0].rect.y, 0);
        assert_eq!(nodes[1].rect.y, 8); // 7 + 1
        assert_eq!(nodes[2].rect.y, 16); // 7+1+7+1
    }

    #[test]
    fn toggle_on_off() {
        let on = json!({
            "type": "toggle",
            "props": { "on": true },
            "children": [],
            "events": {}
        });
        let off = json!({
            "type": "toggle",
            "props": { "on": false },
            "children": [],
            "events": {}
        });
        let on_nodes = layout_node(&on, rect(0, 0, 80, 24));
        let off_nodes = layout_node(&off, rect(0, 0, 80, 24));
        assert_eq!(on_nodes[0].text.as_deref(), Some("[x]"));
        assert_eq!(off_nodes[0].text.as_deref(), Some("[ ]"));
        assert_eq!(on_nodes[0].rect.width, 5);
    }

    #[test]
    fn input_sizing() {
        let tree = json!({
            "type": "input",
            "props": { "placeholder": "Type here..." },
            "children": [],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(5, 10, 40, 3));
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].rect.width, 40);
        assert_eq!(nodes[0].rect.height, 1);
        assert_eq!(nodes[0].text.as_deref(), Some("Type here..."));
    }

    #[test]
    fn zstack_overlapping() {
        let tree = json!({
            "type": "zstack",
            "props": {},
            "children": [
                make_text("Background"),
                make_text("Foreground")
            ],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        assert_eq!(nodes.len(), 2);
        // Both should have the same rect
        assert_eq!(nodes[0].rect, nodes[1].rect);
    }

    #[test]
    fn hstack_fixed_width_child() {
        // child1 has width=30, child2 gets remainder (80-30=50)
        let tree = json!({
            "type": "hstack",
            "props": {},
            "children": [
                {
                    "type": "text",
                    "props": { "value": "Left", "width": 30 },
                    "children": [],
                    "events": {}
                },
                make_text("Right")
            ],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].rect.x, 0);
        assert_eq!(nodes[0].rect.width, 30);
        assert_eq!(nodes[1].rect.x, 30);
        assert_eq!(nodes[1].rect.width, 50);
    }

    #[test]
    fn vstack_fixed_height_child() {
        // child1 has height=1, child2 (a vstack container) gets remainder (24-1=23)
        let tree = json!({
            "type": "vstack",
            "props": {},
            "children": [
                {
                    "type": "text",
                    "props": { "value": "Header", "height": 1 },
                    "children": [],
                    "events": {}
                },
                {
                    "type": "vstack",
                    "props": {},
                    "children": [make_text("Body")],
                    "events": {}
                }
            ],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        // text("Header") + text("Body") = 2 nodes
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].rect.y, 0);
        assert_eq!(nodes[0].rect.height, 1);
        // The inner vstack child gets y=1 and the text inside gets height from the 23-high allocation
        assert_eq!(nodes[1].rect.y, 1);
    }

    #[test]
    fn vstack_align_start() {
        // With align="start", children get height=1 and stack tightly at top
        let tree = json!({
            "type": "vstack",
            "props": { "align": "start" },
            "children": [
                make_text("Line1"),
                make_text("Line2"),
                make_text("Line3")
            ],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].rect.y, 0);
        assert_eq!(nodes[0].rect.height, 1);
        assert_eq!(nodes[1].rect.y, 1);
        assert_eq!(nodes[1].rect.height, 1);
        assert_eq!(nodes[2].rect.y, 2);
        assert_eq!(nodes[2].rect.height, 1);
    }

    #[test]
    fn hstack_align_start() {
        // With align="start", children get natural width and pack left
        let tree = json!({
            "type": "hstack",
            "props": { "align": "start" },
            "children": [
                make_button("OK"),
                make_button("Cancel")
            ],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        assert_eq!(nodes.len(), 2);
        // button "OK" → width 6 ("[ OK ]"), button "Cancel" → width 10 ("[ Cancel ]")
        assert_eq!(nodes[0].rect.x, 0);
        assert_eq!(nodes[0].rect.width, 6);
        assert_eq!(nodes[1].rect.x, 6);
        assert_eq!(nodes[1].rect.width, 10);
    }

    #[test]
    fn unknown_type_skipped() {
        let tree = json!({
            "type": "foobar",
            "props": {},
            "children": [],
            "events": {}
        });
        let nodes = layout_node(&tree, rect(0, 0, 80, 24));
        assert_eq!(nodes.len(), 0);
    }
}
