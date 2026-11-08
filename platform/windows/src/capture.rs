//! Screenshots that show the state before the click.
//!
//! The low-level hook sees a click before the target app does, but capturing on
//! demand takes longer than the app takes to react (M0: wrong in 0 of 30 trials
//! correct). So Windows.Graphics.Capture runs continuously, and each monitor keeps
//! a short history of frames in GPU textures, each with its capture time. A click
//! reads back the newest frame captured before it.
//!
//! The monitor list is checked on every grab. When monitors are added, removed or
//! resized, or the GPU device is lost (driver reset, some sleep and wake cycles),
//! capture is rebuilt. Until new frames arrive, a grab falls back to GDI, which
//! cannot show the before state, and says so in [`Grabbed::before_verified`].

use crate::clock;
use crate::frame::Frame;
use crate::monitors::{self, Monitor};
use anyhow::{anyhow, bail, Result};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use windows::core::{factory, IInspectable, Interface};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

/// Frames of history per monitor. Each one costs a full-screen GPU texture.
const HISTORY: usize = 3;
/// Minimum time between kept frames. A frame arriving sooner overwrites the newest
/// slot, so a busy or high-refresh screen cannot flush the history in a few
/// milliseconds. With `HISTORY` this gives at least 50 ms of reach back.
const SPACING_MS: i64 = 25;

struct Slot {
    tex: ID3D11Texture2D,
    meta: SlotTimes,
}

/// When a slot started filling, and the capture time of what it holds now.
/// A slot keeps taking newer frames for `SPACING_MS` after `first`, then the
/// ring moves on, so older slots stay older and the history reaches back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SlotTimes {
    first: i64,
    qpc: i64,
}

/// Where a new frame captured at `captured` goes: `Some(i)` overwrites slot `i`
/// (the newest, still within its spacing window, or the oldest when full);
/// `None` means push a new slot.
fn slot_for(times: &[SlotTimes], newest: usize, captured: i64, spacing: i64, history: usize) -> Option<usize> {
    match times.get(newest) {
        Some(n) if captured - n.first < spacing => Some(newest),
        _ if times.len() < history => None,
        _ => Some((newest + 1) % history),
    }
}

/// The slot to read for an event at `t`, and whether its frame is proven to be
/// the screen just before `t`. Not proven when nothing predates `t`, or when a
/// slot that held a frame from before `t` was since overwritten by one after
/// it: that lost frame was newer than anything left.
fn choose(times: &[SlotTimes], t: i64) -> Option<(usize, bool)> {
    let before = times.iter().enumerate().filter(|(_, s)| s.qpc <= t).max_by_key(|(_, s)| s.qpc).map(|(i, _)| i);
    match before {
        Some(i) => {
            let lost = times.iter().any(|s| s.first <= t && s.qpc > t);
            Some((i, !lost))
        }
        None => times.iter().enumerate().min_by_key(|(_, s)| s.qpc).map(|(i, _)| (i, false)),
    }
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
    frames: AtomicU64,
}

struct MonitorCapture {
    shared: Arc<Shared>,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    token: i64,
    staging: Option<ID3D11Texture2D>,
}

struct Wgc {
    device: ID3D11Device,
    ctx: Arc<Mutex<ID3D11DeviceContext>>,
    /// One per entry of `Capturer::monitors`, same order. `None` if that monitor failed.
    captures: Vec<Option<MonitorCapture>>,
}

impl Drop for Wgc {
    fn drop(&mut self) {
        for c in self.captures.iter().flatten() {
            let _ = c.pool.RemoveFrameArrived(c.token);
            let _ = c.session.Close();
            let _ = c.pool.Close();
        }
    }
}

