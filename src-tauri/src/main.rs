// Prevents an additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The same executable doubles as the isolated PDF extraction worker process.
    if std::env::args().nth(1).as_deref() == Some(papryvio_lib::PDF_WORKER_FLAG) {
        std::process::exit(papryvio_lib::run_pdf_worker());
    }
    papryvio_lib::run();
}
