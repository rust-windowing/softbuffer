//! Implementation of software buffering for X11.
//!
//! This module converts the input buffer into an XImage and then sends it over the wire to be
//! drawn by the X server. The SHM extension is used if available.

#![allow(clippy::uninlined_format_args)]

use crate::backend_interface::*;
use crate::error::{InitError, SwResultExt};
use crate::{Rect, SoftBufferError};
use raw_window_handle::{
    HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle, XcbDisplayHandle,
    XcbWindowHandle,
};
use rustix::{
    fd::{AsFd, BorrowedFd, OwnedFd},
    mm, shm as posix_shm,
};

use std::{
    collections::HashSet,
    fmt,
    fs::File,
    io, mem,
    num::{NonZeroU16, NonZeroU32},
    ptr::{null_mut, NonNull},
    rc::Rc,
    slice,
};

use as_raw_xcb_connection::AsRawXcbConnection;
use x11rb::connection::{Connection, SequenceNumber};
use x11rb::cookie::Cookie;
use x11rb::errors::{ConnectionError, ReplyError, ReplyOrIdError};
use x11rb::protocol::shm::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _, ImageOrder, VisualClass, Visualid};
use x11rb::xcb_ffi::XCBConnection;

pub struct X11DisplayImpl<D: ?Sized> {
    /// The handle to the XCB connection.
    connection: Option<XCBConnection>,

    /// SHM extension is available.
    is_shm_available: bool,

    /// All visuals using softbuffer's pixel representation
    supported_visuals: HashSet<Visualid>,

    /// The generic display where the `connection` field comes from.
    ///
    /// Without `&mut`, the underlying connection cannot be closed without other unsafe behavior.
    /// With `&mut`, the connection can be dropped without us knowing about it. Therefore, we
    /// cannot provide `&mut` access to this field.
    _display: D,
}

impl<D: HasDisplayHandle + ?Sized> ContextInterface<D> for Rc<X11DisplayImpl<D>> {
    /// Create a new `X11DisplayImpl`.
    fn new(display: D) -> Result<Self, InitError<D>>
    where
        D: Sized,
    {
        // Get the underlying libxcb handle.
        let raw = display.display_handle()?.as_raw();
        let xcb_handle = match raw {
            RawDisplayHandle::Xcb(xcb_handle) => xcb_handle,
            RawDisplayHandle::Xlib(xlib) => {
                // Convert to an XCB handle.
                let connection = xlib.display.map(|display| {
                    // Get the underlying XCB connection.
                    // SAFETY: The user has asserted that the display handle is valid.
                    unsafe {
                        let display = tiny_xlib::Display::from_ptr(display.as_ptr());
                        NonNull::new_unchecked(display.as_raw_xcb_connection()).cast()
                    }
                });

                // Construct the equivalent XCB display and window handles.
                XcbDisplayHandle::new(connection, xlib.screen)
            }
            _ => return Err(InitError::Unsupported(display)),
        };

        // Validate the display handle to ensure we can use it.
        let connection = match xcb_handle.connection {
            Some(connection) => {
                // Wrap the display handle in an x11rb connection.
                // SAFETY: We don't own the connection, so don't drop it. We also assert that the connection is valid.
                let result =
                    unsafe { XCBConnection::from_raw_xcb_connection(connection.as_ptr(), false) };

                result.swbuf_err("Failed to wrap XCB connection")?
            }
            None => {
                // The user didn't provide an XCB connection, so create our own.
                log::info!("no XCB connection provided by the user, so spawning our own");
                XCBConnection::connect(None)
                    .swbuf_err("Failed to spawn XCB connection")?
                    .0
            }
        };

        let is_shm_available = is_shm_available(&connection);
        if !is_shm_available {
            log::warn!("SHM extension is not available. Performance may be poor.");
        }

        let supported_visuals = supported_visuals(&connection);

        Ok(Rc::new(X11DisplayImpl {
            connection: Some(connection),
            is_shm_available,
            supported_visuals,
            _display: display,
        }))
    }
}

impl<D: ?Sized> X11DisplayImpl<D> {
    fn connection(&self) -> &XCBConnection {
        self.connection
            .as_ref()
            .expect("X11DisplayImpl::connection() called after X11DisplayImpl::drop()")
    }
}

