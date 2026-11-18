//! Presents into real windows and reads back what DWM shows.

use std::mem;
use std::num::{NonZeroIsize, NonZeroU32};
use std::ptr;
use std::slice;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{mpsc, Arc, Once};
use std::thread;
use std::time::{Duration, Instant};

use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawWindowHandle,
    Win32WindowHandle, WindowHandle,
};
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::Graphics::Gdi;
use windows_sys::Win32::Storage::Xps::{PrintWindow, PW_CLIENTONLY};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging as wm;

use super::{GdiPresenter, Presenter};
use crate::backend_dispatch::SurfaceDispatch;
use crate::{Context, Pixel, Rect, Surface};

/// Asks DWM for the window as it composes it, swap chains included.
const PW_RENDERFULLCONTENT: u32 = 2;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 150;

const RED: u32 = 0xff0000;
const GREEN: u32 = 0x00ff00;
const BLUE: u32 = 0x0000ff;
const YELLOW: u32 = 0xffff00;
const CYAN: u32 = 0x00ffff;

#[test]
fn dxgi_presents_damage() {
    let window = Window::new();
    let context = Context::new(&window).unwrap();
    let mut surface = Surface::new(&context, &window).unwrap();
    assert!(matches!(presenter(&mut surface), Presenter::Dxgi(_)));

    presents_damage(&window, &mut surface);
}

#[test]
fn gdi_presents_damage() {
    let window = Window::new();
    let context = Context::new(&window).unwrap();
    let mut surface = Surface::new(&context, &window).unwrap();
    // GDI still draws into the window, as nothing was presented through DXGI.
    *presenter(&mut surface) = Presenter::Gdi(GdiPresenter::new(window.0).unwrap());

    presents_damage(&window, &mut surface);
}

/// Presents a whole frame, rectangles that cover each other, and a smaller frame.
///
/// With two buffers, each partial present lands on the buffer before the previous one, so the
/// window only shows the right frame if the previous frame's damage reached it too.
fn presents_damage(window: &Window, surface: &mut Surface<&Window, &Window>) {
    let mut expected = Frame::new(WIDTH, HEIGHT, RED);
    surface
        .resize(
            NonZeroU32::new(WIDTH).unwrap(),
            NonZeroU32::new(HEIGHT).unwrap(),
        )
        .unwrap();
    let mut buffer = surface.next_buffer().unwrap();
    assert_eq!(buffer.age(), 0);
    expected.draw_into(&mut buffer);
    buffer.present().unwrap();
    window.assert_shows(&expected);

    let frames: [&[(Rect, u32)]; 6] = [
        &[(rect(20, 20, 50, 40), BLUE)],
        &[(rect(110, 70, 60, 50), GREEN)],
        &[(rect(40, 40, 90, 60), YELLOW)],
        &[(rect(20, 20, 50, 40), RED)],
        // Damage that reaches past the frame is cut to it.
        &[(rect(150, 100, 100, 100), CYAN)],
        // Rectangles that overlap.
        &[(rect(30, 90, 50, 40), BLUE), (rect(60, 110, 50, 30), GREEN)],
    ];
    for damage in frames {
        let mut buffer = surface.next_buffer().unwrap();
        assert_eq!(buffer.age(), 1);
        for &(rect, color) in damage {
            expected.fill(rect, color);
        }
        expected.draw_into(&mut buffer);
        let damage: Vec<Rect> = damage.iter().map(|&(rect, _)| rect).collect();
        buffer.present_with_damage(&damage).unwrap();
        window.assert_shows(&expected);
    }

    // A frame smaller than the window is drawn in its top left corner.
    let expected = Frame::new(WIDTH / 2, HEIGHT / 2, CYAN);
    surface
        .resize(
            NonZeroU32::new(WIDTH / 2).unwrap(),
            NonZeroU32::new(HEIGHT / 2).unwrap(),
        )
        .unwrap();
    let mut buffer = surface.next_buffer().unwrap();
    assert_eq!(buffer.age(), 0);
    expected.draw_into(&mut buffer);
    buffer.present().unwrap();
    window.assert_shows(&expected);
}

