use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    // W57a-v2: KHÔNG gọi tdlib_rs::build::build() nữa (tdlib-rs đã bỏ khỏi
    // [build-dependencies]). Lý do: build-dep đó làm binary build-script của
    // browserx link động libtdjson → cargo check/clippy chạy build script bị
    // dyld abort trên máy không set DYLD_FALLBACK_LIBRARY_PATH.
    //
    // Link-search + link-lib cho tdjson vẫn do build script của dependency
    // thường `tdlib-rs` (feature download-tdlib) emit. Nhưng rustc-link-arg
    // (rpath) của dep không áp vào bin của package này, nên tự emit LC_RPATH
    // trỏ vào out-dir của tdlib-rs để app runtime load được libtdjson (W55b).
    emit_tdjson_rpath();
    tauri_build::build()
}

fn emit_tdjson_rpath() {
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        // Windows không dùng rpath; tdjson.dll được tdlib-rs copy vào ~/.cargo/bin.
        return;
    }

    // OUT_DIR = <target>/<profile>/build/browserx-<hash>/out
    // → <target>/<profile>/build là ancestor thứ 2, nơi chứa tdlib-rs-<hash>/out.
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let Some(build_root) = out_dir.ancestors().nth(2).map(PathBuf::from) else {
        return;
    };

    let mut lib_dirs: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(&build_root) {
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with("tdlib-rs-") {
                continue;
            }
            let lib_dir = entry.path().join("out").join("tdlib").join("lib");
            let has_tdjson = fs::read_dir(&lib_dir)
                .map(|files| {
                    files.flatten().any(|f| {
                        f.file_name()
                            .to_string_lossy()
                            .starts_with("libtdjson.")
                    })
                })
                .unwrap_or(false);
            if has_tdjson {
                lib_dirs.push(lib_dir);
            }
        }
    }

    if lib_dirs.is_empty() {
        // Fresh build: tdlib-rs không có `links` key nên cargo không đảm bảo
        // build script của nó chạy trước build.rs này. Emit rerun-if-changed
        // vào path chưa tồn tại để lần `cargo build` sau build.rs chạy lại và
        // gắn rpath (không block-wait để tránh deadlock với -j1).
        let sentinel = build_root.join(".browserx-tdjson-rpath-pending");
        println!("cargo:rerun-if-changed={}", sentinel.display());
        println!(
            "cargo:warning=libtdjson chưa có trong {}/tdlib-rs-*/out/tdlib/lib; \
             chạy lại `cargo build` để gắn LC_RPATH cho tdjson.",
            build_root.display()
        );
        return;
    }

    lib_dirs.sort();
    for dir in &lib_dirs {
        println!("cargo:rerun-if-changed={}", dir.display());
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
    }
}
