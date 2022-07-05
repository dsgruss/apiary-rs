use eframe::egui;

pub trait DisplayModule {
    fn is_open(&self) -> bool;
    fn update(&mut self, ctx: &egui::Context);
}
