//! Spike 2: Windows.Graphics.Capture with a continuously updated "latest frame".
//!
//! The product decision is that a step image shows the state *before* the click.
//! The low-level hook sees the click before the target app does, but capturing on
//! demand takes longer than the app takes to react. So each monitor keeps a short
//! history of frames in GPU textures (a cheap GPU copy per changed frame), each with
//! its capture time. A click reads back the newest frame captured before the click,
//! so a frame that arrives while the worker is waking up is not used by mistake.

use crate::clock;
use crate::imgops::Frame;
use crate::monitors::Monitor;
use anyhow::{anyhow, Result};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use windows::core::{factory, IInspectable, Interface};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{
    Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession,
};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// Frames of history per monitor. Each one costs a full-screen GPU texture.
const HISTORY: usize = 3;
/// Minimum time between kept frames. A frame arriving sooner overwrites the newest
/// slot, so a busy or high-refresh screen cannot flush the history in a few
/// milliseconds. Together with `HISTORY` this gives at least 50 ms of reach back.
const SPACING_MS: i64 = 25;

struct Slot {
    tex: ID3D11Texture2D,
    /// Capture time in QPC ticks.
    qpc: i64,
}

#[derive(Default)]
struct Ring {
    slots: Vec<Slot>,
    newest: usize,
    width: u32,
    height: u32,
}

struct Shared {
    ring: Mutex<Ring>,
    staging: Mutex<Option<ID3D11Texture2D>>,
    frames: AtomicU64,
    /// Sum of (arrival - capture time), to report how far behind frames arrive.
    delivery_ticks: AtomicI64,
}

struct MonitorCapture {
    monitor: Monitor,
    shared: Arc<Shared>,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    token: i64,
}

pub struct WgcCapturer {
    device: ID3D11Device,
    ctx: Arc<Mutex<ID3D11DeviceContext>>,
    captures: Vec<MonitorCapture>,
    pub border_disabled: bool,
    pub border_error: Option<String>,
}

pub struct Grabbed {
    pub frame: Frame,
    /// How long before the click the frame was captured. Large values are normal on a
    /// static screen. Negative means no frame in the history predates the click.
    pub frame_age_ms: f64,
    /// Readback cost: GPU copy, map, and CPU copy.
    pub readback_ms: f64,
    /// True when a newer frame existed and was correctly skipped.
    pub skipped_newer: bool,
}

impl WgcCapturer {
    pub fn supported() -> bool {
        GraphicsCaptureSession::IsSupported().unwrap_or(false)
    }

    pub fn start(monitors: &[Monitor]) -> Result<Self> {
        let (device, ctx) = create_device()?;
        let winrt_device = winrt_device(&device)?;
        let ctx = Arc::new(Mutex::new(ctx));
        let mut captures = Vec::new();
        let mut border_disabled = true;
        let mut border_error = None;

        for m in monitors {
            let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(m.hmonitor())? };
            let size = item.Size()?;
            let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
                &winrt_device,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                size,
            )?;
            let shared = Arc::new(Shared {
                ring: Mutex::new(Ring::default()),
                staging: Mutex::new(None),
                frames: AtomicU64::new(0),
                delivery_ticks: AtomicI64::new(0),
            });

            let handler_shared = shared.clone();
            let handler_ctx = ctx.clone();
            let handler_device = device.clone();
            let handler = TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(
                move |sender, _| {
                    let Some(pool) = sender.as_ref() else { return Ok(()) };
                    let frame = pool.TryGetNextFrame()?;
                    let surface = frame.Surface()?;
                    let access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
                    let tex: ID3D11Texture2D = unsafe { access.GetInterface()? };
                    // SystemRelativeTime is QPC-based, in 100 ns units.
                    let captured = frame.SystemRelativeTime()?.Duration as i128 * clock::freq() as i128 / 10_000_000;
                    store_frame(&handler_shared, &handler_ctx, &handler_device, &tex, captured as i64)?;
                    let _ = frame.Close();
                    Ok(())
                },
            );
            let token = pool.FrameArrived(&handler)?;