/// Like the `winit_multithread` example, presents from another thread while the window's thread
/// waits for it.
#[test]
fn dxgi_presents_from_another_thread() {
    let window = Arc::new(Window::new());
    let context = Context::new(window.clone()).unwrap();
    let mut surface = Surface::new(&context, window.clone()).unwrap();
    assert!(matches!(presenter(&mut surface), Presenter::Dxgi(_)));

    let expected = Arc::new(Frame::new(WIDTH, HEIGHT, GREEN));
    let (done, finished) = mpsc::channel();
    let frame = expected.clone();
    thread::spawn(move || {
        surface
            .resize(
                NonZeroU32::new(WIDTH).unwrap(),
                NonZeroU32::new(HEIGHT).unwrap(),
            )
            .unwrap();
        for _ in 0..3 {
            let mut buffer = surface.next_buffer().unwrap();
            frame.draw_into(&mut buffer);
            buffer.present().unwrap();
        }
        done.send(surface).unwrap();
    });
    let surface = finished
        .recv_timeout(Duration::from_secs(10))
        .expect("presenting from another thread blocked");
    window.assert_shows(&expected);
    // The window shows the swap chain only while it exists.
    drop(surface);
}

/// Presents while the window is minimized, then shows the last frame once it's restored.
#[test]
fn dxgi_presents_to_a_minimized_window() {
    let window = Arc::new(Window::new());
    let context = Context::new(window.clone()).unwrap();
    let mut surface = Surface::new(&context, window.clone()).unwrap();
    assert!(matches!(presenter(&mut surface), Presenter::Dxgi(_)));

    let mut expected = Frame::new(WIDTH, HEIGHT, RED);
    surface
        .resize(
            NonZeroU32::new(WIDTH).unwrap(),
            NonZeroU32::new(HEIGHT).unwrap(),
        )
        .unwrap();
    let mut buffer = surface.next_buffer().unwrap();
    expected.draw_into(&mut buffer);
    buffer.present().unwrap();
    window.assert_shows(&expected);

    unsafe { wm::ShowWindow(window.0, wm::SW_MINIMIZE) };
    pump();
    // On another thread, so that a present that waits for the window fails the test.
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        for (index, color) in [BLUE, GREEN, YELLOW].into_iter().enumerate() {
            let damage = rect(20 + 40 * index as u32, 20, 30, 30);
            let mut buffer = surface.next_buffer().unwrap();
            expected.fill(damage, color);
            expected.draw_into(&mut buffer);
            buffer.present_with_damage(&[damage]).unwrap();
        }
        done.send((surface, expected)).unwrap();
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut surface, expected) = loop {
        pump();
        match finished.recv_timeout(Duration::from_millis(10)) {
            Ok(result) => break result,
            Err(_) if Instant::now() < deadline => {}
            Err(_) => panic!("presenting to a minimized window blocked"),
        }
    };

    unsafe { wm::ShowWindow(window.0, wm::SW_RESTORE) };
    let buffer = surface.next_buffer().unwrap();
    buffer.present_with_damage(&[]).unwrap();
    window.assert_shows(&expected);
}

fn presenter<D, W>(surface: &mut Surface<D, W>) -> &mut Presenter {
    let SurfaceDispatch::Win32(surface) = &mut *surface.surface_impl;
    &mut surface.presenter
}

fn rect(x: u32, y: u32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y,
        width: NonZeroU32::new(width).unwrap(),
        height: NonZeroU32::new(height).unwrap(),
    }
}

/// The frame that the window should show, as `0xRRGGBB` values.
struct Frame {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
}

impl Frame {
    fn new(width: u32, height: u32, color: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![color; (width * height) as usize],
        }
    }

    fn fill(&mut self, rect: Rect, color: u32) {
        let right = (rect.x + rect.width.get()).min(self.width);
        let bottom = (rect.y + rect.height.get()).min(self.height);
        for y in rect.y..bottom {
            for x in rect.x..right {
                self.pixels[(y * self.width + x) as usize] = color;
            }
        }
    }

    fn draw_into(&self, buffer: &mut crate::Buffer<'_>) {
        assert_eq!(buffer.width().get(), self.width);
        assert_eq!(buffer.height().get(), self.height);
        for (row, pixels) in buffer
            .pixel_rows()
            .zip(self.pixels.chunks(self.width as usize))
        {
            for (pixel, &color) in row.iter_mut().zip(pixels) {
                let [b, g, r, _] = color.to_le_bytes();
                *pixel = Pixel::new_rgb(r, g, b);
            }
        }
    }
}

/// A top-level window with a client area of `WIDTH` by `HEIGHT` pixels.
struct Window(HWND);

// SAFETY: A window handle can be used from any thread.
unsafe impl Send for Window {}
unsafe impl Sync for Window {}