/// The handle to an X11 drawing context.
pub struct X11Impl<D: ?Sized, W: ?Sized> {
    /// X display this window belongs to.
    display: Rc<X11DisplayImpl<D>>,

    /// The window to draw to.
    window: xproto::Window,

    /// The graphics context to use when drawing.
    gc: xproto::Gcontext,

    /// The depth (bits per pixel) of the drawing context.
    depth: u8,

    /// The visual ID of the drawing context.
    visual_id: u32,

    /// The buffer we draw to.
    buffer: Buffer,

    /// Buffer has been presented.
    buffer_presented: bool,

    /// The current buffer width/height.
    size: Option<(NonZeroU16, NonZeroU16)>,

    /// Keep the window alive.
    window_handle: W,
}

/// The buffer that is being drawn to.
enum Buffer {
    /// A buffer implemented using shared memory to prevent unnecessary copying.
    Shm(ShmBuffer),

    /// A normal buffer that we send over the wire.
    Wire(Vec<u32>),
}

struct ShmBuffer {
    /// The shared memory segment, paired with its ID.
    seg: Option<(ShmSegment, shm::Seg)>,

    /// A cookie indicating that the shared memory segment is ready to be used.
    ///
    /// We can't soundly read from or write to the SHM segment until the X server is done processing the
    /// `shm::PutImage` request. However, the X server handles requests in order, which means that, if
    /// we send a very small request after the `shm::PutImage` request, then the X server will have to
    /// process that request before it can process the `shm::PutImage` request. Therefore, we can use
    /// the reply to that small request to determine when the `shm::PutImage` request is done.
    ///
    /// In this case, we use `GetInputFocus` since it is a very small request.
    ///
    /// We store the sequence number instead of the `Cookie` since we cannot hold a self-referential
    /// reference to the `connection` field.
    done_processing: Option<SequenceNumber>,
}

impl<D: HasDisplayHandle + ?Sized, W: HasWindowHandle> SurfaceInterface<D, W> for X11Impl<D, W> {
    type Context = Rc<X11DisplayImpl<D>>;
    type Buffer<'a> = BufferImpl<'a, D, W> where Self: 'a;

    /// Create a new `X11Impl` from a `HasWindowHandle`.
    fn new(window_src: W, display: &Rc<X11DisplayImpl<D>>) -> Result<Self, InitError<W>> {
        // Get the underlying raw window handle.
        let raw = window_src.window_handle()?.as_raw();
        let window_handle = match raw {
            RawWindowHandle::Xcb(xcb) => xcb,
            RawWindowHandle::Xlib(xlib) => {
                let window = match NonZeroU32::new(xlib.window as u32) {
                    Some(window) => window,
                    None => return Err(SoftBufferError::IncompleteWindowHandle.into()),
                };
                let mut xcb_window_handle = XcbWindowHandle::new(window);
                xcb_window_handle.visual_id = NonZeroU32::new(xlib.visual_id as u32);
                xcb_window_handle
            }
            _ => {
                return Err(InitError::Unsupported(window_src));
            }
        };

        log::trace!("new: window_handle={:X}", window_handle.window);
        let window = window_handle.window.get();

        // Run in parallel: start getting the window depth and (if necessary) visual.
        let display2 = display.clone();
        let tokens = {
            let geometry_token = display2
                .connection()
                .get_geometry(window)
                .swbuf_err("Failed to send geometry request")?;
            let window_attrs_token = if window_handle.visual_id.is_none() {
                Some(
                    display2
                        .connection()
                        .get_window_attributes(window)
                        .swbuf_err("Failed to send window attributes request")?,
                )
            } else {
                None
            };

            (geometry_token, window_attrs_token)
        };

        // Create a new graphics context to draw to.
        let gc = display
            .connection()
            .generate_id()
            .swbuf_err("Failed to generate GC ID")?;
        display
            .connection()
            .create_gc(
                gc,
                window,
                &xproto::CreateGCAux::new().graphics_exposures(0),
            )
            .swbuf_err("Failed to send GC creation request")?
            .check()
            .swbuf_err("Failed to create GC")?;

        // Finish getting the depth of the window.
        let (geometry_reply, visual_id) = {
            let (geometry_token, window_attrs_token) = tokens;
            let geometry_reply = geometry_token
                .reply()
                .swbuf_err("Failed to get geometry reply")?;
            let visual_id = match window_attrs_token {
                None => window_handle.visual_id.unwrap().get(),
                Some(window_attrs) => {
                    window_attrs
                        .reply()
                        .swbuf_err("Failed to get window attributes reply")?
                        .visual
                }
            };

            (geometry_reply, visual_id)
        };

        if !display.supported_visuals.contains(&visual_id) {
            return Err(SoftBufferError::PlatformError(
                Some(format!(
                    "Visual 0x{visual_id:x} does not use softbuffer's pixel format and is unsupported"
                )),
                None,
            )
            .into());
        }

        // See if SHM is available.
        let buffer = if display.is_shm_available {
            // SHM is available.
            Buffer::Shm(ShmBuffer {
                seg: None,
                done_processing: None,
            })
        } else {
            // SHM is not available.
            Buffer::Wire(Vec::new())
        };

        Ok(Self {
            display: display.clone(),
            window,
            gc,
            depth: geometry_reply.depth,
            visual_id,
            buffer,
            buffer_presented: false,
            size: None,
            window_handle: window_src,
        })
    }

