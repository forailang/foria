use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, Button, Label, Orientation};
use std::cell::Cell;
use std::rc::Rc;

fn main() {
    let app = Application::builder()
        .application_id("dev.forai.gtk4_spike")
        .build();

    app.connect_activate(|app| {
        let count = Rc::new(Cell::new(0));

        let container = GtkBox::new(Orientation::Vertical, 12);
        container.set_margin_top(16);
        container.set_margin_bottom(16);
        container.set_margin_start(16);
        container.set_margin_end(16);

        let label = Label::new(Some("GTK4 spike count: 0"));
        let button = Button::with_label("Increment");

        let label_clone = label.clone();
        let count_clone = Rc::clone(&count);
        button.connect_clicked(move |_| {
            let next = count_clone.get() + 1;
            count_clone.set(next);
            label_clone.set_text(&format!("GTK4 spike count: {next}"));
        });

        container.append(&label);
        container.append(&button);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("forai GTK4 Spike")
            .default_width(420)
            .default_height(180)
            .child(&container)
            .build();

        window.present();
    });

    app.run();
}
