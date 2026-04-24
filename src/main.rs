#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod analysis;
mod app;
mod cache;
mod replay;

use macroquad::prelude::{load_ttf_font, Conf, FilterMode, Font};

fn window_conf() -> Conf {
    Conf {
        window_title: "Tetr.io Replay Analyzer".to_owned(),
        window_width: 1600,
        window_height: 900,
        high_dpi: true,
        sample_count: 4,
        ..Default::default()
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    install_ui_font().await;
    let mut app = app::App::new();

    loop {
        app.frame();
        macroquad::prelude::next_frame().await;
    }
}

async fn install_ui_font() {
    #[cfg(target_os = "windows")]
    {
        const WINDOWS_UI_FONT_PATHS: &[&str] = &[
            "C:/Windows/Fonts/segoeui.ttf",
            "C:/Windows/Fonts/arial.ttf",
        ];

        let preload_sizes = [15_u16, 16, 18, 20, 22, 26, 28, 30];
        let ascii = Font::ascii_character_list();

        for path in WINDOWS_UI_FONT_PATHS {
            if let Ok(mut font) = load_ttf_font(path).await {
                font.set_filter(FilterMode::Linear);
                for size in preload_sizes {
                    font.populate_font_cache(&ascii, size);
                }
                app::install_ui_font(font);
                break;
            }
        }
    }
}