    #[inline]
    fn window(&self) -> &W {
        &self.window_handle
    }

    fn resize(&mut self, width: NonZeroU32, height: NonZeroU32) -> Result<(), SoftBufferError> {
        log::trace!(
            "resize: window={:X}, size={}x{}",
            self.window,
            width,
            height
        );

        // Width and height should fit in u16.
        let width: NonZeroU16 = width
            .try_into()
            .or(Err(SoftBufferError::SizeOutOfRange { width, height }))?;
        let height: NonZeroU16 = height.try_into().or(Err(SoftBufferError::SizeOutOfRange {
            width: width.into(),
            height,
        }))?;

        if self.size != Some((width, height)) {
            self.buffer_presented = false;
            self.buffer
                .resize(self.display.connection(), width.get(), height.get())
                .swbuf_err("Failed to resize X11 buffer")?;

            // We successfully resized the buffer.
            self.size = Some((width, height));
        }

        Ok(())
    }

    fn buffer_mut(&mut self) -> Result<BufferImpl<'_, D, W>, SoftBufferError> {
        log::trace!("buffer_mut: window={:X}", self.window);

        // Finish waiting on the previous `shm::PutImage` request, if any.
        self.buffer.finish_wait(self.display.connection())?;

        // We can now safely call `buffer_mut` on the buffer.
        Ok(BufferImpl(self))
    }

    fn fetch(&mut self) -> Result<Vec<u32>, SoftBufferError> {
        log::trace!("fetch: window={:X}", self.window);

        let (width, height) = self
            .size
            .expect("Must set size of surface before calling `fetch()`");

        // TODO: Is it worth it to do SHM here? Probably not.
        let reply = self
            .display
            .connection()
            .get_image(
                xproto::ImageFormat::Z_PIXMAP,
                self.window,
                0,
                0,
                width.get(),
                height.get(),
                u32::MAX,
            )
            .swbuf_err("Failed to send image fetching request")?
            .reply()
            .swbuf_err("Failed to fetch image from window")?;

        if reply.depth == self.depth && reply.visual == self.visual_id {
            let mut out = vec![0u32; reply.data.len() / 4];
            bytemuck::cast_slice_mut::<u32, u8>(&mut out).copy_from_slice(&reply.data);
            Ok(out)
        } else {
            Err(SoftBufferError::PlatformError(
                Some("Mismatch between reply and window data".into()),
                None,
            ))
        }
    }
}

pub struct BufferImpl<'a, D: ?Sized, W: ?Sized>(&'a mut X11Impl<D, W>);

