use std::{
    ffi::CStr,
    fs::File,
    os::unix::prelude::{AsRawFd, FileExt, FromRawFd},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use wayland_client::{
    protocol::{wl_buffer, wl_shm, wl_shm_pool, wl_surface},
    Connection, Dispatch, QueueHandle,
};

use super::State;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn create_memfile() -> File {
    use nix::{
        fcntl::{fcntl, FcntlArg, SealFlag},
        sys::memfd::{memfd_create, MemFdCreateFlag},
    };

    let name = unsafe { CStr::from_bytes_with_nul_unchecked("softbuffer\0".as_bytes()) };
    let fd = memfd_create(
        name,
        MemFdCreateFlag::MFD_CLOEXEC | MemFdCreateFlag::MFD_ALLOW_SEALING,
    )
    .expect("Failed to create memfd to store buffer.");
    let _ = fcntl(
        fd,
        FcntlArg::F_ADD_SEALS(SealFlag::F_SEAL_SHRINK | SealFlag::F_SEAL_SEAL),
    )
    .expect("Failed to seal memfd.");
    unsafe { File::from_raw_fd(fd) }
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn create_memfile() -> File {
    use nix::{
        errno::Errno,
        fcntl::OFlag,
        sys::{
            mman::{shm_open, shm_unlink},
            stat::Mode,
        },
    };
    use std::iter;

    for _ in 0..=4 {
        let mut name = String::from("softbuffer-");
        name.extend(iter::repeat_with(fastrand::alphanumeric).take(7));
        name.push('\0');

        let name = unsafe { CStr::from_bytes_with_nul_unchecked(name.as_bytes()) };
        // `CLOEXEC` is implied with `shm_open`
        let fd = shm_open(
            name,
            OFlag::O_RDWR | OFlag::O_CREAT | OFlag::O_EXCL,
            Mode::S_IRWXU,
        );
        if fd != Err(Errno::EEXIST) {
            let fd = fd.expect("Failed to create POSIX shm to store buffer.");
            let _ = shm_unlink(name);
            return unsafe { File::from_raw_fd(fd) };
        }
    }

    panic!("Failed to generate non-existant shm name")
}

pub(super) struct WaylandBuffer {
    qh: QueueHandle<State>,
    tempfile: File,
    pool: wl_shm_pool::WlShmPool,
    pool_size: i32,
    buffer: wl_buffer::WlBuffer,
    width: i32,
    height: i32,
    released: Arc<AtomicBool>,
}

impl WaylandBuffer {
    pub fn new(shm: &wl_shm::WlShm, width: i32, height: i32, qh: &QueueHandle<State>) -> Self {
        let tempfile = create_memfile();
        let pool_size = width * height * 4;
        let pool = shm.create_pool(tempfile.as_raw_fd(), pool_size, qh, ());
        let released = Arc::new(AtomicBool::new(true));
        let buffer = pool.create_buffer(
            0,
            width,
            height,
            width * 4,
            wl_shm::Format::Xrgb8888,
            qh,
            released.clone(),
        );
        Self {
            qh: qh.clone(),
            tempfile,
            pool,
            pool_size,
            buffer,
            width,
            height,
            released,
        }
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        // If size is the same, there's nothing to do
        if self.width != width || self.height != height {
            // Destroy old buffer
            self.buffer.destroy();

            // Grow pool, if needed
            let size = ((width * height * 4) as u32).next_power_of_two() as i32;
            if size > self.pool_size {
                let _ = self.tempfile.set_len(size as u64);
                self.pool.resize(size);
                self.pool_size = size;
            }

            // Create buffer with correct size
            self.buffer = self.pool.create_buffer(
                0,
                width,
                height,
                width * 4,
                wl_shm::Format::Xrgb8888,
                &self.qh,
                self.released.clone(),
            );
            self.width = width;
            self.height = height;
        }
    }

    pub fn write(&self, buffer: &[u32]) {
        let buffer =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr() as *const u8, buffer.len() * 4) };
        self.tempfile
            .write_all_at(buffer, 0)
            .expect("Failed to write buffer to temporary file.");
    }

    pub fn attach(&self, surface: &wl_surface::WlSurface) {
        self.released.store(false, Ordering::SeqCst);
        surface.attach(Some(&self.buffer), 0, 0);
    }

    pub fn released(&self) -> bool {
        self.released.load(Ordering::SeqCst)
    }
}

impl Drop for WaylandBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
        self.pool.destroy();
    }
}

impl Dispatch<wl_shm_pool::WlShmPool, ()> for State {
    fn event(
        _: &mut State,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
    }
}

impl Dispatch<wl_buffer::WlBuffer, Arc<AtomicBool>> for State {
    fn event(
        _: &mut State,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        released: &Arc<AtomicBool>,
        _: &Connection,
        _: &QueueHandle<State>,
    ) {
        if let wl_buffer::Event::Release = event {
            released.store(true, Ordering::SeqCst);
        }
    }
}
