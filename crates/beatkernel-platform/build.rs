//! Build only an explicitly requested, caller-supplied Windows ASIO SDK bridge.
fn main() {
    println!("cargo:rerun-if-env-changed=BEATKERNEL_ASIO_SDK_DIR");
    println!("cargo:rerun-if-changed=src/windows/asio/bridge.cpp");
    #[cfg(feature = "asio-sdk")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        build_sdk();
    }
}

#[cfg(feature = "asio-sdk")]
fn build_sdk() {
    use std::path::PathBuf;
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        panic!("asio-sdk currently requires a Windows MSVC target and MSVC/clang-cl C++ compiler; GNU IASIO C++ ABI compatibility is not implemented");
    }
    let root = PathBuf::from(std::env::var_os("BEATKERNEL_ASIO_SDK_DIR")
        .filter(|value| !value.is_empty())
        .expect("asio-sdk requires BEATKERNEL_ASIO_SDK_DIR pointing to a caller-supplied licensed SDK; builds never download it"));
    let common = root.join("common");
    for name in ["iasiodrv.h", "asio.h", "asiosys.h"] {
        let header = common.join(name);
        if !header.is_file() {
            panic!("asio-sdk requires actual SDK header {}", header.display());
        }
        println!("cargo:rerun-if-changed={}", header.display());
    }
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .include(common)
        .file("src/windows/asio/bridge.cpp")
        .define("WIN32_LEAN_AND_MEAN", None)
        .define("NOMINMAX", None)
        .flag("/EHsc");
    let compiler = build.try_get_compiler().unwrap_or_else(|error| {
        panic!("asio-sdk requires an available MSVC/clang-cl Windows C++ compiler: {error}");
    });
    if !compiler.is_like_msvc() {
        panic!("asio-sdk requires a compiler using the MSVC IASIO C++ ABI");
    }
    build
        .try_compile("beatkernel_asio_control")
        .unwrap_or_else(|error| {
            panic!("caller-supplied SDK control bridge compilation failed: {error}");
        });
    println!("cargo:rustc-link-lib=ole32");
    println!("cargo:rustc-link-lib=user32");
}