impl<'a, D: HasDisplayHandle + ?Sized, W: HasWindowHandle + ?Sized> BufferInterface
    for BufferImpl<'a, D, W>
{
    #[inline]
    fn pixels(&self) -> &[u32] {
        // SAFETY: We called `finish_wait` on the buffer, so it is safe to call `buffer()`.
        unsafe { self.0.buffer.buffer() }
    }

    #[inline]
    fn pixels_mut(&mut self) -> &mut [u32] {
        // SAFETY: We called `finish_wait` on the buffer, so it is safe to call `buffer_mut`.
        unsafe { self.0.buffer.buffer_mut() }
    }

    fn age(&self) -> u8 {
        if self.0.buffer_presented {
            1
        } else {
            0
        }
    }

    /// Push the buffer to the window.
    fn present_with_damage(self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        let imp = self.0;

        let (surface_width, surface_height) = imp
            .size
            .expect("Must set size of surface before calling `present_with_damage()`");

        log::trace!("present: window={:X}", imp.window);

        match imp.buffer {
            Buffer::Wire(ref wire) => {
                // This is a suboptimal strategy, raise a stink in the debug logs.
                log::debug!("Falling back to non-SHM method for window drawing.");

                imp.display
                    .connection()
                    .put_image(
                        xproto::ImageFormat::Z_PIXMAP,
                        imp.window,
                        imp.gc,
                        surface_width.get(),
                        surface_height.get(),
                        0,
                        0,
                        0,
                        imp.depth,
                        bytemuck::cast_slice(wire),
                    )
                    .map(|c| c.ignore_error())
                    .push_err()
                    .swbuf_err("Failed to draw image to window")?;
            }

            Buffer::Shm(ref mut shm) => {
                // If the X server is still processing the last image, wait for it to finish.
                // SAFETY: We know that we called finish_wait() before this.
                // Put the image into the window.
                if let Some((_, segment_id)) = shm.seg {
                    damage
                        .iter()
                        .try_for_each(|rect| {
                            let (src_x, src_y, dst_x, dst_y, width, height) = (|| {
                                Some((
                                    u16::try_from(rect.x).ok()?,
                                    u16::try_from(rect.y).ok()?,
                                    i16::try_from(rect.x).ok()?,
                                    i16::try_from(rect.y).ok()?,
                                    u16::try_from(rect.width.get()).ok()?,
                                    u16::try_from(rect.height.get()).ok()?,
                                ))
                            })(
                            )
                            .ok_or(SoftBufferError::DamageOutOfRange { rect: *rect })?;
                            imp.display
                                .connection()
                                .shm_put_image(
                                    imp.window,
                                    imp.gc,
                                    surface_width.get(),
                                    surface_height.get(),
                                    src_x,
                                    src_y,
                                    width,
                                    height,
                                    dst_x,
                                    dst_y,
                                    imp.depth,
                                    xproto::ImageFormat::Z_PIXMAP.into(),
                                    false,
                                    segment_id,
                                    0,
                                )
                                .push_err()
                                .map(|c| c.ignore_error())
                                .swbuf_err("Failed to draw image to window")
                        })
                        .and_then(|()| {
                            // Send a short request to act as a notification for when the X server is done processing the image.
                            shm.begin_wait(imp.display.connection())
                                .swbuf_err("Failed to draw image to window")
                        })?;
                }
            }
        }

        imp.buffer_presented = true;

        Ok(())
    }

    fn present(self) -> Result<(), SoftBufferError> {
        let (width, height) = self
            .0
            .size
            .expect("Must set size of surface before calling `present()`");
        self.present_with_damage(&[Rect {
            x: 0,
            y: 0,
            width: width.into(),
            height: height.into(),
        }])
    }
}

impl Buffer {
    /// Resize the buffer to the given size.
    fn resize(
        &mut self,
        conn: &impl Connection,
        width: u16,
        height: u16,
    ) -> Result<(), PushBufferError> {
        match self {
            Buffer::Shm(ref mut shm) => shm.alloc_segment(conn, total_len(width, height)),
            Buffer::Wire(wire) => {
                wire.resize(total_len(width, height) / 4, 0);
                Ok(())
            }
        }
    }

    /// Finish waiting for an ongoing `shm::PutImage` request, if there is one.
    fn finish_wait(&mut self, conn: &impl Connection) -> Result<(), SoftBufferError> {
        if let Buffer::Shm(ref mut shm) = self {
            shm.finish_wait(conn)
                .swbuf_err("Failed to wait for X11 buffer")?;
        }

        Ok(())
    }

    /// Get a reference to the buffer.
    ///
    /// # Safety
    ///
    /// `finish_wait()` must be called in between `shm::PutImage` requests and this function.
    #[inline]
    unsafe fn buffer(&self) -> &[u32] {
        match self {
            Buffer::Shm(ref shm) => unsafe { shm.as_ref() },
            Buffer::Wire(wire) => wire,
        }
    }