pub struct Grabbed {
    pub frame: Frame,
    /// The frame is proven to predate the input event.
    pub before_verified: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CaptureStats {
    pub wgc_frames: u64,
    pub rebuilds: u64,
    pub gdi_fallbacks: u64,
    pub not_verified: u64,
    pub last_error: Option<String>,
    pub border_disabled: bool,
}

pub struct Capturer {
    monitors: Vec<Monitor>,
    wgc: Option<Wgc>,
    last_rebuild: std::time::Instant,
    pub stats: CaptureStats,
    /// Force the GDI path (self-test only), to prove the "not verified" flag works.
    pub force_gdi: bool,
}

impl Capturer {
    pub fn start() -> Self {
        let mut c = Capturer {
            monitors: Vec::new(),
            wgc: None,
            last_rebuild: std::time::Instant::now(),
            stats: CaptureStats::default(),
            force_gdi: false,
        };
        c.rebuild();
        c.stats.rebuilds = 0;
        c
    }

    pub fn monitors(&self) -> &[Monitor] {
        &self.monitors
    }

    pub fn wgc_running(&self) -> bool {
        self.wgc.as_ref().is_some_and(|w| w.captures.iter().any(Option::is_some))
    }

    fn rebuild(&mut self) {
        self.wgc = None; // Close the old sessions first.
        self.last_rebuild = std::time::Instant::now();
        self.monitors = monitors::enumerate();
        self.stats.rebuilds += 1;
        if !GraphicsCaptureSession::IsSupported().unwrap_or(false) {
            self.stats.last_error = Some("Windows.Graphics.Capture is not supported".into());
            return;
        }
        match start_wgc(&self.monitors) {
            Ok((w, border_disabled)) => {
                self.wgc = Some(w);
                self.stats.border_disabled = border_disabled;
            }
            Err(e) => self.stats.last_error = Some(format!("{e:#}")),
        }
    }

    /// Throw away every kept frame and start capture again. Used when the person
    /// comes back from a lock or sleep: whatever was kept before is stale, and
    /// until a new frame arrives a click is flagged as not verified.
    pub fn reset(&mut self) {
        self.rebuild();
    }

    /// Rebuild if the set of monitors or their sizes changed, or the GPU was lost.
    fn refresh(&mut self) {
        let now = monitors::enumerate();
        let device_lost = self.wgc.as_ref().is_some_and(|w| unsafe { w.device.GetDeviceRemovedReason().is_err() });
        // When capture cannot start at all, retry every few seconds, not on
        // every click: building a device takes time on this thread.
        let retry = self.wgc.is_none() && self.last_rebuild.elapsed() > std::time::Duration::from_secs(5);
        if now != self.monitors || device_lost || retry {
            self.rebuild();
        }
    }

    pub fn frames_received(&self) -> u64 {
        self.wgc.as_ref().map_or(0, |w| w.captures.iter().flatten().map(|c| c.shared.frames.load(Ordering::Relaxed)).sum())
    }

    /// The screen of the monitor containing (x, y), as it was just before `t` (QPC).
    pub fn grab(&mut self, x: i32, y: i32, t: i64) -> Result<Grabbed> {
        self.refresh();
        let idx = monitors::index_for(&self.monitors, x, y)
            .ok_or_else(|| anyhow!("the point is on no monitor"))?;
        if !self.force_gdi {
            match self.grab_wgc(idx, t) {
                Ok(g) => {
                    if !g.before_verified {
                        self.stats.not_verified += 1;
                    }
                    return Ok(g);
                }
                Err(e) => self.stats.last_error = Some(format!("{e:#}")),
            }
        }
        let m = self.monitors[idx];
        let frame = gdi_capture(m.x, m.y, m.w, m.h)?;
        self.stats.gdi_fallbacks += 1;
        self.stats.not_verified += 1;
        Ok(Grabbed { frame, before_verified: false })
    }

