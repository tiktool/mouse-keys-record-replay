#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
#[cfg(windows)]
mod appicon;
mod autostart;
mod config;
mod engine;
mod icons;
mod input;
mod shortcuts;
mod ui;
mod update;

fn main() {
    app::run();
}