    /// Get a mutable reference to the buffer.
    ///
    /// # Safety
    ///
    /// `finish_wait()` must be called in between `shm::PutImage` requests and this function.
    #[inline]
    unsafe fn buffer_mut(&mut self) -> &mut [u32] {
        match self {
            Buffer::Shm(ref mut shm) => unsafe { shm.as_mut() },
            Buffer::Wire(wire) => wire,
        }
    }
}

impl ShmBuffer {
    /// Allocate a new `ShmSegment` of the given size.
    fn alloc_segment(
        &mut self,
        conn: &impl Connection,
        buffer_size: usize,
    ) -> Result<(), PushBufferError> {
        // Round the size up to the next power of two to prevent frequent reallocations.
        let size = buffer_size.next_power_of_two();

        // Get the size of the segment currently in use.
        let needs_realloc = match self.seg {
            Some((ref seg, _)) => seg.size() < size,
            None => true,
        };

        // Reallocate if necessary.
        if needs_realloc {
            let new_seg = ShmSegment::new(size, buffer_size)?;
            self.associate(conn, new_seg)?;
        } else if let Some((ref mut seg, _)) = self.seg {
            seg.set_buffer_size(buffer_size);
        }

        Ok(())
    }

    /// Get the SHM buffer as a reference.
    ///
    /// # Safety
    ///
    /// `finish_wait()` must be called before this function is.
    #[inline]
    unsafe fn as_ref(&self) -> &[u32] {
        match self.seg.as_ref() {
            Some((seg, _)) => {
                let buffer_size = seg.buffer_size();

                // SAFETY: No other code should be able to access the segment.
                bytemuck::cast_slice(unsafe { &seg.as_ref()[..buffer_size] })
            }
            None => {
                // Nothing has been allocated yet.
                &[]
            }
        }
    }

    /// Get the SHM buffer as a mutable reference.
    ///
    /// # Safety
    ///
    /// `finish_wait()` must be called before this function is.
    #[inline]
    unsafe fn as_mut(&mut self) -> &mut [u32] {
        match self.seg.as_mut() {
            Some((seg, _)) => {
                let buffer_size = seg.buffer_size();

                // SAFETY: No other code should be able to access the segment.
                bytemuck::cast_slice_mut(unsafe { &mut seg.as_mut()[..buffer_size] })
            }
            None => {
                // Nothing has been allocated yet.
                &mut []
            }
        }
    }

    /// Associate an SHM segment with the server.
    fn associate(
        &mut self,
        conn: &impl Connection,
        seg: ShmSegment,
    ) -> Result<(), PushBufferError> {
        // Register the guard.
        let new_id = conn.generate_id()?;
        conn.shm_attach_fd(new_id, seg.as_fd().try_clone_to_owned().unwrap(), true)?
            .ignore_error();

        // Take out the old one and detach it.
        if let Some((old_seg, old_id)) = self.seg.replace((seg, new_id)) {
            // Wait for the old segment to finish processing.
            self.finish_wait(conn)?;

            conn.shm_detach(old_id)?.ignore_error();

            // Drop the old segment.
            drop(old_seg);
        }

        Ok(())
    }

    /// Begin waiting for the SHM processing to finish.
    fn begin_wait(&mut self, c: &impl Connection) -> Result<(), PushBufferError> {
        let cookie = c.get_input_focus()?.sequence_number();
        let old_cookie = self.done_processing.replace(cookie);
        debug_assert!(old_cookie.is_none());
        Ok(())
    }

    /// Wait for the SHM processing to finish.
    fn finish_wait(&mut self, c: &impl Connection) -> Result<(), PushBufferError> {
        if let Some(done_processing) = self.done_processing.take() {
            // Cast to a cookie and wait on it.
            let cookie = Cookie::<_, xproto::GetInputFocusReply>::new(c, done_processing);
            cookie.reply()?;
        }

        Ok(())
    }
}

struct ShmSegment {
    id: File,
    ptr: NonNull<i8>,
    size: usize,
    buffer_size: usize,
}

