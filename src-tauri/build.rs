fn main() {
    // W55a spike: link tdjson (feature download-tdlib tải prebuilt TDLib từ GitHub releases).
    tdlib_rs::build::build(None);
    tauri_build::build()
}
