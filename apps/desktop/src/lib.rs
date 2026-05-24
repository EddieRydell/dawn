pub mod actions;
pub mod app_model;
pub mod editor_session;
pub mod layout_persistence;
pub mod ui;
pub mod workspace;

pub fn run() {
    ui::shell::run();
}
