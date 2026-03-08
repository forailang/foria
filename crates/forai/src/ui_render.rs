// ui_render.rs — Terminal render pass (crossterm)
// Converts PositionedNodes into draw commands and executes them.

use crate::ui_layout::{PositionedNode, Rect, layout_node};
use crossterm::style::{Attribute, Color};
use serde_json::Value;
use std::io::Write;

// ---------------------------------------------------------------------------
// Draw commands — intermediate representation between layout and terminal
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum DrawCommand {
    MoveTo(u16, u16),
    Print(String),
    SetColor(Color),
    SetBgColor(Color),
    SetAttribute(Attribute),
    ResetColor,
    Clear,
}

fn color_from_name(name: &str) -> Option<Color> {
    match name {
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "blue" => Some(Color::Blue),
        "yellow" => Some(Color::Yellow),
        "cyan" => Some(Color::Cyan),
        "magenta" => Some(Color::Magenta),
        "white" => Some(Color::White),
        "black" => Some(Color::Black),
        "grey" | "gray" => Some(Color::Grey),
        "dark_red" => Some(Color::DarkRed),
        "dark_green" => Some(Color::DarkGreen),
        "dark_blue" => Some(Color::DarkBlue),
        "dark_yellow" => Some(Color::DarkYellow),
        "dark_cyan" => Some(Color::DarkCyan),
        "dark_magenta" => Some(Color::DarkMagenta),
        "dark_grey" | "dark_gray" => Some(Color::DarkGrey),
        _ => None,
    }
}

/// Emit style commands for a node's props (color, bg, bold, italic, reverse).
/// Returns true if any style was emitted (so caller knows to reset).
fn emit_style_commands(props: &serde_json::Map<String, Value>, cmds: &mut Vec<DrawCommand>) -> bool {
    let mut styled = false;
    if let Some(color) = props.get("color").and_then(|v| v.as_str()).and_then(color_from_name) {
        cmds.push(DrawCommand::SetColor(color));
        styled = true;
    }
    if let Some(bg) = props.get("bg").and_then(|v| v.as_str()).and_then(color_from_name) {
        cmds.push(DrawCommand::SetBgColor(bg));
        styled = true;
    }
    if props.get("bold").and_then(|v| v.as_bool()).unwrap_or(false) {
        cmds.push(DrawCommand::SetAttribute(Attribute::Bold));
        styled = true;
    }
    if props.get("italic").and_then(|v| v.as_bool()).unwrap_or(false) {
        cmds.push(DrawCommand::SetAttribute(Attribute::Italic));
        styled = true;
    }
    if props.get("reverse").and_then(|v| v.as_bool()).unwrap_or(false) {
        cmds.push(DrawCommand::SetAttribute(Attribute::Reverse));
        styled = true;
    }
    styled
}

fn emit_style_reset(cmds: &mut Vec<DrawCommand>) {
    cmds.push(DrawCommand::ResetColor);
    cmds.push(DrawCommand::SetAttribute(Attribute::Reset));
}

// ---------------------------------------------------------------------------
// Render pass: PositionedNode → DrawCommand
// ---------------------------------------------------------------------------

pub fn render_to_commands(nodes: &[PositionedNode]) -> Vec<DrawCommand> {
    let mut cmds = Vec::new();

    for node in nodes {
        match node.node_type.as_str() {
            "screen" => {
                cmds.push(DrawCommand::Clear);
            }
            "text" => {
                if let Some(ref text) = node.text {
                    // Wrap text at available width
                    let width = node.rect.width as usize;
                    if width == 0 {
                        continue;
                    }
                    let styled = emit_style_commands(&node.props, &mut cmds);
                    let mut y = node.rect.y;
                    let mut remaining = text.as_str();
                    while !remaining.is_empty() {
                        let chunk_len = std::cmp::min(remaining.len(), width);
                        let chunk = &remaining[..chunk_len];
                        cmds.push(DrawCommand::MoveTo(node.rect.x, y));
                        cmds.push(DrawCommand::Print(chunk.to_string()));
                        remaining = &remaining[chunk_len..];
                        y += 1;
                    }
                    if styled {
                        emit_style_reset(&mut cmds);
                    }
                }
            }
            "button" => {
                if let Some(ref text) = node.text {
                    let styled = emit_style_commands(&node.props, &mut cmds);
                    cmds.push(DrawCommand::MoveTo(node.rect.x, node.rect.y));
                    cmds.push(DrawCommand::Print(text.clone()));
                    if styled {
                        emit_style_reset(&mut cmds);
                    }
                }
            }
            "input" => {
                cmds.push(DrawCommand::MoveTo(node.rect.x, node.rect.y));
                let placeholder = node.text.as_deref().unwrap_or("");
                // Draw input field: placeholder text padded to width
                let width = node.rect.width as usize;
                let display = if placeholder.len() > width {
                    &placeholder[..width]
                } else {
                    placeholder
                };
                let padded = format!("{display:<width$}", width = width);
                cmds.push(DrawCommand::SetColor(Color::DarkGrey));
                cmds.push(DrawCommand::Print(padded));
                cmds.push(DrawCommand::ResetColor);
            }
            "toggle" => {
                if let Some(ref text) = node.text {
                    cmds.push(DrawCommand::MoveTo(node.rect.x, node.rect.y));
                    cmds.push(DrawCommand::Print(text.clone()));
                }
            }
            _ => {} // screen and unknown types produce no direct output
        }
    }

    cmds
}

