use std::f32::consts::PI;
use eframe::egui;

#[cfg(feature = "network-local")]
use apiary_core::socket_local::LocalInterface;
#[cfg(feature = "network-local")]
pub type SelectedInterface<const I: usize, const O: usize> = LocalInterface<I, O>;

#[cfg(feature = "network-native")]
use apiary_core::socket_native::NativeInterface;
#[cfg(feature = "network-native")]
pub type SelectedInterface<const I: usize, const O: usize> = NativeInterface<I, O>;

pub trait DisplayModule {
    fn width(&self) -> f32;
    fn is_open(&self) -> bool;
    fn update(&mut self, ui: &mut egui::Ui);
}

#[derive(Debug)]
pub struct UiUpdate {
    pub input: bool,
    pub id: usize,
    pub on: bool,
}

impl UiUpdate {
    pub fn new(input: bool, id: usize, on: bool) -> Self {
        UiUpdate { input, id, on }
    }
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

pub struct Knob<'a> {
    value: &'a mut f32,
    text: egui::WidgetText,
    from: f32,
    to: f32,
    log: bool,
    drag_value: f32,
    log_scale: f32,
}

impl<'a> Knob<'a> {
    pub fn new(
        value: &'a mut f32,
        text: impl Into<egui::WidgetText>,
        from: f32,
        to: f32,
        log: bool,
    ) -> Self {
        let mut log_scale = 1.0;
        let drag_value = if log {
            log_scale = (to / from).log10();
            (*value / from).log10() / log_scale
        } else {
            (*value - from) / (to - from)
        };
        Knob {
            value,
            text: text.into(),
            from,
            to,
            log,
            drag_value,
            log_scale,
        }
    }
}

impl<'a> egui::Widget for Knob<'a> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let desired_size = ui.spacing().interact_size.y * egui::vec2(4.0, 4.0);
        let (rect, mut response) =
            ui.allocate_exact_size(desired_size, egui::Sense::click_and_drag());
        if response.dragged() {
            let drag_value = (self.drag_value - response.drag_delta().y / 300.0).clamp(0.0, 1.0);
            *self.value = if self.log {
                self.from * (10.0_f32).powf(self.log_scale * drag_value)
            } else {
                drag_value * (self.to - self.from) + self.from
            };
            response.mark_changed();
        }
        if ui.is_rect_visible(rect) {
            let visuals = ui
                .style()
                .interact_selectable(&response, response.dragged());
            let rect = rect.expand(visuals.expansion);
            let radius = rect.height() * 0.3;
            for i in 0..100 {
                let pos0 = rect.center() + frac_to_vec(i as f32 / 100.0, radius);
                let pos1 = rect.center() + frac_to_vec((i + 1) as f32 / 100.0, radius);
                ui.painter().line_segment([pos0, pos1], visuals.fg_stroke);
            }
            ui.painter().circle(
                rect.center() + frac_to_vec(self.drag_value, radius),
                radius * 0.25,
                visuals.bg_fill,
                visuals.fg_stroke,
            );
        }
        if response.changed() {
            egui::show_tooltip(ui.ctx(), egui::Id::new("value"), |ui| {
                ui.label(format!("{:.1}", *self.value));
            });
        }
        ui.label(self.text);
        response
    }
}

fn frac_to_vec(val: f32, radius: f32) -> egui::Vec2 {
    let theta = val * 300.0 + 120.0;
    [
        radius * (PI * theta / 180.0).cos(),
        radius * (PI * theta / 180.0).sin(),
    ]
    .into()
}
