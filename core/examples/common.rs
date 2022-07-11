use eframe::egui;

pub trait DisplayModule {
    fn width(&self) -> f32;
    fn is_open(&self) -> bool;
    fn update(&mut self, ui: &mut egui::Ui);
}

pub struct UiUpdate {
    pub input: bool,
    pub id: usize,
    pub on: bool,
}

pub struct Jack<'a> {
    on: &'a mut bool,
    text: egui::WidgetText,
}

impl<'a> Jack<'a> {
    pub fn new(on: &'a mut bool, text: impl Into<egui::WidgetText>) -> Self {
        Jack {
            on,
            text: text.into(),
        }
    }
}

impl<'a> egui::Widget for Jack<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.horizontal(|ui| {
            let desired_size = ui.spacing().interact_size.y * egui::vec2(1.0, 1.0);
            let (rect, mut response) =
                ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());
            if response.dragged() {
                if !*self.on {
                    response.mark_changed();
                }
                *self.on = true;
            } else if response.drag_released() {
                if *self.on {
                    response.mark_changed();
                }
                *self.on = false;
            }
            if ui.is_rect_visible(rect) {
                let visuals = ui.style().interact_selectable(&response, *self.on);
                let rect = rect.expand(visuals.expansion);
                let radius = 0.5 * rect.height();
                ui.painter()
                    .rect(rect, radius, visuals.bg_fill, visuals.bg_stroke);
                ui.painter().circle(
                    rect.center(),
                    0.75 * radius,
                    visuals.bg_fill,
                    visuals.fg_stroke,
                );
            }
            response | ui.checkbox(self.on, self.text)
        })
        .inner
    }
}