impl ShmSegment {
    /// Create a new `ShmSegment` with the given size.
    fn new(size: usize, buffer_size: usize) -> io::Result<Self> {
        assert!(size >= buffer_size);

        // Create a shared memory segment.
        let id = File::from(create_shm_id()?);

        // Set its length.
        id.set_len(size as u64)?;

        // Map the shared memory to our file descriptor space.
        let ptr = unsafe {
            let ptr = mm::mmap(
                null_mut(),
                size,
                mm::ProtFlags::READ | mm::ProtFlags::WRITE,
                mm::MapFlags::SHARED,
                &id,
                0,
            )?;

            match NonNull::new(ptr.cast()) {
                Some(ptr) => ptr,
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "unexpected null when mapping SHM segment",
                    ));
                }
            }
        };

        Ok(Self {
            id,
            ptr,
            size,
            buffer_size,
        })
    }

    /// Get this shared memory segment as a reference.
    ///
    /// # Safety
    ///
    /// One must ensure that no other processes are writing to this memory.
    unsafe fn as_ref(&self) -> &[i8] {
        unsafe { slice::from_raw_parts(self.ptr.as_ptr(), self.size) }
    }

    /// Get this shared memory segment as a mutable reference.
    ///
    /// # Safety
    ///
    /// One must ensure that no other processes are reading from or writing to this memory.
    unsafe fn as_mut(&mut self) -> &mut [i8] {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr(), self.size) }
    }

    /// Set the size of the buffer for this shared memory segment.
    fn set_buffer_size(&mut self, buffer_size: usize) {
        assert!(self.size >= buffer_size);
        self.buffer_size = buffer_size
    }

    /// Get the size of the buffer for this shared memory segment.
    fn buffer_size(&self) -> usize {
        self.buffer_size
    }

    /// Get the size of this shared memory segment.
    fn size(&self) -> usize {
        self.size
    }
}

impl AsFd for ShmSegment {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.id.as_fd()
    }
}

impl Drop for ShmSegment {
    fn drop(&mut self) {
        unsafe {
            // Unmap the shared memory segment.
            mm::munmap(self.ptr.as_ptr().cast(), self.size).ok();
        }
    }
}

impl<D: ?Sized> Drop for X11DisplayImpl<D> {
    fn drop(&mut self) {
        // Make sure that the x11rb connection is dropped before its source is.
        self.connection = None;
    }
}

impl<D: ?Sized, W: ?Sized> Drop for X11Impl<D, W> {
    fn drop(&mut self) {
        // If we used SHM, make sure it's detached from the server.
        if let Buffer::Shm(mut shm) = mem::replace(&mut self.buffer, Buffer::Wire(Vec::new())) {
            // If we were in the middle of processing a buffer, wait for it to finish.
            shm.finish_wait(self.display.connection()).ok();

            if let Some((segment, seg_id)) = shm.seg.take() {
                if let Ok(token) = self.display.connection().shm_detach(seg_id) {
                    token.ignore_error();
                }

                // Drop the segment.
                drop(segment);
            }
        }

        // Close the graphics context that we created.
        if let Ok(token) = self.display.connection().free_gc(self.gc) {
            token.ignore_error();
        }
    }
}

/// Create a shared memory identifier.
fn create_shm_id() -> io::Result<OwnedFd> {
    use posix_shm::{Mode, ShmOFlags};

    let mut rng = fastrand::Rng::new();
    let mut name = String::with_capacity(23);

    // Only try four times; the chances of a collision on this space is astronomically low, so if
    // we miss four times in a row we're probably under attack.
    for i in 0..4 {
        name.clear();
        name.push_str("softbuffer-x11-");
        name.extend(std::iter::repeat_with(|| rng.alphanumeric()).take(7));

        // Try to create the shared memory segment.
        match posix_shm::shm_open(
            &name,
            ShmOFlags::RDWR | ShmOFlags::CREATE | ShmOFlags::EXCL,
            Mode::RWXU,
        ) {
            Ok(id) => {
                posix_shm::shm_unlink(&name).ok();
                return Ok(id);
            }

            Err(rustix::io::Errno::EXIST) => {
                log::warn!("x11: SHM ID collision at {} on try number {}", name, i);
            }

            Err(e) => return Err(e.into()),
        };
    }

    Err(io::Error::new(
        io::ErrorKind::Other,
        "failed to generate a non-existent SHM name",
    ))
}

/// Test to see if SHM is available.
fn is_shm_available(c: &impl Connection) -> bool {
    // Create a small SHM segment.
    let seg = match ShmSegment::new(0x1000, 0x1000) {
        Ok(seg) => seg,
        Err(_) => return false,
    };

    // Attach and detach it.
    let seg_id = match c.generate_id() {
        Ok(id) => id,
        Err(_) => return false,
    };

    let (attach, detach) = {
        let attach = c.shm_attach_fd(seg_id, seg.as_fd().try_clone_to_owned().unwrap(), false);
        let detach = c.shm_detach(seg_id);

        match (attach, detach) {
            (Ok(attach), Ok(detach)) => (attach, detach),
            _ => return false,
        }
    };

    // Check the replies.
    matches!((attach.check(), detach.check()), (Ok(()), Ok(())))
}