// ---------------------------------------------------------------------------
// Execute draw commands to a writer via crossterm
// ---------------------------------------------------------------------------

pub fn execute_commands(commands: &[DrawCommand], stdout: &mut impl Write) {
    use crossterm::{
        cursor, execute,
        style::{Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor},
        terminal::{Clear, ClearType},
    };

    for cmd in commands {
        match cmd {
            DrawCommand::MoveTo(x, y) => {
                let _ = execute!(stdout, cursor::MoveTo(*x, *y));
            }
            DrawCommand::Print(text) => {
                let _ = execute!(stdout, Print(text));
            }
            DrawCommand::SetColor(color) => {
                let _ = execute!(stdout, SetForegroundColor(*color));
            }
            DrawCommand::SetBgColor(color) => {
                let _ = execute!(stdout, SetBackgroundColor(*color));
            }
            DrawCommand::SetAttribute(attr) => {
                let _ = execute!(stdout, SetAttribute(*attr));
            }
            DrawCommand::ResetColor => {
                let _ = execute!(stdout, ResetColor);
            }
            DrawCommand::Clear => {
                let _ = execute!(stdout, Clear(ClearType::All), cursor::MoveTo(0, 0));
            }
        }
    }
    let _ = stdout.flush();
}

// ---------------------------------------------------------------------------
// Full pipeline: UiNode tree → terminal output
// ---------------------------------------------------------------------------

