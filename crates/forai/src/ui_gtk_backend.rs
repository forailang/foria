use crate::ui_gtk::{GtkNode, normalize_event, parse_node};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Align, Orientation};
use serde_json::{Value, json};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

enum GtkCmd {
    Mount(Value),
    Update(Value),
    Shutdown,
}

pub struct GtkBackend {
    cmd_tx: Sender<GtkCmd>,
    event_rx: Receiver<Value>,
}

impl GtkBackend {
    pub fn start() -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<GtkCmd>();
        let (event_tx, event_rx) = mpsc::channel::<Value>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();

        std::thread::spawn(move || {
            let init = gtk4::init().map_err(|e| format!("GTK init failed: {e}"));
            if let Err(e) = init {
                let _ = ready_tx.send(Err(e));
                return;
            }

            let window = gtk4::Window::builder()
                .title("forai Linux UI (GTK)")
                .default_width(960)
                .default_height(640)
                .build();
            window.present();

            let _ = ready_tx.send(Ok(()));

            let loop_ = glib::MainLoop::new(None, false);
            let loop_quit = loop_.clone();
            let event_tx_close = event_tx.clone();
            window.connect_close_request(move |_| {
                let _ = event_tx_close.send(json!({ "type": "window", "action": "close" }));
                loop_quit.quit();
                glib::Propagation::Proceed
            });

            let loop_for_cmd = loop_.clone();
            let window_for_cmd = window.clone();
            glib::timeout_add_local(Duration::from_millis(16), move || {
                loop {
                    match cmd_rx.try_recv() {
                        Ok(GtkCmd::Mount(tree)) | Ok(GtkCmd::Update(tree)) => {
                            if let Ok(node) = parse_node(&tree) {
                                let widget = build_widget(&node, &event_tx);
                                window_for_cmd.set_child(Some(&widget));
                                window_for_cmd.present();
                            }
                        }
                        Ok(GtkCmd::Shutdown) => {
                            loop_for_cmd.quit();
                            return glib::ControlFlow::Break;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            loop_for_cmd.quit();
                            return glib::ControlFlow::Break;
                        }
                    }
                }
                glib::ControlFlow::Continue
            });

            loop_.run();
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { cmd_tx, event_rx }),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(format!("GTK startup channel error: {e}")),
        }
    }

    pub fn mount(&self, tree: Value) -> Result<(), String> {
        self.cmd_tx
            .send(GtkCmd::Mount(tree))
            .map_err(|e| format!("failed to send GTK mount command: {e}"))
    }

    pub fn update(&self, tree: Value) -> Result<(), String> {
        self.cmd_tx
            .send(GtkCmd::Update(tree))
            .map_err(|e| format!("failed to send GTK update command: {e}"))
    }

    pub fn next_event(&self) -> Result<Value, String> {
        self.event_rx
            .recv()
            .map_err(|e| format!("GTK event queue closed: {e}"))
    }

    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(GtkCmd::Shutdown);
    }
}

