use memmap2::MmapMut;
use std::{
    ffi::CStr,
    fs::File,
    os::unix::prelude::{AsRawFd, FromRawFd},
    slice,
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

// Round size to use for pool for given dimentions, rounding up to power of 2
fn get_pool_size(width: i32, height: i32) -> i32 {
    ((width * height * 4) as u32).next_power_of_two() as i32
}

unsafe fn map_file(file: &File) -> MmapMut {
    unsafe { MmapMut::map_mut(file.as_raw_fd()).expect("Failed to map shared memory") }
}

pub(super) struct WaylandBuffer {
    qh: QueueHandle<State>,
    tempfile: File,
    map: MmapMut,
    pool: wl_shm_pool::WlShmPool,
    pool_size: i32,
    buffer: wl_buffer::WlBuffer,
    width: i32,
    height: i32,
    released: Arc<AtomicBool>,
    pub age: u8,
}

impl WaylandBuffer {
    pub fn new(shm: &wl_shm::WlShm, width: i32, height: i32, qh: &QueueHandle<State>) -> Self {
        // Calculate size to use for shm pool
        let pool_size = get_pool_size(width, height);

        // Create an `mmap` shared memory
        let tempfile = create_memfile();
        let _ = tempfile.set_len(pool_size as u64);
        let map = unsafe { map_file(&tempfile) };

        // Create wayland shm pool and buffer
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
            map,
            tempfile,
            pool,
            pool_size,
            buffer,
            width,
            height,
            released,
            age: 0,
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
                self.map = unsafe { map_file(&self.tempfile) };
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

    pub fn attach(&self, surface: &wl_surface::WlSurface) {
        self.released.store(false, Ordering::SeqCst);
        surface.attach(Some(&self.buffer), 0, 0);
    }

    pub fn released(&self) -> bool {
        self.released.load(Ordering::SeqCst)
    }

    fn len(&self) -> usize {
        self.width as usize * self.height as usize
    }

    pub unsafe fn mapped_mut(&mut self) -> &mut [u32] {
        unsafe { slice::from_raw_parts_mut(self.map.as_mut_ptr() as *mut u32, self.len()) }
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