pub fn render_ui_tree(tree: &Value, stdout: &mut impl Write) {
    let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
    let available = Rect {
        x: 0,
        y: 0,
        width,
        height,
    };
    let positioned = layout_node(tree, available);
    let mut commands = render_to_commands(&positioned);

    // Always clear screen before rendering to prevent ghost characters
    // from previous frames when text changes length
    if !commands.iter().any(|c| matches!(c, DrawCommand::Clear)) {
        commands.insert(0, DrawCommand::Clear);
    }

    execute_commands(&commands, stdout);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_layout::{PositionedNode, Rect};
    fn make_positioned(
        node_type: &str,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        text: Option<&str>,
    ) -> PositionedNode {
        PositionedNode {
            rect: Rect {
                x,
                y,
                width: w,
                height: h,
            },
            node_type: node_type.into(),
            props: serde_json::Map::new(),
            text: text.map(|s| s.to_string()),
        }
    }

    #[test]
    fn text_node_at_position() {
        let nodes = vec![make_positioned("text", 5, 3, 80, 1, Some("hello"))];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds, vec![
            DrawCommand::MoveTo(5, 3),
            DrawCommand::Print("hello".into()),
        ]);
    }

    #[test]
    fn button_at_origin() {
        let nodes = vec![make_positioned("button", 0, 0, 6, 1, Some("[ OK ]"))];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds, vec![
            DrawCommand::MoveTo(0, 0),
            DrawCommand::Print("[ OK ]".into()),
        ]);
    }

    #[test]
    fn screen_emits_clear() {
        let nodes = vec![make_positioned("screen", 0, 0, 80, 24, None)];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds, vec![DrawCommand::Clear]);
    }

    #[test]
    fn text_wrapping_in_render() {
        let nodes = vec![make_positioned("text", 0, 0, 5, 2, Some("HelloWorld"))];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds, vec![
            DrawCommand::MoveTo(0, 0),
            DrawCommand::Print("Hello".into()),
            DrawCommand::MoveTo(0, 1),
            DrawCommand::Print("World".into()),
        ]);
    }

    #[test]
    fn counter_view_sequence() {
        // Simulate a minimal counter view: screen > [text, button, button]
        let nodes = vec![
            make_positioned("screen", 0, 0, 80, 24, None),
            make_positioned("text", 0, 0, 80, 1, Some("Count: 0")),
            make_positioned("button", 0, 1, 5, 1, Some("[ + ]")),
            make_positioned("button", 6, 1, 5, 1, Some("[ - ]")),
        ];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds[0], DrawCommand::Clear);
        assert_eq!(cmds[1], DrawCommand::MoveTo(0, 0));
        assert_eq!(cmds[2], DrawCommand::Print("Count: 0".into()));
        assert_eq!(cmds[3], DrawCommand::MoveTo(0, 1));
        assert_eq!(cmds[4], DrawCommand::Print("[ + ]".into()));
        assert_eq!(cmds[5], DrawCommand::MoveTo(6, 1));
        assert_eq!(cmds[6], DrawCommand::Print("[ - ]".into()));
    }

    #[test]
    fn toggle_render() {
        let nodes = vec![make_positioned("toggle", 2, 5, 5, 1, Some("[x]"))];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds, vec![
            DrawCommand::MoveTo(2, 5),
            DrawCommand::Print("[x]".into()),
        ]);
    }

    fn make_positioned_with_props(
        node_type: &str,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
        text: Option<&str>,
        props: serde_json::Map<String, Value>,
    ) -> PositionedNode {
        PositionedNode {
            rect: Rect { x, y, width: w, height: h },
            node_type: node_type.into(),
            props,
            text: text.map(|s| s.to_string()),
        }
    }

    #[test]
    fn text_with_color() {
        use serde_json::json;
        let mut props = serde_json::Map::new();
        props.insert("color".into(), json!("green"));
        props.insert("value".into(), json!("ok"));
        let nodes = vec![make_positioned_with_props("text", 0, 0, 80, 1, Some("ok"), props)];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds[0], DrawCommand::SetColor(Color::Green));
        assert_eq!(cmds[1], DrawCommand::MoveTo(0, 0));
        assert_eq!(cmds[2], DrawCommand::Print("ok".into()));
        assert_eq!(cmds[3], DrawCommand::ResetColor);
        assert_eq!(cmds[4], DrawCommand::SetAttribute(Attribute::Reset));
    }

    #[test]
    fn text_with_reverse() {
        use serde_json::json;
        let mut props = serde_json::Map::new();
        props.insert("reverse".into(), json!(true));
        props.insert("value".into(), json!("sel"));
        let nodes = vec![make_positioned_with_props("text", 0, 0, 80, 1, Some("sel"), props)];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds[0], DrawCommand::SetAttribute(Attribute::Reverse));
        assert_eq!(cmds[1], DrawCommand::MoveTo(0, 0));
        assert_eq!(cmds[2], DrawCommand::Print("sel".into()));
        assert_eq!(cmds[3], DrawCommand::ResetColor);
        assert_eq!(cmds[4], DrawCommand::SetAttribute(Attribute::Reset));
    }

    #[test]
    fn text_with_bg_and_bold() {
        use serde_json::json;
        let mut props = serde_json::Map::new();
        props.insert("bg".into(), json!("blue"));
        props.insert("bold".into(), json!(true));
        props.insert("value".into(), json!("hi"));
        let nodes = vec![make_positioned_with_props("text", 0, 0, 80, 1, Some("hi"), props)];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds[0], DrawCommand::SetBgColor(Color::Blue));
        assert_eq!(cmds[1], DrawCommand::SetAttribute(Attribute::Bold));
        assert_eq!(cmds[2], DrawCommand::MoveTo(0, 0));
        assert_eq!(cmds[3], DrawCommand::Print("hi".into()));
        assert_eq!(cmds[4], DrawCommand::ResetColor);
        assert_eq!(cmds[5], DrawCommand::SetAttribute(Attribute::Reset));
    }

    #[test]
    fn text_with_italic() {
        use serde_json::json;
        let mut props = serde_json::Map::new();
        props.insert("italic".into(), json!(true));
        props.insert("value".into(), json!("slant"));
        let nodes = vec![make_positioned_with_props("text", 0, 0, 80, 1, Some("slant"), props)];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds[0], DrawCommand::SetAttribute(Attribute::Italic));
        assert_eq!(cmds[1], DrawCommand::MoveTo(0, 0));
        assert_eq!(cmds[2], DrawCommand::Print("slant".into()));
        assert_eq!(cmds[3], DrawCommand::ResetColor);
        assert_eq!(cmds[4], DrawCommand::SetAttribute(Attribute::Reset));
    }

    #[test]
    fn color_from_name_mapping() {
        assert_eq!(super::color_from_name("red"), Some(Color::Red));
        assert_eq!(super::color_from_name("green"), Some(Color::Green));
        assert_eq!(super::color_from_name("grey"), Some(Color::Grey));
        assert_eq!(super::color_from_name("gray"), Some(Color::Grey));
        assert_eq!(super::color_from_name("invalid"), None);
    }

    #[test]
    fn input_render() {
        let nodes = vec![make_positioned("input", 0, 0, 10, 1, Some("Name"))];
        let cmds = render_to_commands(&nodes);
        assert_eq!(cmds[0], DrawCommand::MoveTo(0, 0));
        assert_eq!(cmds[1], DrawCommand::SetColor(Color::DarkGrey));
        assert_eq!(cmds[2], DrawCommand::Print("Name      ".into()));
        assert_eq!(cmds[3], DrawCommand::ResetColor);
    }
}