fn build_widget(node: &GtkNode, event_tx: &Sender<Value>) -> gtk4::Widget {
    match node.node_type.as_str() {
        "screen" => {
            let container = gtk4::Box::new(Orientation::Vertical, 0);
            apply_style_common(&container, node);
            for child in &node.children {
                let w = build_widget(child, event_tx);
                container.append(&w);
            }
            container.upcast::<gtk4::Widget>()
        }
        "vstack" => {
            let spacing = node
                .style
                .values
                .get("spacing")
                .and_then(|s| s.strip_suffix("px"))
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            let box_ = gtk4::Box::new(Orientation::Vertical, spacing);
            apply_style_common(&box_, node);
            for child in &node.children {
                let w = build_widget(child, event_tx);
                box_.append(&w);
            }
            box_.upcast::<gtk4::Widget>()
        }
        "hstack" => {
            let spacing = node
                .style
                .values
                .get("spacing")
                .and_then(|s| s.strip_suffix("px"))
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            let box_ = gtk4::Box::new(Orientation::Horizontal, spacing);
            apply_style_common(&box_, node);
            for child in &node.children {
                let w = build_widget(child, event_tx);
                box_.append(&w);
            }
            box_.upcast::<gtk4::Widget>()
        }
        "zstack" => {
            let overlay = gtk4::Overlay::new();
            apply_style_common(&overlay, node);
            for child in &node.children {
                let w = build_widget(child, event_tx);
                overlay.add_overlay(&w);
            }
            overlay.upcast::<gtk4::Widget>()
        }
        "text" => {
            let label = gtk4::Label::new(node.text.as_deref());
            label.set_xalign(0.0);
            label.set_wrap(true);
            apply_style_common(&label, node);
            label.upcast::<gtk4::Widget>()
        }
        "html" => {
            let raw_html = node.text.as_deref().unwrap_or("");
            let pango = html_to_pango(raw_html);
            let label = gtk4::Label::new(None);
            label.set_markup(&pango);
            label.set_xalign(0.0);
            label.set_wrap(true);
            label.set_selectable(true);
            apply_style_common(&label, node);
            label.upcast::<gtk4::Widget>()
        }
        "button" => {
            let label_text = node.text.as_deref().unwrap_or("");
            let button = gtk4::Button::with_label(label_text);
            apply_style_common(&button, node);
            if let Some((action, value)) = node.events.iter().next() {
                let action = action.clone();
                let value = value.clone();
                let tx = event_tx.clone();
                button.connect_clicked(move |_| {
                    let _ = tx.send(normalize_event("button", &action, value.clone()));
                });
            }
            button.upcast::<gtk4::Widget>()
        }
        "input" => {
            let entry = gtk4::Entry::new();
            if let Some(ph) = node.props.get("placeholder").and_then(|v| v.as_str()) {
                entry.set_placeholder_text(Some(ph));
            }
            if let Some(v) = node.props.get("value").and_then(|v| v.as_str()) {
                entry.set_text(v);
            }
            apply_style_common(&entry, node);
            if let Some((action, _)) = node.events.iter().next() {
                let action = action.clone();
                let tx = event_tx.clone();
                entry.connect_changed(move |e| {
                    let _ = tx.send(normalize_event(
                        "input",
                        &action,
                        json!(e.text().to_string()),
                    ));
                });
            }
            entry.upcast::<gtk4::Widget>()
        }
        "toggle" => {
            let toggle = gtk4::CheckButton::new();
            let active = node
                .props
                .get("value")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            toggle.set_active(active);
            apply_style_common(&toggle, node);
            if let Some((action, _)) = node.events.iter().next() {
                let action = action.clone();
                let tx = event_tx.clone();
                toggle.connect_toggled(move |t| {
                    let _ = tx.send(normalize_event("toggle", &action, json!(t.is_active())));
                });
            }
            toggle.upcast::<gtk4::Widget>()
        }
        _ => {
            let container = gtk4::Box::new(Orientation::Vertical, 0);
            for child in &node.children {
                let w = build_widget(child, event_tx);
                container.append(&w);
            }
            container.upcast::<gtk4::Widget>()
        }
    }
}

