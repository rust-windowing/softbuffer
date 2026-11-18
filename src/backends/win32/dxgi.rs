//! Presentation through a DXGI flip model swap chain.
//!
//! GDI blits into the surface that DWM composes from, and nothing marks where a frame ends, so a
//! composition during a blit shows the top of one frame over the bottom of the previous one. A
//! flip model swap chain hands DWM whole frames instead.
//!
//! The buffer is a staging texture of a WARP device, which renders on the CPU and needs no graphics
//! driver. Presenting copies the damage into the swap chain's back buffer, and DXGI copies the rest
//! of the previous frame.

use std::ffi::c_void;
use std::fmt;
use std::io;
use std::marker::PhantomData;
use std::mem;
use std::num::NonZeroU32;
use std::ptr::{self, NonNull};
use std::slice;
use std::sync::OnceLock;

use windows_sys::core::HRESULT;
use windows_sys::Win32::Foundation::{HWND, RECT};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};

use self::ffi::*;
use crate::{Pixel, Rect, SoftBufferError};

/// A window's swap chain, with the device that copies frames into it.
#[derive(Debug)]
pub struct Presenter {
    window: HWND,
    /// Made by `configure`.
    frame: Option<Frame>,
    /// Made by the first `configure`.
    swap_chain: Option<Com<IDXGISwapChain1Vtbl>>,
    factory: Com<IDXGIFactory2Vtbl>,
    context: Com<ID3D11DeviceContextVtbl>,
    device: Com<ID3D11DeviceVtbl>,
    /// A frame was presented, after which GDI no longer draws into the window.
    presented: bool,
}

// SAFETY: Direct3D 11 and DXGI objects aren't tied to a thread. The device context must not be
// used by two threads at once, which can't happen, as the surface isn't `Sync`.
unsafe impl Send for Presenter {}

#[derive(Debug)]
struct Frame {
    /// The texture that holds the frame. The CPU draws into it while it's mapped.
    staging: Com<IUnknownVtbl>,
    width: NonZeroU32,
    height: NonZeroU32,
    mapping: Option<Mapping>,
    /// The texture holds the frame that was presented last.
    presented: bool,
    /// The swap chain's buffers may not hold the last frame, so the next present copies all of it.
    present_whole_frame: bool,
}

#[derive(Debug)]
struct Mapping {
    pixels: NonNull<Pixel>,
    byte_stride: NonZeroU32,
}

