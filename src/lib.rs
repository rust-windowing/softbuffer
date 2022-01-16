#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "windows")]
mod win32;

use raw_window_handle::{HasRawWindowHandle, RawWindowHandle};

pub struct GraphicsContext<W: HasRawWindowHandle>{
    window: W,
    graphics_context_impl: Box<dyn GraphicsContextImpl>
}

impl<W: HasRawWindowHandle> GraphicsContext<W> {

    pub unsafe fn new(window: W) -> Self{
        let raw_handle = window.raw_window_handle();
        let imple = match raw_handle{
            #[cfg(target_os = "linux")]
            RawWindowHandle::Xlib(xlib_handle) => Box::new(x11::X11Impl::new(xlib_handle)),
            #[cfg(target_os = "windows")]
            RawWindowHandle::Win32(win32_handle) => Box::new(win32::Win32Impl::new(&win32_handle)),
            unimplemented_handle_type => unimplemented!("Unsupported window handle type: {}.", window_handle_type_name(&unimplemented_handle_type))
        };

        Self{
            window,
            graphics_context_impl: imple
        }
    }

    #[inline]
    pub fn window(&self) -> &W{
        &self.window
    }

    #[inline]
    pub fn window_mut(&mut self) -> &mut W{
        &mut self.window
    }

    #[inline]
    pub fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16){
        if (width as usize)*(height as usize) != buffer.len(){
            panic!("The size of the passed buffer is not the correct size. Its length must be exactly width*height.");
        }

        unsafe {
            self.graphics_context_impl.set_buffer(buffer, width, height);
        }
    }

}

trait GraphicsContextImpl{
    unsafe fn set_buffer(&mut self, buffer: &[u32], width: u16, height: u16);
}

fn window_handle_type_name(handle: &RawWindowHandle) -> &'static str{
    match handle{
        RawWindowHandle::Xlib(_) => "Xlib",
        RawWindowHandle::Win32(_) => "Win32",
        RawWindowHandle::WinRt(_) => "WinRt",
        RawWindowHandle::Web(_) => "Web",
        RawWindowHandle::Wayland(_) => "Wayland",
        RawWindowHandle::AndroidNdk(_) => "AndroidNdk",
        RawWindowHandle::AppKit(_) => "AppKit",
        RawWindowHandle::Orbital(_) => "Orbital",
        RawWindowHandle::UiKit(_) => "UiKit",
        _ => "Unknown Name" //don't completely fail to compile if there is a new raw window handle type that's added at some point
    }
}