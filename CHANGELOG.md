# 0.3.0

* On MacOS, the contents scale is updated when set_buffer() is called, to adapt when the window is on a new screen (#68).
* **Breaking:** Split the `GraphicsContext` type into `Context` and `Surface` (#64).
* On Web, cache the document in the `Context` type (#66).
* **Breaking:** Introduce a new "owned buffer" for no-copy presentation (#65).
* Enable support for multi-threaded WASM (#77).
* Fix buffer resizing on X11 (#69).
* Add a set of functions for handling buffer damage (#99).
* Add a `fetch()` function for getting the window contents (#104).
* Bump MSRV to 1.64 (#81).

# 0.2.1

* Bump `windows-sys` to 0.48

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