impl Presenter {
    /// Creates a WARP device for presenting to `window`.
    pub fn new(window: HWND) -> Result<Self, SoftBufferError> {
        let create_device = d3d11_create_device().ok_or_else(|| {
            SoftBufferError::PlatformError(Some("Direct3D 11 is not available".into()), None)
        })?;
        let mut device = ptr::null_mut();
        let mut context = ptr::null_mut();
        let result = unsafe {
            create_device(
                ptr::null_mut(),
                D3D_DRIVER_TYPE_WARP,
                ptr::null_mut(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                ptr::null(),
                0,
                D3D11_SDK_VERSION,
                &mut device,
                ptr::null_mut(),
                &mut context,
            )
        };
        check(result, "failed to create a WARP device")?;
        let device = unsafe { Com::<ID3D11DeviceVtbl>::new(device) }?;
        let context = unsafe { Com::<ID3D11DeviceContextVtbl>::new(context) }?;

        // Swap chains must come from the factory that made the device's adapter.
        let mut dxgi_device = ptr::null_mut();
        let result = unsafe {
            (device.vtable().base.QueryInterface)(
                device.as_ptr(),
                &IID_IDXGI_DEVICE,
                &mut dxgi_device,
            )
        };
        check(result, "failed to get the DXGI device")?;
        let dxgi_device = unsafe { Com::<IDXGIDeviceVtbl>::new(dxgi_device) }?;
        let mut adapter = ptr::null_mut();
        let result =
            unsafe { (dxgi_device.vtable().GetAdapter)(dxgi_device.as_ptr(), &mut adapter) };
        check(result, "failed to get the device's adapter")?;
        let adapter = unsafe { Com::<IDXGIObjectVtbl>::new(adapter) }?;
        let mut factory = ptr::null_mut();
        let result = unsafe {
            (adapter.vtable().GetParent)(adapter.as_ptr(), &IID_IDXGI_FACTORY2, &mut factory)
        };
        check(result, "failed to get the adapter's factory")?;
        let factory = unsafe { Com::<IDXGIFactory2Vtbl>::new(factory) }?;

        Ok(Self {
            window,
            frame: None,
            swap_chain: None,
            factory,
            context,
            device,
            presented: false,
        })
    }

    /// Whether a frame was presented, after which GDI no longer draws into the window.
    pub fn presented(&self) -> bool {
        self.presented
    }

    /// Sizes the frame and the swap chain, which the first call creates.
    pub fn configure(
        &mut self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<(), SoftBufferError> {
        if let Some(frame) = &self.frame {
            if frame.width == width && frame.height == height {
                return Ok(());
            }
        }

        let staging = self.create_staging(width, height)?;
        match &self.swap_chain {
            Some(swap_chain) => {
                let result = unsafe {
                    (swap_chain.vtable().ResizeBuffers)(
                        swap_chain.as_ptr(),
                        0,
                        width.get(),
                        height.get(),
                        DXGI_FORMAT_UNKNOWN,
                        0,
                    )
                };
                check(result, "failed to resize the swap chain")?;
            }
            None => self.swap_chain = Some(self.create_swap_chain(width, height)?),
        }

        self.unmap();
        self.frame = Some(Frame {
            staging,
            width,
            height,
            mapping: None,
            presented: false,
            present_whole_frame: true,
        });
        Ok(())
    }

    fn create_staging(
        &self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<Com<IUnknownVtbl>, SoftBufferError> {
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width.get(),
            Height: height.get(),
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ | D3D11_CPU_ACCESS_WRITE,
            MiscFlags: 0,
        };
        let mut texture = ptr::null_mut();
        let result = unsafe {
            (self.device.vtable().CreateTexture2D)(
                self.device.as_ptr(),
                &desc,
                ptr::null(),
                &mut texture,
            )
        };
        check(result, "failed to create a staging texture")?;
        unsafe { Com::new(texture) }
    }

    fn create_swap_chain(
        &self,
        width: NonZeroU32,
        height: NonZeroU32,
    ) -> Result<Com<IDXGISwapChain1Vtbl>, SoftBufferError> {
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: width.get(),
            Height: height.get(),
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: 0,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            // Like GDI, draw a frame of another size than the window in its top left corner.
            Scaling: DXGI_SCALING_NONE,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Flags: 0,
        };
        let mut swap_chain = ptr::null_mut();
        let result = unsafe {
            (self.factory.vtable().CreateSwapChainForHwnd)(
                self.factory.as_ptr(),
                self.device.as_ptr(),
                self.window,
                &desc,
                ptr::null(),
                ptr::null_mut(),
                &mut swap_chain,
            )
        };
        check(result, "failed to create a swap chain for the window")?;
        let swap_chain = unsafe { Com::new(swap_chain) }?;

        // Keep DXGI from watching the window's messages and turning Alt+Enter into full screen.
        let result = unsafe {
            (self.factory.vtable().MakeWindowAssociation)(
                self.factory.as_ptr(),
                self.window,
                DXGI_MWA_NO_WINDOW_CHANGES | DXGI_MWA_NO_ALT_ENTER,
            )
        };
        check(result, "failed to associate the window")?;
        Ok(swap_chain)
    }

    /// Maps the frame for the CPU to draw into.
    pub fn map(&mut self) -> Result<(), SoftBufferError> {
        let frame = self
            .frame
            .as_mut()
            .expect("Must set size of surface before calling `next_buffer()`");
        if frame.mapping.is_some() {
            return Ok(());
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE {
            pData: ptr::null_mut(),
            RowPitch: 0,
            DepthPitch: 0,
        };
        let result = unsafe {
            (self.context.vtable().Map)(
                self.context.as_ptr(),
                frame.staging.as_ptr(),
                0,
                D3D11_MAP_READ_WRITE,
                0,
                &mut mapped,
            )
        };
        check(result, "failed to map the staging texture")?;

        // A 2D texture's depth pitch spans all of its rows.
        let byte_stride = mapped.RowPitch as usize;
        let fits = byte_stride >= frame.width.get() as usize * 4
            && byte_stride % 4 == 0
            && mapped.DepthPitch as usize >= byte_stride * frame.height.get() as usize
            && mapped.pData as usize % mem::align_of::<Pixel>() == 0;
        match (
            NonNull::new(mapped.pData.cast()),
            NonZeroU32::new(mapped.RowPitch),
        ) {
            (Some(pixels), Some(byte_stride)) if fits => {
                frame.mapping = Some(Mapping {
                    pixels,
                    byte_stride,
                });
                Ok(())
            }
            _ => {
                unsafe {
                    (self.context.vtable().Unmap)(self.context.as_ptr(), frame.staging.as_ptr(), 0)
                };
                Err(SoftBufferError::PlatformError(
                    Some("the staging texture was mapped with an unexpected layout".into()),
                    None,
                ))
            }
        }
    }

    fn unmap(&mut self) {
        if let Some(frame) = &mut self.frame {
            if frame.mapping.take().is_some() {
                unsafe {
                    (self.context.vtable().Unmap)(self.context.as_ptr(), frame.staging.as_ptr(), 0)
                };
            }
        }
    }

    fn mapped(&self) -> (&Frame, &Mapping) {
        let frame = self.frame.as_ref().unwrap();
        (frame, frame.mapping.as_ref().unwrap())
    }

    pub fn byte_stride(&self) -> NonZeroU32 {
        self.mapped().1.byte_stride
    }

    pub fn width(&self) -> NonZeroU32 {
        self.mapped().0.width
    }

    pub fn height(&self) -> NonZeroU32 {
        self.mapped().0.height
    }

    pub fn pixels_mut(&mut self) -> &mut [Pixel] {
        let (frame, mapping) = self.mapped();
        let len = mapping.byte_stride.get() as usize / 4 * frame.height.get() as usize;
        // SAFETY: The mapping holds this many pixels, checked in `map`. It stays valid until the
        // texture is unmapped, which needs `&mut self`.
        unsafe { slice::from_raw_parts_mut(mapping.pixels.as_ptr(), len) }
    }

    pub fn age(&self) -> u8 {
        u8::from(self.mapped().0.presented)
    }

    /// Copies the damage into the swap chain and presents it.
    pub fn present(&mut self, damage: &[Rect]) -> Result<(), SoftBufferError> {
        self.unmap();
        let frame = self.frame.as_mut().unwrap();
        let swap_chain = self.swap_chain.as_ref().unwrap();
        let (width, height) = (frame.width.get(), frame.height.get());

        let mut dirty: Vec<RECT> = if frame.present_whole_frame {
            vec![RECT {
                left: 0,
                top: 0,
                right: width as i32,
                bottom: height as i32,
            }]
        } else {
            damage
                .iter()
                .filter_map(|rect| clip(rect, width, height))
                .collect()
        };
        if dirty.is_empty() {
            return Ok(());
        }

        let mut back_buffer = ptr::null_mut();
        let result = unsafe {
            (swap_chain.vtable().GetBuffer)(
                swap_chain.as_ptr(),
                0,
                &IID_ID3D11_TEXTURE2D,
                &mut back_buffer,
            )
        };
        check(result, "failed to get the swap chain's back buffer")?;
        let back_buffer = unsafe { Com::<IUnknownVtbl>::new(back_buffer) }?;
        for rect in &dirty {
            let source = D3D11_BOX {
                left: rect.left as u32,
                top: rect.top as u32,
                front: 0,
                right: rect.right as u32,
                bottom: rect.bottom as u32,
                back: 1,
            };
            unsafe {
                (self.context.vtable().CopySubresourceRegion)(
                    self.context.as_ptr(),
                    back_buffer.as_ptr(),
                    0,
                    source.left,
                    source.top,
                    0,
                    frame.staging.as_ptr(),
                    0,
                    &source,
                )
            };
        }
        drop(back_buffer);

        // DXGI copies what the previous frame changed outside of the dirty rectangles, so the
        // back buffer ends up with the whole frame. A whole frame needs no rectangles.
        let parameters = DXGI_PRESENT_PARAMETERS {
            DirtyRectsCount: if frame.present_whole_frame {
                0
            } else {
                dirty.len() as u32
            },
            pDirtyRects: if frame.present_whole_frame {
                ptr::null_mut()
            } else {
                dirty.as_mut_ptr()
            },
            pScrollRect: ptr::null_mut(),
            pScrollOffset: ptr::null_mut(),
        };
        let result =
            unsafe { (swap_chain.vtable().Present1)(swap_chain.as_ptr(), 0, 0, &parameters) };
        // `DXGI_STATUS_OCCLUDED` means that the window isn't visible, not that the present failed.
        check(result, "failed to present")?;
        frame.present_whole_frame = false;
        frame.presented = true;
        self.presented = true;
        Ok(())
    }
}

impl Drop for Presenter {
    fn drop(&mut self) {
        self.unmap();
    }
}

/// The part of `rect` inside the frame, if any.
fn clip(rect: &Rect, width: u32, height: u32) -> Option<RECT> {
    let left = rect.x.min(width);
    let top = rect.y.min(height);
    let right = rect.x.saturating_add(rect.width.get()).min(width);
    let bottom = rect.y.saturating_add(rect.height.get()).min(height);
    // Direct3D textures are at most 16384 pixels wide, so these fit in an `i32`.
    (left < right && top < bottom).then_some(RECT {
        left: left as i32,
        top: top as i32,
        right: right as i32,
        bottom: bottom as i32,
    })
}

fn check(result: HRESULT, message: &str) -> Result<(), SoftBufferError> {
    if result < 0 {
        Err(SoftBufferError::PlatformError(
            Some(message.into()),
            Some(Box::new(io::Error::from_raw_os_error(result))),
        ))
    } else {
        Ok(())
    }
}

/// `D3D11CreateDevice`, loaded at run time so that a system without `d3d11.dll` still runs.
fn d3d11_create_device() -> Option<D3D11CreateDevice> {
    static FUNCTION: OnceLock<Option<D3D11CreateDevice>> = OnceLock::new();
    *FUNCTION.get_or_init(|| {
        // The library stays loaded, as the objects it creates run its code.
        let module = unsafe {
            LoadLibraryExW(
                windows_sys::w!("d3d11.dll"),
                ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        if module.is_null() {
            return None;
        }
        let function = unsafe { GetProcAddress(module, b"D3D11CreateDevice\0".as_ptr()) }?;
        // SAFETY: This is the function's signature.
        Some(unsafe {
            mem::transmute::<unsafe extern "system" fn() -> isize, D3D11CreateDevice>(function)
        })
    })
}

/// An owned COM interface pointer, whose methods are the vtable `V`.
struct Com<V>(NonNull<c_void>, PhantomData<V>);

impl<V> Com<V> {
    /// Takes ownership of an interface pointer that a call returned.
    ///
    /// # Safety
    ///
    /// The pointer must be null, or point to an interface whose vtable starts with `V`, with a
    /// reference that the caller owns.
    unsafe fn new(pointer: *mut c_void) -> Result<Self, SoftBufferError> {
        match NonNull::new(pointer) {
            Some(pointer) => Ok(Self(pointer, PhantomData)),
            None => Err(SoftBufferError::PlatformError(
                Some("a call returned no interface".into()),
                None,
            )),
        }
    }

    fn as_ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    fn vtable(&self) -> &V {
        // SAFETY: An interface pointer points to a pointer to its vtable, which starts with `V`.
        unsafe { &**self.as_ptr().cast::<*const V>() }
    }
}

impl<V> Drop for Com<V> {
    fn drop(&mut self) {
        // SAFETY: Every vtable starts with the methods of `IUnknown`.
        unsafe {
            let vtable = &**self.as_ptr().cast::<*const IUnknownVtbl>();
            (vtable.Release)(self.as_ptr());
        }
    }
}

impl<V> fmt::Debug for Com<V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Com").field(&self.0).finish()
    }
}

/// The Direct3D 11 and DXGI definitions used here, from the Windows SDK's `d3d11.h`, `dxgi.h`,
/// `dxgi1_2.h` and `dxgiformat.h`. `windows-sys` doesn't include them.
#[allow(non_camel_case_types, non_snake_case)]
mod ffi {
    use std::ffi::c_void;

    use windows_sys::core::{GUID, HRESULT};
    use windows_sys::Win32::Foundation::{HMODULE, HWND, POINT, RECT};

    pub type D3D11CreateDevice = unsafe extern "system" fn(
        adapter: *mut c_void,
        driver_type: i32,
        software: HMODULE,
        flags: u32,
        feature_levels: *const i32,
        feature_level_count: u32,
        sdk_version: u32,
        device: *mut *mut c_void,
        feature_level: *mut i32,
        immediate_context: *mut *mut c_void,
    ) -> HRESULT;

    pub const D3D_DRIVER_TYPE_WARP: i32 = 5;
    pub const D3D11_SDK_VERSION: u32 = 7;
    pub const D3D11_CREATE_DEVICE_BGRA_SUPPORT: u32 = 0x20;
    pub const D3D11_USAGE_STAGING: i32 = 3;
    pub const D3D11_CPU_ACCESS_WRITE: u32 = 0x10000;
    pub const D3D11_CPU_ACCESS_READ: u32 = 0x20000;
    pub const D3D11_MAP_READ_WRITE: i32 = 3;
    pub const DXGI_FORMAT_UNKNOWN: i32 = 0;
    pub const DXGI_FORMAT_B8G8R8A8_UNORM: i32 = 87;
    pub const DXGI_USAGE_RENDER_TARGET_OUTPUT: u32 = 0x20;
    pub const DXGI_SCALING_NONE: i32 = 1;
    pub const DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL: i32 = 3;
    pub const DXGI_ALPHA_MODE_IGNORE: i32 = 3;
    pub const DXGI_MWA_NO_WINDOW_CHANGES: u32 = 1;
    pub const DXGI_MWA_NO_ALT_ENTER: u32 = 2;

    pub const IID_IDXGI_DEVICE: GUID = GUID::from_u128(0x54ec77fa_1377_44e6_8c32_88fd5f44c84c);
    pub const IID_IDXGI_FACTORY2: GUID = GUID::from_u128(0x50c83a1c_e072_4c48_87b0_3630fa36a6d0);
    pub const IID_ID3D11_TEXTURE2D: GUID = GUID::from_u128(0x6f15aaf2_d208_4e89_9ab4_489535d34f9c);

    #[repr(C)]
    pub struct DXGI_SAMPLE_DESC {
        pub Count: u32,
        pub Quality: u32,
    }

    #[repr(C)]
    pub struct D3D11_TEXTURE2D_DESC {
        pub Width: u32,
        pub Height: u32,
        pub MipLevels: u32,
        pub ArraySize: u32,
        pub Format: i32,
        pub SampleDesc: DXGI_SAMPLE_DESC,
        pub Usage: i32,
        pub BindFlags: u32,
        pub CPUAccessFlags: u32,
        pub MiscFlags: u32,
    }

    #[repr(C)]
    pub struct D3D11_MAPPED_SUBRESOURCE {
        pub pData: *mut c_void,
        pub RowPitch: u32,
        pub DepthPitch: u32,
    }

    #[repr(C)]
    pub struct D3D11_BOX {
        pub left: u32,
        pub top: u32,
        pub front: u32,
        pub right: u32,
        pub bottom: u32,
        pub back: u32,
    }

    #[repr(C)]
    pub struct DXGI_SWAP_CHAIN_DESC1 {
        pub Width: u32,
        pub Height: u32,
        pub Format: i32,
        pub Stereo: i32,
        pub SampleDesc: DXGI_SAMPLE_DESC,
        pub BufferUsage: u32,
        pub BufferCount: u32,
        pub Scaling: i32,
        pub SwapEffect: i32,
        pub AlphaMode: i32,
        pub Flags: u32,
    }

    #[repr(C)]
    pub struct DXGI_PRESENT_PARAMETERS {
        pub DirtyRectsCount: u32,
        pub pDirtyRects: *mut RECT,
        pub pScrollRect: *mut RECT,
        pub pScrollOffset: *mut POINT,
    }

    // Each vtable lists the methods in the order of the headers, up to the last one used here.
    // Methods that aren't called are placeholders.

    type Method = usize;

    #[repr(C)]
    pub struct IUnknownVtbl {
        pub QueryInterface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        _AddRef: Method,
        pub Release: unsafe extern "system" fn(*mut c_void) -> u32,
    }

    #[repr(C)]
    pub struct ID3D11DeviceVtbl {
        pub base: IUnknownVtbl,
        /// `CreateBuffer` and `CreateTexture1D`.
        _before_CreateTexture2D: [Method; 2],
        pub CreateTexture2D: unsafe extern "system" fn(
            *mut c_void,
            *const D3D11_TEXTURE2D_DESC,
            *const c_void,
            *mut *mut c_void,
        ) -> HRESULT,
    }

    #[repr(C)]
    pub struct ID3D11DeviceContextVtbl {
        pub base: IUnknownVtbl,
        /// The 4 methods of `ID3D11DeviceChild`, then `VSSetConstantBuffers` to `Draw`.
        _before_Map: [Method; 11],
        pub Map: unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            u32,
            i32,
            u32,
            *mut D3D11_MAPPED_SUBRESOURCE,
        ) -> HRESULT,
        pub Unmap: unsafe extern "system" fn(*mut c_void, *mut c_void, u32),
        /// `PSSetConstantBuffers` to `RSSetScissorRects`.
        _before_CopySubresourceRegion: [Method; 30],
        pub CopySubresourceRegion: unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            u32,
            u32,
            u32,
            u32,
            *mut c_void,
            u32,
            *const D3D11_BOX,
        ),
    }

