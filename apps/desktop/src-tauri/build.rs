fn main() {
    println!("cargo:rerun-if-changed=native/Voice.swift");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        use std::{path::PathBuf, process::Command};
        let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let generated = root.join("native/generated");
        std::fs::create_dir_all(&generated).expect("Create native build folder");
        let cache = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("swift-cache");
        let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
            Ok("aarch64") => "arm64",
            Ok("x86_64") => "x86_64",
            _ => panic!("Unsupported macOS speech architecture"),
        };
        let status = Command::new("xcrun")
            .args([
                "swiftc",
                "-swift-version",
                "5",
                "-emit-library",
                "-module-name",
                "PCCVoice",
                "-target",
                &format!("{arch}-apple-macos11.0"),
                "-module-cache-path",
            ])
            .arg(&cache)
            .arg(root.join("native/Voice.swift"))
            .arg("-o")
            .arg(generated.join("libpcc_voice.dylib"))
            .args([
                "-Xlinker",
                "-install_name",
                "-Xlinker",
                "@rpath/libpcc_voice.dylib",
            ])
            .status()
            .expect("Xcode Swift compiler is required for macOS dictation");
        assert!(status.success(), "Swift voice bridge compilation failed");
        println!("cargo:rustc-link-search=native={}", generated.display());
        println!("cargo:rustc-link-lib=dylib=pcc_voice");
        println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Frameworks");
        if std::env::var("PROFILE").as_deref() == Ok("debug") {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", generated.display());
        }
    }
    tauri_build::build()
}
