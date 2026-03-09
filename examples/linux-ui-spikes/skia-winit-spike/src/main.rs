use skia_safe::{surfaces, Color, Paint, PaintStyle, Rect};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes};

fn render_with_skia(width: i32, height: i32, active: bool) {
    let Some(mut surface) = surfaces::raster_n32_premul((width, height)) else {
        return;
    };

    let canvas = surface.canvas();
    canvas.clear(Color::WHITE);

    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(PaintStyle::Fill);
    paint.set_color(if active { Color::from_rgb(0x26, 0xa6, 0x5b) } else { Color::from_rgb(0x45, 0x7b, 0xc4) });

    let rect = Rect::from_xywh(24.0, 24.0, (width - 48).max(40) as f32, (height - 48).max(40) as f32);
    canvas.draw_rect(rect, &paint);
    // Raster surfaces don't require explicit flush in skia-safe 0.90.
}

struct App {
    window: Option<Window>,
    active: bool,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            active: false,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = WindowAttributes::default()
            .with_title("forai Skia+winit Spike (click to toggle)")
            .with_visible(true)
            .with_inner_size(LogicalSize::new(640.0, 420.0));

        let window = event_loop
            .create_window(attrs)
            .expect("failed to create window");
        window.request_redraw();
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.as_ref() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(_) => {
                window.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.active = !self.active;
                let suffix = if self.active { "active" } else { "idle" };
                window.set_title(&format!("forai Skia+winit Spike ({suffix})"));
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let size = window.inner_size();
                render_with_skia(size.width as i32, size.height as i32, self.active);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("failed to create event loop");
    let mut app = App::new();
    event_loop.run_app(&mut app).expect("event loop error");
}