    /// `IDXGIObject`, which `IDXGIAdapter` inherits without adding methods used here.
    #[repr(C)]
    pub struct IDXGIObjectVtbl {
        pub base: IUnknownVtbl,
        /// `SetPrivateData`, `SetPrivateDataInterface` and `GetPrivateData`.
        _before_GetParent: [Method; 3],
        pub GetParent:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    }

    #[repr(C)]
    pub struct IDXGIDeviceVtbl {
        pub base: IDXGIObjectVtbl,
        pub GetAdapter: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    }

    #[repr(C)]
    pub struct IDXGIFactory2Vtbl {
        pub base: IDXGIObjectVtbl,
        /// `IDXGIFactory::EnumAdapters`.
        _before_MakeWindowAssociation: [Method; 1],
        pub MakeWindowAssociation: unsafe extern "system" fn(*mut c_void, HWND, u32) -> HRESULT,
        /// `GetWindowAssociation` to `IDXGIFactory2::IsWindowedStereoEnabled`.
        _before_CreateSwapChainForHwnd: [Method; 6],
        pub CreateSwapChainForHwnd: unsafe extern "system" fn(
            *mut c_void,
            *mut c_void,
            HWND,
            *const DXGI_SWAP_CHAIN_DESC1,
            *const c_void,
            *mut c_void,
            *mut *mut c_void,
        ) -> HRESULT,
    }

    #[repr(C)]
    pub struct IDXGISwapChain1Vtbl {
        pub base: IDXGIObjectVtbl,
        /// `IDXGIDeviceSubObject::GetDevice` and `IDXGISwapChain::Present`.
        _before_GetBuffer: [Method; 2],
        pub GetBuffer:
            unsafe extern "system" fn(*mut c_void, u32, *const GUID, *mut *mut c_void) -> HRESULT,
        /// `SetFullscreenState` to `GetDesc`.
        _before_ResizeBuffers: [Method; 3],
        pub ResizeBuffers:
            unsafe extern "system" fn(*mut c_void, u32, u32, u32, i32, u32) -> HRESULT,
        /// `ResizeTarget` to `IDXGISwapChain1::GetCoreWindow`.
        _before_Present1: [Method; 8],
        pub Present1: unsafe extern "system" fn(
            *mut c_void,
            u32,
            u32,
            *const DXGI_PRESENT_PARAMETERS,
        ) -> HRESULT,
    }
}
