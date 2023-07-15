fn main() {
    cfg_aliases::cfg_aliases! {
        free_unix: { all(unix, not(any(target_vendor = "apple", target_os = "android", target_os = "redox"))) },
        kms_platform: { all(feature = "kms", free_unix, not(target_arch = "wasm32")) },
        x11_platform: { all(feature = "x11", free_unix, not(target_arch = "wasm32")) },
        wayland_platform: { all(feature = "wayland", free_unix, not(target_arch = "wasm32")) },
    }
}
