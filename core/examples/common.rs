use eframe::egui;

pub trait DisplayModule {
    fn width(&self) -> f32;
    fn is_open(&self) -> bool;
    fn update(&mut self, ui: &mut egui::Ui);
}
