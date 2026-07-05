fn main() {
    let is_release = std::env::var("PROFILE").as_deref() == Ok("release");
    let is_windows_msvc = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let is_supported_thunk_arch = target_arch == "x86" || target_arch == "x86_64";

    if is_release && is_windows_msvc && is_supported_thunk_arch {
        thunk::thunk();
    } else {
        println!("cargo:warning=thunk-rs skipped for this build target");
    }
}