/// Collect all visuals that use softbuffer's pixel format
fn supported_visuals(c: &impl Connection) -> HashSet<Visualid> {
    // Check that depth 24 uses 32 bits per pixels
    if !c
        .setup()
        .pixmap_formats
        .iter()
        .any(|f| f.depth == 24 && f.bits_per_pixel == 32)
    {
        log::warn!("X11 server does not have a depth 24 format with 32 bits per pixel");
        return HashSet::new();
    }

    // How does the server represent red, green, blue components of a pixel?
    #[cfg(target_endian = "little")]
    let own_byte_order = ImageOrder::LSB_FIRST;
    #[cfg(target_endian = "big")]
    let own_byte_order = ImageOrder::MSB_FIRST;
    let expected_masks = if c.setup().image_byte_order == own_byte_order {
        (0xff0000, 0xff00, 0xff)
    } else {
        // This is the byte-swapped version of our wished-for format
        (0xff00, 0xff0000, 0xff000000)
    };

    c.setup()
        .roots
        .iter()
        .flat_map(|screen| {
            screen
                .allowed_depths
                .iter()
                .filter(|depth| depth.depth == 24)
                .flat_map(|depth| {
                    depth
                        .visuals
                        .iter()
                        .filter(|visual| {
                            // Ignore grayscale or indexes / color palette visuals
                            visual.class == VisualClass::TRUE_COLOR
                                || visual.class == VisualClass::DIRECT_COLOR
                        })
                        .filter(|visual| {
                            // Colors must be laid out as softbuffer expects
                            expected_masks == (visual.red_mask, visual.green_mask, visual.blue_mask)
                        })
                        .map(|visual| visual.visual_id)
                })
        })
        .collect()
}

/// An error that can occur when pushing a buffer to the window.
#[derive(Debug)]
enum PushBufferError {
    /// We encountered an X11 error.
    X11(ReplyError),

    /// We exhausted the XID space.
    XidExhausted,

    /// A system error occurred while creating the shared memory segment.
    System(io::Error),
}

impl fmt::Display for PushBufferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::X11(e) => write!(f, "X11 error: {}", e),
            Self::XidExhausted => write!(f, "XID space exhausted"),
            Self::System(e) => write!(f, "System error: {}", e),
        }
    }
}

impl std::error::Error for PushBufferError {}

impl From<ConnectionError> for PushBufferError {
    fn from(e: ConnectionError) -> Self {
        Self::X11(ReplyError::ConnectionError(e))
    }
}

impl From<ReplyError> for PushBufferError {
    fn from(e: ReplyError) -> Self {
        Self::X11(e)
    }
}

impl From<ReplyOrIdError> for PushBufferError {
    fn from(e: ReplyOrIdError) -> Self {
        match e {
            ReplyOrIdError::ConnectionError(e) => Self::X11(ReplyError::ConnectionError(e)),
            ReplyOrIdError::X11Error(e) => Self::X11(ReplyError::X11Error(e)),
            ReplyOrIdError::IdsExhausted => Self::XidExhausted,
        }
    }
}

impl From<io::Error> for PushBufferError {
    fn from(e: io::Error) -> Self {
        Self::System(e)
    }
}

/// Convenient wrapper to cast errors into PushBufferError.
trait PushResultExt<T, E> {
    fn push_err(self) -> Result<T, PushBufferError>;
}

impl<T, E: Into<PushBufferError>> PushResultExt<T, E> for Result<T, E> {
    fn push_err(self) -> Result<T, PushBufferError> {
        self.map_err(Into::into)
    }
}

/// Get the length that a slice needs to be to hold a buffer of the given dimensions.
#[inline(always)]
fn total_len(width: u16, height: u16) -> usize {
    let width: usize = width.into();
    let height: usize = height.into();

    width
        .checked_mul(height)
        .and_then(|len| len.checked_mul(4))
        .unwrap_or_else(|| panic!("Dimensions are too large: ({} x {})", width, height))
}