impl Window {
    fn new() -> Self {
        static REGISTER: Once = Once::new();
        static NEXT_POSITION: AtomicI32 = AtomicI32::new(0);

        let class = windows_sys::w!("softbuffer test window");
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        REGISTER.call_once(|| {
            // Read back pixels instead of scaled ones on displays with a scale factor.
            unsafe { wm::SetProcessDPIAware() };
            let class = wm::WNDCLASSW {
                lpfnWndProc: Some(wm::DefWindowProcW),
                hInstance: instance,
                lpszClassName: class,
                ..unsafe { mem::zeroed() }
            };
            assert_ne!(unsafe { wm::RegisterClassW(&class) }, 0);
        });

        let style = wm::WS_OVERLAPPEDWINDOW | wm::WS_VISIBLE;
        let mut frame = RECT {
            left: 0,
            top: 0,
            right: WIDTH as i32,
            bottom: HEIGHT as i32,
        };
        unsafe { wm::AdjustWindowRectEx(&mut frame, style, 0, 0) };
        // Side by side, so that windows of tests that run at the same time don't cover each other.
        let position = 50 + 250 * (NEXT_POSITION.fetch_add(1, Ordering::Relaxed) % 4);
        let hwnd = unsafe {
            wm::CreateWindowExW(
                0,
                class,
                windows_sys::w!("softbuffer"),
                style,
                position,
                50,
                frame.right - frame.left,
                frame.bottom - frame.top,
                ptr::null_mut(),
                ptr::null_mut(),
                instance,
                ptr::null(),
            )
        };
        assert!(!hwnd.is_null(), "failed to create a window");
        pump();
        Self(hwnd)
    }

    /// Waits until DWM shows the frame in the top left corner of the client area.
    fn assert_shows(&self, expected: &Frame) {
        // Leave out the corners, which Windows 11 rounds.
        let inside = |x: u32, y: u32| {
            (8..expected.width.min(WIDTH) - 8).contains(&x)
                && (8..expected.height.min(HEIGHT) - 8).contains(&y)
        };
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            pump();
            let shown = self.capture();
            let wrong = (0..expected.height)
                .flat_map(|y| (0..expected.width).map(move |x| (x, y)))
                .filter(|&(x, y)| inside(x, y))
                .find(|&(x, y)| {
                    shown[(y * WIDTH + x) as usize]
                        != expected.pixels[(y * expected.width + x) as usize]
                });
            match wrong {
                None => return,
                Some((x, y)) if Instant::now() > deadline => panic!(
                    "the window shows {:06x} at ({x}, {y}) instead of {:06x}",
                    shown[(y * WIDTH + x) as usize],
                    expected.pixels[(y * expected.width + x) as usize],
                ),
                Some(_) => thread::sleep(Duration::from_millis(20)),
            }
        }
    }

    /// The client area as DWM shows it, as `0xRRGGBB` values.
    fn capture(&self) -> Vec<u32> {
        unsafe {
            let dc = Gdi::CreateCompatibleDC(ptr::null_mut());
            let mut info: Gdi::BITMAPINFO = mem::zeroed();
            info.bmiHeader = Gdi::BITMAPINFOHEADER {
                biSize: mem::size_of::<Gdi::BITMAPINFOHEADER>() as u32,
                biWidth: WIDTH as i32,
                biHeight: -(HEIGHT as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: Gdi::BI_RGB,
                ..mem::zeroed()
            };
            let mut bits = ptr::null_mut();
            let bitmap = Gdi::CreateDIBSection(
                dc,
                &info,
                Gdi::DIB_RGB_COLORS,
                &mut bits,
                ptr::null_mut(),
                0,
            );
            assert!(!bitmap.is_null(), "failed to create a bitmap");
            let previous = Gdi::SelectObject(dc, bitmap);
            let printed = PrintWindow(self.0, dc, PW_CLIENTONLY | PW_RENDERFULLCONTENT);
            let pixels = slice::from_raw_parts(bits.cast::<u32>(), (WIDTH * HEIGHT) as usize)
                .iter()
                .map(|pixel| pixel & 0xffffff)
                .collect();
            Gdi::SelectObject(dc, previous);
            Gdi::DeleteObject(bitmap);
            Gdi::DeleteDC(dc);
            assert_ne!(printed, 0, "failed to print the window");
            pixels
        }
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        unsafe { wm::DestroyWindow(self.0) };
    }
}

impl HasWindowHandle for Window {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let handle = Win32WindowHandle::new(NonZeroIsize::new(self.0 as isize).unwrap());
        // SAFETY: The window lives as long as `self`.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(handle)) })
    }
}

impl HasDisplayHandle for Window {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::windows())
    }
}

/// Handles the messages of the windows that this thread created.
fn pump() {
    unsafe {
        let mut message = mem::zeroed();
        while wm::PeekMessageW(&mut message, ptr::null_mut(), 0, 0, wm::PM_REMOVE) != 0 {
            wm::TranslateMessage(&message);
            wm::DispatchMessageW(&message);
        }
    }
}