    fn grab_wgc(&mut self, idx: usize, t: i64) -> Result<Grabbed> {
        let m = self.monitors[idx];
        let wgc = self.wgc.as_mut().ok_or_else(|| anyhow!("capture is not running"))?;
        let cap = wgc.captures.get_mut(idx).and_then(Option::as_mut).ok_or_else(|| anyhow!("no capture for this monitor"))?;
        let ring = cap.shared.ring.lock().unwrap_or_else(|e| e.into_inner());
        let times: Vec<SlotTimes> = ring.slots.iter().map(|s| s.meta).collect();
        let Some((idx, before_verified)) = choose(&times, t) else { bail!("no frame received yet") };
        let chosen = &ring.slots[idx];
        let (w, h) = (ring.width, ring.height);
        if i64::from(w) != i64::from(m.w) || i64::from(h) != i64::from(m.h) {
            bail!("frame size {w}x{h} does not match the monitor {}x{}", m.w, m.h);
        }

        let recreate = match &cap.staging {
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
            unsafe { wgc.device.CreateTexture2D(&d, None, Some(&mut tex))? };
            cap.staging = tex;
        }
        let staging = cap.staging.as_ref().ok_or_else(|| anyhow!("staging texture missing"))?;

        let mut bgra = vec![0u8; w as usize * h as usize * 4];
        {
            let ctx = wgc.ctx.lock().unwrap_or_else(|e| e.into_inner());
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
        drop(ring);
        Ok(Grabbed { frame: Frame { width: w, height: h, origin_x: m.x, origin_y: m.y, bgra }, before_verified })
    }
}

fn start_wgc(monitors: &[Monitor]) -> Result<(Wgc, bool)> {
    let (device, ctx) = create_device()?;
    let winrt_device = winrt_device(&device)?;
    let ctx = Arc::new(Mutex::new(ctx));
    let mut wgc = Wgc { device: device.clone(), ctx: ctx.clone(), captures: Vec::new() };
    let mut border_disabled = true;
    for m in monitors {
        match start_monitor(m, &device, &winrt_device, &ctx) {
            Ok((c, border)) => {
                border_disabled &= border;
                wgc.captures.push(Some(c));
            }
            Err(_) => wgc.captures.push(None),
        }
    }
    if wgc.captures.iter().all(Option::is_none) {
        bail!("capture could not start on any monitor");
    }
    Ok((wgc, border_disabled))
}

fn start_monitor(
    m: &Monitor,
    device: &ID3D11Device,
    winrt_device: &IDirect3DDevice,
    ctx: &Arc<Mutex<ID3D11DeviceContext>>,
) -> Result<(MonitorCapture, bool)> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
    let item: GraphicsCaptureItem = unsafe { interop.CreateForMonitor(m.hmonitor())? };
    let size = item.Size()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        winrt_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        size,
    )?;
    let shared = Arc::new(Shared { ring: Mutex::new(Ring::default()), frames: AtomicU64::new(0) });
    let (handler_shared, handler_ctx, handler_device) = (shared.clone(), ctx.clone(), device.clone());
    let handler = TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |sender, _| {
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
    });
    let token = pool.FrameArrived(&handler)?;
    let session = pool.CreateCaptureSession(&item)?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    let border = session.SetIsBorderRequired(false).is_ok();
    session.StartCapture()?;
    Ok((MonitorCapture { shared, pool, session, token, staging: None }, border))
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
    let mut ring = shared.ring.lock().unwrap_or_else(|e| e.into_inner());
    if ring.width != desc.Width || ring.height != desc.Height {
        *ring = Ring { slots: Vec::new(), newest: 0, width: desc.Width, height: desc.Height };
    }
    let spacing = clock::freq() * SPACING_MS / 1000;
    let times: Vec<SlotTimes> = ring.slots.iter().map(|s| s.meta).collect();
    let idx = match slot_for(&times, ring.newest, captured_qpc, spacing, HISTORY) {
        Some(i) => {
            if i != ring.newest {
                ring.slots[i].meta.first = captured_qpc; // Moving on: a fresh window.
            }
            i
        }
        None => {
            let mut d = desc;
            d.Usage = D3D11_USAGE_DEFAULT;
            d.BindFlags = 0;
            d.CPUAccessFlags = 0;
            d.MiscFlags = 0;
            let mut tex = None;
            unsafe { device.CreateTexture2D(&d, None, Some(&mut tex))? };
            let Some(tex) = tex else { return Ok(()) };
            ring.slots.push(Slot { tex, meta: SlotTimes { first: captured_qpc, qpc: captured_qpc } });
            ring.slots.len() - 1
        }
    };
    ring.newest = idx;
    if let Some(slot) = ring.slots.get_mut(idx) {
        let c = ctx.lock().unwrap_or_else(|e| e.into_inner());
        unsafe { c.CopyResource(&slot.tex, src) };
        slot.meta.qpc = captured_qpc;
    }
    shared.frames.fetch_add(1, Ordering::Relaxed);
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

