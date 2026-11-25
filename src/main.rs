//! Visualizador moderno de CSV e planilhas.
//!
//! Uso: `csv_viwer [arquivo ...]`

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod data;
#[cfg(feature = "devshot")]
mod devshot;
mod document;
mod format;
mod icon;
mod theme;
mod ui;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|a| a.to_str()) {
        Some("-h" | "--help") => {
            println!(
                "Visualizador de Planilhas {}\n\n\
                 Uso: csv_viwer [ARQUIVO...]\n\n\
                 Abre arquivos CSV, TSV, TXT, Excel (.xlsx, .xls, .xlsb) e OpenDocument (.ods).\n\n\
                 Opções:\n  \
                 --write-icon CAMINHO [TAMANHO]  grava o ícone do app em PNG\n  \
                 -h, --help                      mostra esta ajuda\n  \
                 -V, --version                   mostra a versão",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        Some("-V" | "--version") => {
            println!("csv_viwer {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--write-icon") => {
            let path = args.get(1).map(PathBuf::from).unwrap_or_else(|| "csv-viwer.png".into());
            let size = args
                .get(2)
                .and_then(|s| s.to_str()?.parse().ok())
                .unwrap_or(256);
            if let Err(e) = icon::write_png(&path, size) {
                eprintln!("erro ao gravar o ícone: {e}");
                std::process::exit(1);
            }
            return Ok(());
        }
        _ => {}
    }
    let files: Vec<PathBuf> = args.into_iter().map(PathBuf::from).collect();

    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Visualizador de Planilhas")
            .with_app_id("csv-viwer")
            .with_inner_size([1320.0, 820.0])
            .with_min_inner_size([760.0, 480.0])
            .with_drag_and_drop(true)
            .with_icon(icon::window_icon()),
        ..Default::default()
    };
    #[cfg(feature = "devshot")]
    if let Some(state) = std::env::var_os("CSV_VIWER_STATE") {
        options.persistence_path = Some(PathBuf::from(state));
    }

    eframe::run_native(
        "csv_viwer",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, files)))),
    )
}
