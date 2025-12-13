# Unreleased

# 0.4.7

- Fix documentation building on `docs.rs`.

# 0.4.7

- Added support for Android using the `ndk` crate.
- Added support for `wasm64-*` targets.
- Improved examples.
- Added `Buffer::width()` and `Buffer::height()` getters.
- `Context` now implements `Clone`.
- `Context`, `Surface` and `Buffer` now implement `Debug`.
- Bump MSRV to Rust 1.71.
- Replace `log` with `tracing`.
- Remove `cfg_aliases` dependency.
- Update to `objc2` 0.6, `objc2-*` 0.3, `drm` 0.14, `rustix` 1.0 and `windows-sys` 0.61.

# 0.4.6

- Added support for iOS, tvOS, watchOS and visionOS (UIKit).
- Redo the way surfaces work on macOS to work directly with layers, which will
  allow initializing directly from a `CALayer` in the future.
- Update to `windows-sys` 0.59.0 and `core-graphics` v0.24.0.
- Add an MSRV policy.

# 0.4.5

- Make the `wayland-sys` dependency optional. (#223)
- Allow for transparent visuals on X11. This technically doesn't work, but
  otherwise `winit` users might get crashes. (#226)

# 0.4.4

- Make `Context` `Send`+`Sync` and `Surface` `Send`. (#217)

# 0.4.3

- Use objc2 as the backend for the CoreGraphics implementation. (#210)
- Update drm-rs to version v0.12.0. (#209)
- Bump MSRV to 1.70.0 to be in line with Winit.

# 0.4.2

- Add the ability to get the underlying window handle. (#193)
- Rework the backend to use a trait-based interface. (#196)
- On Orbital, fix window resize. (#200)
- Fix `bytes()` for KMS/DRM implementation. (#203)

# 0.4.1

- On MacOS, Fix double-free of `NSWindow`. (#180)
- Update `drm` to 0.11 (#178)
  * Fixes build on architectures where drm-rs did not have generated bindings.
- Update x11rb to v0.13 (#183)
- On Web, add support for more `RawWindowHandle` variants. (#188)
- On Wayland, fix buffer age. (#191)

# 0.4.0

- **Breaking:** Port to use `raw-window-handle` v0.6.(#132)
- Enable creating X11 displays without an existing connection. (#171)

# 0.3.3

- Fix a bug in the new shared memory model in X11. (#170)

# 0.3.2

* Document that `present_with_damage` is supported on web platforms. (#152)
* Replace our usage of `nix` with `rustix`. This enables this crate to run without `libc`. (#164)
* Use POSIX shared memory instead of Sys-V shared memory for the X11 backend. (#165)
* Bump version for the following dependencies:
  * `memmap2` (#156)
  * `redox_syscall` (#161)
  * `drm` (#163)

# 0.3.1

* On X11, fix the length of the returned buffer when using the wire-transferred buffer.
* On Web, fix incorrect starting coordinates when handling buffer damage.
* On Redox, use `MAP_SHARED`; fixing behavior with latest Orbital.
* Error instead of segfault on macOS if size isn't set.
* Add `OffscreenCanvas` support in web backend.
* Add DRM/KMS backend, for running on tty without X/Wayland.
* Make `fetch` error on Windows, where it wasn't working correctly.
* Implement `Error` trait for `SoftBufferError`.
* Dependency updates.

# 0.3.0

- On MacOS, the contents scale is updated when set_buffer() is called, to adapt when the window is on a new screen (#68).
- **Breaking:** Split the `GraphicsContext` type into `Context` and `Surface` (#64).
- On Web, cache the document in the `Context` type (#66).
- **Breaking:** Introduce a new "owned buffer" for no-copy presentation (#65).
- Enable support for multi-threaded WASM (#77).
- Fix buffer resizing on X11 (#69).
- Add a set of functions for handling buffer damage (#99).
- Add a `fetch()` function for getting the window contents (#104).
- Bump MSRV to 1.64 (#81).

# 0.2.1

- Bump `windows-sys` to 0.48

# 0.2.0

- Add support for Redox/Orbital.
- Add support for BSD distributions.
- Ported Windows backend from `winapi` to `windows-sys`.
- **Breaking:** Take a reference to a window instead of owning the window.
- Add a `from_raw` function for directly using raw handles.
- Improvements for Wayland support.
- Support for HiDPI on macOS.
- **Breaking:** Add feature flags for `x11` and `wayland` backends.
- Use static dispatch instead of dynamic dispatch for the backends.
- Add `libxcb` support to the X11 backend.
- Use X11 MIT-SHM extension, if available.

# 0.1.1

- Added WASM support (Thanks to [Liamolucko](https://github.com/Liamolucko)!)
- CALayer is now used for Mac OS backend, which is more flexible about what happens in the windowing library (Thanks to [lunixbochs](https://github.com/lunixbochs)!)

# 0.1.0

Initial published version with support for Linux (X11 and Wayland), Mac OS (but buggy), and Windows.