/// GDI `BitBlt`: the last-resort fallback. It captures now, so it cannot show the
/// state before an input event.
fn gdi_capture(x: i32, y: i32, w: i32, h: i32) -> Result<Frame> {
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            bail!("GetDC failed");
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, HGDIOBJ(bmp.0));
        let result = (|| -> Result<Frame> {
            BitBlt(mem, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT)?;
            let mut info = BITMAPINFO::default();
            info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = w;
            info.bmiHeader.biHeight = -h; // Negative means top-down.
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB.0;
            let mut bgra = vec![0u8; w as usize * h as usize * 4];
            let lines = GetDIBits(mem, bmp, 0, h as u32, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
            if lines == 0 {
                bail!("GetDIBits failed");
            }
            Ok(Frame { width: w as u32, height: h as u32, origin_x: x, origin_y: y, bgra })
        })();
        SelectObject(mem, old);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulates the ring with frames arriving every `every` ticks until `until`.
    fn run(every: i64, until: i64, spacing: i64) -> Vec<SlotTimes> {
        let mut times: Vec<SlotTimes> = Vec::new();
        let mut newest = 0;
        let mut t = 0;
        while t <= until {
            let idx = match slot_for(&times, newest, t, spacing, HISTORY) {
                Some(i) => {
                    if i != newest {
                        times[i].first = t;
                    }
                    i
                }
                None => {
                    times.push(SlotTimes { first: t, qpc: t });
                    times.len() - 1
                }
            };
            newest = idx;
            times[idx].qpc = t;
            t += every;
        }
        times
    }

    #[test]
    fn busy_screen_keeps_advancing_and_reaches_back() {
        // 60 Hz (16 ms per frame), 25 ms spacing: the old ring overwrote one slot forever.
        let times = run(16, 1000, 25);
        assert!(times.iter().all(|s| s.first > 900), "slots must move on with time: {times:?}");
        let newest = times.iter().map(|s| s.qpc).max().unwrap();
        let oldest = times.iter().map(|s| s.qpc).min().unwrap();
        assert!(newest - oldest >= 25, "history must reach back: {times:?}");
    }

    #[test]
    fn frame_just_before_the_click_is_chosen() {
        let times = run(16, 1000, 25);
        let newest = times.iter().map(|s| s.qpc).max().unwrap();
        let t = newest + 5; // The click, after the newest frame.
        let (i, verified) = choose(&times, t).unwrap();
        assert_eq!(times[i].qpc, newest);
        assert!(verified);
    }

    #[test]
    fn overwritten_pre_click_frame_is_not_verified() {
        // Slot 2 started before the click and now holds a frame after it.
        let times = vec![
            SlotTimes { first: 0, qpc: 10 },
            SlotTimes { first: 30, qpc: 40 },
            SlotTimes { first: 60, qpc: 80 },
        ];
        let (i, verified) = choose(&times, 70).unwrap();
        assert_eq!(times[i].qpc, 40);
        assert!(!verified, "the frame from 60-70 was lost, so 40 may be stale");
    }

    #[test]
    fn clean_history_is_verified() {
        let times = vec![
            SlotTimes { first: 0, qpc: 10 },
            SlotTimes { first: 30, qpc: 40 },
            SlotTimes { first: 60, qpc: 60 },
        ];
        assert_eq!(choose(&times, 70), Some((2, true)));
        // Static screen: one old frame is still the current state.
        assert_eq!(choose(&[SlotTimes { first: 5, qpc: 5 }], 100_000), Some((0, true)));
    }

    #[test]
    fn nothing_before_the_event_is_not_verified() {
        let times = vec![SlotTimes { first: 50, qpc: 60 }];
        assert_eq!(choose(&times, 40), Some((0, false)));
        assert_eq!(choose(&[], 40), None);
    }
}
