# 0.2.0

* Add support for Redox/Orbital.
* Add support for BSD distributions.
* Ported Windows backend from `winapi` to `windows-sys`.
* **Breaking:** Take a reference to a window instead of owning the window.
* Add a `from_raw` function for directly using raw handles.
* Improvements for Wayland support.
* Support for HiDPI on macOS.
* **Breaking:** Add feature flags for `x11` and `wayland` backends.
* Use static dispatch instead of dynamic dispatch for the backends.
* Add `libxcb` support to the X11 backend.
* Use X11 MIT-SHM extension, if available.

# 0.1.1

* Added WASM support (Thanks to [Liamolucko](https://github.com/Liamolucko)!)
* CALayer is now used for Mac OS backend, which is more flexible about what happens in the windowing library (Thanks to [lunixbochs](https://github.com/lunixbochs)!)

# 0.1.0

Initial published version with support for Linux (X11 and Wayland), Mac OS (but buggy), and Windows.
