//! One persistent Windows Graphics Capture session per game window. The OS controls its privacy border.
//! Polling reuses the session; minimizing or closing the game releases it.

use super::sys::{Hwnd, note};
use std::cell::RefCell;
use std::time::Instant;
use vorax_scan::pixels::Img;
use windows::core::Interface;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

pub struct Capturer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    rt_device: IDirect3DDevice,
    active: RefCell<Option<Active>>,
}

struct Active {
    hwnd: Hwnd,
    session: Session,
    size: windows::Graphics::SizeInt32,
    last: Option<Img>,
    at: Instant,
}

struct Session {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

impl Capturer {
    pub fn new() -> windows::core::Result<Capturer> {
        unsafe {
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context))?;
            let device: ID3D11Device = device.unwrap();
            let dxgi: IDXGIDevice = device.cast()?;
            let rt_device: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;
            Ok(Capturer { device, context: context.unwrap(), rt_device, active: RefCell::new(None) })
        }
    }

    fn start(&self, hwnd: Hwnd) -> windows::core::Result<Session> {
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd.get())? };
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&self.rt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, item.Size()?)?;
        let session = pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsCursorCaptureEnabled(false);
        let border = session.SetIsBorderRequired(false);
        note(&format!("capture session start hwnd={:?}; borderless_request={:?}; Windows may retain its privacy border", hwnd, border));
        session.StartCapture()?;
        Ok(Session { pool, session })
    }

    fn to_img(&self, frame: &windows::Graphics::Capture::Direct3D11CaptureFrame) -> windows::core::Result<Img> {
        unsafe {
            let size = frame.ContentSize()?;
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = access.GetInterface()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            self.device.CreateTexture2D(&desc, None, Some(&mut staging))?;
            let staging = staging.unwrap();
            self.context.CopyResource(&staging, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let (w, h) = ((size.Width as u32).min(desc.Width) as usize, (size.Height as u32).min(desc.Height) as usize);
            let mut img = Img::new(w, h);
            let src = std::slice::from_raw_parts(mapped.pData as *const u8, mapped.RowPitch as usize * h);
            for y in 0..h {
                let row = &src[y * mapped.RowPitch as usize..];
                for x in 0..w {
                    let (b, g, r) = (row[x * 4], row[x * 4 + 1], row[x * 4 + 2]);
                    img.data[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
            self.context.Unmap(&staging, 0);
            Ok(img)
        }
    }

    /// Keep one capture session per game window. Reopening it for every poll flashes the Windows border.
    pub fn grab(&self, hwnd: Hwnd) -> windows::core::Result<Option<Img>> {
        if super::sys::client_rect(hwnd).is_none() { self.stop(); return Ok(None); }
        let mut active = self.active.borrow_mut();
        if active.as_ref().is_none_or(|a| a.hwnd != hwnd) {
            let session = self.start(hwnd)?;
            let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd.get())? };
            *active = Some(Active { hwnd, session, size: item.Size()?, last: None, at: Instant::now() });
        }
        let a = active.as_mut().unwrap();
        // The pool only emits frames when the window changes. Reuse a recent frame for a static board.
        let wait_ms=if a.last.is_some() {40} else {1000};
        let result = self.latest_active(a,wait_ms);
        match result {
            Ok(Some(img)) => {
                a.at = Instant::now();
                a.last = Some(img.clone());
                Ok(Some(img))
            }
            Ok(None) => Ok(a.last.clone()),
            Err(e) => { *active = None; Err(e) }
        }
    }

    fn latest_active(&self, a: &mut Active, wait_ms: u64) -> windows::core::Result<Option<Img>> {
        let start = Instant::now();
        loop {
            let mut newest:Option<windows::Graphics::Capture::Direct3D11CaptureFrame> = None;
            while let Ok(f) = a.session.pool.TryGetNextFrame() { if let Some(old)=newest.replace(f) {let _=old.Close();} }
            if let Some(frame) = newest {
                let size = frame.ContentSize()?;
                if size != a.size {
                    let _ = frame.Close();
                    a.session.pool.Recreate(&self.rt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size)?;
                    note(&format!("capture pool resize {:?} -> {:?}", a.size, size));
                    a.size = size;
                    a.last = None;
                    continue;
                }
                let img = self.to_img(&frame);
                let _ = frame.Close();
                return img.map(|img| Some(client_only(a.hwnd, img)));
            }
            if start.elapsed().as_millis() >= wait_ms as u128 { return Ok(None); }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }

    pub fn state(&self)->serde_json::Value {
        self.active.borrow().as_ref().map_or(serde_json::json!({"active":false}),|a|serde_json::json!({"active":true,"hwnd":a.hwnd.0,"size":[a.size.Width,a.size.Height],"lastFrameAgeMs":a.at.elapsed().as_millis()}))
    }

    pub fn stop(&self) {
        if self.active.borrow_mut().take().is_some() { note("capture session stopped"); }
    }

    /// Stabilize the static UI, not moving monsters or flames elsewhere in the window.
    pub fn grab_stable(&self, hwnd: Hwnd) -> windows::core::Result<Option<Img>> {
        let Some(mut frame) = self.grab(hwnd)? else { return Ok(None) };
        let mut watch = vorax_scan::watch::ScreenWatch::default();
        watch.accept(&frame, 1.0);
        let start = Instant::now();
        let mut still = 0;
        while start.elapsed().as_millis() < 1500 {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let Some(next) = self.grab(hwnd)? else { return Ok(None) };
            still = if watch.differs(&next, 1.0) { 0 } else { still + 1 };
            frame = next;
            watch.accept(&frame, 1.0);
            if still >= 2 { break; }
        }
        Ok(Some(frame))
    }

}

/// The game's picture alone: a window capture also holds its title bar and borders (even ones off screen), so
/// the client area is cut out where Windows says it sits inside the captured frame.
fn client_only(hwnd: Hwnd, img: Img) -> Img {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    let Some((cx, cy, cw, ch)) = super::sys::client_rect(hwnd) else { return img };
    let mut frame = RECT::default();
    let ok = unsafe { DwmGetWindowAttribute(hwnd.get(), DWMWA_EXTENDED_FRAME_BOUNDS, &mut frame as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32) };
    if ok.is_err() {
        return img;
    }
    let (dx, dy) = (cx - frame.left, cy - frame.top);
    if dx < 0 || dy < 0 || (dx + cw) as usize > img.width || (dy + ch) as usize > img.height || (cw as usize == img.width && ch as usize == img.height) {
        return img;
    }
    vorax_scan::window::crop_to(&img, &vorax_scan::pixels::Rect::new(dx as f64, dy as f64, cw as f64, ch as f64))
}