            let session = pool.CreateCaptureSession(&item)?;
            let _ = session.SetIsCursorCaptureEnabled(false);
            if let Err(e) = session.SetIsBorderRequired(false) {
                border_disabled = false;
                border_error = Some(e.to_string());
            }
            session.StartCapture()?;
            captures.push(MonitorCapture { monitor: *m, shared, pool, session, token });
        }
        Ok(Self { device, ctx, captures, border_disabled, border_error })
    }

    pub fn frames_received(&self) -> u64 {
        self.captures.iter().map(|c| c.shared.frames.load(Ordering::Relaxed)).sum()
    }

    /// Mean delay from a frame's capture time to its arrival here.
    pub fn mean_delivery_ms(&self) -> f64 {
        let frames: u64 = self.captures.iter().map(|c| c.shared.frames.load(Ordering::Relaxed)).sum();
        let ticks: i64 = self.captures.iter().map(|c| c.shared.delivery_ticks.load(Ordering::Relaxed)).sum();
        if frames == 0 {
            0.0
        } else {
            clock::to_ms(ticks) / frames as f64
        }
    }

    /// Read back the newest frame captured before `click_t` (the hook timestamp).
    pub fn grab(&self, index: usize, click_t: i64) -> Result<Grabbed> {
        let cap = self.captures.get(index).ok_or_else(|| anyhow!("no capture for monitor {index}"))?;
        let t0 = clock::now();
        let ring = cap.shared.ring.lock().unwrap();
        let newest_qpc = ring.slots.iter().map(|s| s.qpc).max().ok_or_else(|| anyhow!("no frame received yet"))?;
        let chosen = ring
            .slots
            .iter()
            .filter(|s| s.qpc <= click_t)
            .max_by_key(|s| s.qpc)
            // Nothing predates the click: the oldest frame is the closest we have.
            .or_else(|| ring.slots.iter().min_by_key(|s| s.qpc))
            .ok_or_else(|| anyhow!("no frame received yet"))?;
        let (w, h) = (ring.width, ring.height);

        let mut staging_guard = cap.shared.staging.lock().unwrap();
        let recreate = match staging_guard.as_ref() {
            Some(s) => {
                let mut d = D3D11_TEXTURE2D_DESC::default();
                unsafe { s.GetDesc(&mut d) };
                d.Width != w || d.Height != h
            }
            None => true,
        };
        if recreate {
            let mut d = D3D11_TEXTURE2D_DESC::default();
            unsafe { chosen.tex.GetDesc(&mut d) };
            d.Usage = D3D11_USAGE_STAGING;
            d.BindFlags = 0;
            d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            d.MiscFlags = 0;
            let mut tex = None;
            unsafe { self.device.CreateTexture2D(&d, None, Some(&mut tex))? };
            *staging_guard = tex;
        }
        let staging = staging_guard.as_ref().ok_or_else(|| anyhow!("staging texture missing"))?;

        let mut bgra = vec![0u8; w as usize * h as usize * 4];
        {
            let ctx = self.ctx.lock().unwrap();
            unsafe {
                ctx.CopyResource(staging, &chosen.tex);
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                ctx.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
                let row = w as usize * 4;
                let pitch = mapped.RowPitch as usize;
                let src = mapped.pData as *const u8;
                for y in 0..h as usize {
                    std::ptr::copy_nonoverlapping(src.add(y * pitch), bgra.as_mut_ptr().add(y * row), row);
                }
                ctx.Unmap(staging, 0);
            }
        }
        let t1 = clock::now();
        Ok(Grabbed {
            frame: Frame {
                width: w,
                height: h,
                origin_x: cap.monitor.x,
                origin_y: cap.monitor.y,
                bgra,
            },
            frame_age_ms: clock::to_ms(click_t - chosen.qpc),
            readback_ms: clock::to_ms(t1 - t0),
            skipped_newer: chosen.qpc != newest_qpc,
        })
    }
}

impl Drop for WgcCapturer {
    fn drop(&mut self) {
        for c in &self.captures {
            let _ = c.pool.RemoveFrameArrived(c.token);
            let _ = c.session.Close();
            let _ = c.pool.Close();
        }
    }
}

fn store_frame(
    shared: &Shared,
    ctx: &Mutex<ID3D11DeviceContext>,
    device: &ID3D11Device,
    src: &ID3D11Texture2D,
    captured_qpc: i64,
) -> windows::core::Result<()> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { src.GetDesc(&mut desc) };
    let mut ring = shared.ring.lock().unwrap();
    if ring.width != desc.Width || ring.height != desc.Height {
        *ring = Ring { slots: Vec::new(), newest: 0, width: desc.Width, height: desc.Height };
    }
    let spacing = clock::freq() * SPACING_MS / 1000;
    let overwrite_newest = ring
        .slots
        .get(ring.newest)
        .map(|s| captured_qpc - s.qpc < spacing)
        .unwrap_or(false);
    if !overwrite_newest {
        if ring.slots.len() < HISTORY {
            let mut d = desc;
            d.Usage = D3D11_USAGE_DEFAULT;
            d.BindFlags = 0;
            d.CPUAccessFlags = 0;
            d.MiscFlags = 0;
            let mut tex = None;
            unsafe { device.CreateTexture2D(&d, None, Some(&mut tex))? };
            if let Some(tex) = tex {
                ring.slots.push(Slot { tex, qpc: captured_qpc });
                ring.newest = ring.slots.len() - 1;
            }
        } else {
            ring.newest = (ring.newest + 1) % HISTORY;
        }
    }
    let idx = ring.newest;
    if let Some(slot) = ring.slots.get_mut(idx) {
        let c = ctx.lock().unwrap();
        unsafe { c.CopyResource(&slot.tex, src) };
        slot.qpc = captured_qpc;
    }
    shared.frames.fetch_add(1, Ordering::Relaxed);
    shared.delivery_ticks.fetch_add(clock::now() - captured_qpc, Ordering::Relaxed);
    Ok(())
}

fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut ctx = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut ctx),
        )?;
    }
    Ok((
        device.ok_or_else(|| anyhow!("D3D11CreateDevice returned no device"))?,
        ctx.ok_or_else(|| anyhow!("D3D11CreateDevice returned no context"))?,
    ))
}

fn winrt_device(device: &ID3D11Device) -> Result<IDirect3DDevice> {
    let dxgi: IDXGIDevice = device.cast()?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi)? };
    Ok(inspectable.cast()?)
}