/// Convert a subset of HTML to Pango markup for GTK Label rendering.
/// Supported: <b>, <strong>, <i>, <em>, <u>, <a href>, <br>, <p>, <li>, <blockquote>.
/// All other tags are stripped. Bare `&` is escaped for Pango.
fn html_to_pango(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c == '<' {
            // Extract the full tag
            let mut tag = String::new();
            for tc in chars.by_ref() {
                tag.push(tc);
                if tc == '>' {
                    break;
                }
            }
            let tag_lower = tag.to_lowercase();
            let tag_name = extract_tag_name(&tag_lower);

            match tag_name.as_str() {
                "br" => out.push('\n'),
                "p" => out.push('\n'),
                "/p" => out.push('\n'),
                "li" => out.push_str("\n • "),
                "/li" | "ul" | "/ul" | "ol" | "/ol" => {}
                "blockquote" => out.push_str("\n  "),
                "/blockquote" => out.push('\n'),
                "strong" => out.push_str("<b>"),
                "/strong" => out.push_str("</b>"),
                "em" => out.push_str("<i>"),
                "/em" => out.push_str("</i>"),
                // Pango-safe tags: keep as-is
                "b" | "/b" | "i" | "/i" | "u" | "/u" | "s" | "/s" | "sub" | "/sub"
                | "sup" | "/sup" | "big" | "/big" | "small" | "/small" | "tt" | "/tt" => {
                    out.push_str(&tag);
                }
                // <a href="..."> → keep for Pango
                "a" => out.push_str(&tag),
                "/a" => out.push_str("</a>"),
                // <span> → keep for Pango
                "span" => out.push_str(&tag),
                "/span" => out.push_str("</span>"),
                // All other tags: strip
                _ => {}
            }
        } else if c == '&' {
            // Collect the entity
            let mut entity = String::new();
            for ec in chars.by_ref() {
                entity.push(ec);
                if ec == ';' || entity.len() > 10 {
                    break;
                }
            }
            match entity.as_str() {
                "&amp;" => out.push_str("&amp;"),
                "&lt;" => out.push_str("&lt;"),
                "&gt;" => out.push_str("&gt;"),
                "&quot;" => out.push('"'),
                "&mdash;" => out.push('—'),
                "&ndash;" => out.push('–'),
                "&hellip;" => out.push('…'),
                "&rsquo;" => out.push('\u{2019}'),
                "&lsquo;" => out.push('\u{2018}'),
                "&rdquo;" => out.push('\u{201D}'),
                "&ldquo;" => out.push('\u{201C}'),
                "&nbsp;" => out.push(' '),
                "&apos;" => out.push('\''),
                // Unknown entity or bare & — escape for Pango
                other => {
                    if other.ends_with(';') {
                        out.push_str(other);
                    } else {
                        out.push_str("&amp;");
                        out.push_str(&other[1..]);
                    }
                }
            }
        } else {
            chars.next();
            out.push(c);
        }
    }

    // Collapse 3+ newlines to 2
    let mut result = String::with_capacity(out.len());
    let mut nl_count = 0;
    for c in out.chars() {
        if c == '\n' {
            nl_count += 1;
            if nl_count <= 2 {
                result.push(c);
            }
        } else {
            nl_count = 0;
            result.push(c);
        }
    }

    result.trim().to_string()
}

/// Extract the tag name from a lowercased tag like "<p>", "</p>", "<a href=\"...\">", "<br/>".
fn extract_tag_name(tag: &str) -> String {
    let inner = tag
        .trim_start_matches('<')
        .trim_end_matches('>')
        .trim_end_matches('/')
        .trim();
    let is_closing = inner.starts_with('/');
    let name_part = if is_closing { &inner[1..] } else { inner };
    let name = name_part
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or("");
    if is_closing {
        format!("/{name}")
    } else {
        name.to_string()
    }
}

fn apply_style_common<W: IsA<gtk4::Widget>>(w: &W, node: &GtkNode) {
    let widget = w.as_ref();

    if let Some(width) = node
        .style
        .values
        .get("width")
        .and_then(|s| s.strip_suffix("px"))
        .and_then(|s| s.parse::<i32>().ok())
    {
        widget.set_width_request(width);
    }
    if let Some(height) = node
        .style
        .values
        .get("height")
        .and_then(|s| s.strip_suffix("px"))
        .and_then(|s| s.parse::<i32>().ok())
    {
        widget.set_height_request(height);
    }
    if let Some(margin) = node
        .style
        .values
        .get("margin")
        .and_then(|s| s.strip_suffix("px"))
        .and_then(|s| s.parse::<i32>().ok())
    {
        widget.set_margin_top(margin);
        widget.set_margin_bottom(margin);
        widget.set_margin_start(margin);
        widget.set_margin_end(margin);
    }
    if let Some(align) = node.style.values.get("align").map(String::as_str) {
        match align {
            "start" => widget.set_halign(Align::Start),
            "center" => widget.set_halign(Align::Center),
            "end" => widget.set_halign(Align::End),
            _ => {}
        }
    }
}
