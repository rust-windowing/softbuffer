fn main() {
    println!("cargo:rustc-check-cfg=cfg(free_unix)");
    println!("cargo:rustc-check-cfg=cfg(kms_platform)");
    println!("cargo:rustc-check-cfg=cfg(x11_platform)");
    println!("cargo:rustc-check-cfg=cfg(wayland_platform)");

    cfg_aliases::cfg_aliases! {
        free_unix: { all(unix, not(any(target_vendor = "apple", target_os = "android", target_os = "redox"))) },
        kms_platform: { all(feature = "kms", free_unix, not(target_arch = "wasm32")) },
        x11_platform: { all(feature = "x11", free_unix, not(target_arch = "wasm32")) },
        wayland_platform: { all(feature = "wayland", free_unix, not(target_arch = "wasm32")) },
    }
}
