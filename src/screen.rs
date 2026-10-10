//! Reads a few pixels of the primary screen for the Ambient layer, with DXGI Desktop
//! Duplication: the GPU copies only the pixels needed into a small texture that is read from the
//! CPU; if the screen has not changed nothing is read. It is opened only when the active effect
//! has an Ambient, and is closed with it.
//! (GDI was ruled out: every BitBlt from the screen waits for the compositor, ~16 ms.)

use crate::layers::Rgb;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_WAIT_TIMEOUT,
    DXGI_OUTDUPL_FRAME_INFO,
};

const STRIP_W: u32 = 8192; // one row of the grid = one strip of pixels
const MAX_ROWS: u32 = 64;

pub struct Screen {
    output: IDXGIOutput1,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    dup: Option<IDXGIOutputDuplication>,
    seen: bool,  // at least one reading done: until then, a still screen is read with GDI
    staging: [ID3D11Texture2D; 2],
    pending: Option<usize>, // texture with a copy in flight on the GPU, to be read on the next pass
    next: usize,
    size: (i32, i32),
}

impl Screen {
    /// Screen unchanged. But the duplication delivers an image only when something changes:
    /// if I have not seen one yet (screen still since it was opened) I read the pixels once with GDI.
    fn still(&mut self, requests: &[(Vec<i32>, Vec<i32>)]) -> Option<Vec<Vec<Rgb>>> {
        if self.seen {
            return None;
        }
        self.seen = true;
        Some(requests.iter().map(|(xs, ys)| gdi_sample(xs, ys)).collect())
    }

    /// The primary screen (the one with its corner at (0, 0) of the desktop).
    pub fn open() -> Option<Screen> {
        unsafe {
            let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
            let mut a = 0;
            while let Ok(adapter) = factory.EnumAdapters1(a) {
                let mut o = 0;
                while let Ok(output) = adapter.EnumOutputs(o) {
                    let desc = output.GetDesc().ok()?;
                    let r = desc.DesktopCoordinates;
                    if desc.AttachedToDesktop.as_bool() && r.left == 0 && r.top == 0 {
                        let output: IDXGIOutput1 = output.cast().ok()?;
                        let mut device = None;
                        let mut context = None;
                        D3D11CreateDevice(
                            &adapter,
                            D3D_DRIVER_TYPE_UNKNOWN,
                            Default::default(),
                            D3D11_CREATE_DEVICE_FLAG(0),
                            None,
                            D3D11_SDK_VERSION,
                            Some(&mut device),
                            None,
                            Some(&mut context),
                        )
                        .ok()?;
                        let device: ID3D11Device = device?;
                        let desc = D3D11_TEXTURE2D_DESC {
                            Width: STRIP_W,
                            Height: MAX_ROWS,
                            MipLevels: 1,
                            ArraySize: 1,
                            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                            Usage: D3D11_USAGE_STAGING,
                            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                            ..Default::default()
                        };
                        let (mut a, mut b) = (None, None);
                        device.CreateTexture2D(&desc, None, Some(&mut a)).ok()?;
                        device.CreateTexture2D(&desc, None, Some(&mut b)).ok()?;
                        let size = (r.right - r.left, r.bottom - r.top);
                        return Some(Screen { output, device, context: context?, dup: None, seen: false, staging: [a?, b?], pending: None, next: 0, size });
                    }
                    o += 1;
                }
                a += 1;
            }
            None
        }
    }

    /// Width and height of the primary screen, in pixels.
    pub fn size(&self) -> (i32, i32) {
        self.size
    }

    /// Reads the requested pixels: for each request (columns, rows) the pixels row by row.
    /// None = nothing new (screen unchanged, or not readable now, e.g. the UAC screen).
    /// The GPU copies this pass's pixels and I read them on the next pass (40 ms): that way I do not
    /// wait for the GPU, which cost more than everything else.
    pub fn capture(&mut self, requests: &[(Vec<i32>, Vec<i32>)]) -> Option<Vec<Vec<Rgb>>> {
        let out = self.pending.and_then(|i| self.read(i, requests));
        if out.is_some() {
            self.pending = None;
        }
        let copied = self.copy(requests);
        if copied {
            return out;
        }
        out.or_else(|| self.still(requests))
    }

    /// Copies the pixels of the latest desktop image into one of the two textures. false = no
    /// new image.
    fn copy(&mut self, requests: &[(Vec<i32>, Vec<i32>)]) -> bool {
        unsafe {
            if self.dup.is_none() {
                self.dup = self.output.DuplicateOutput(&self.device).ok();
                if let Some(d) = &self.dup {
                    let desc = d.GetDesc();
                    self.size = (desc.ModeDesc.Width as i32, desc.ModeDesc.Height as i32);
                }
            }
            let Some(dup) = self.dup.as_ref() else { return false };
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res: Option<IDXGIResource> = None;
            if let Err(e) = dup.AcquireNextFrame(0, &mut info, &mut res) {
                if e.code() != DXGI_ERROR_WAIT_TIMEOUT {
                    self.dup = None; // resolution change, protected desktop...: I reopen it later
                }
                return false;
            }
            // only the cursor has changed: the image is the same as before
            let tex: Option<ID3D11Texture2D> = if info.LastPresentTime != 0 { res.and_then(|r| r.cast().ok()) } else { None };
            let Some(tex) = tex else {
                let _ = dup.ReleaseFrame();
                return false;
            };
            let (w, h) = self.size;
            let dst = self.next;
            let mut row = 0u32;
            for (xs, ys) in requests {
                let x0 = xs.iter().cloned().min().unwrap_or(0).clamp(0, w - 1) as u32;
                let x1 = (xs.iter().cloned().max().unwrap_or(0).clamp(0, w - 1) as u32).min(x0 + STRIP_W - 1);
                for &y in ys {
                    if row < MAX_ROWS {
                        let y = y.clamp(0, h - 1) as u32;
                        let b = D3D11_BOX { left: x0, top: y, front: 0, right: x1 + 1, bottom: y + 1, back: 1 };
                        self.context.CopySubresourceRegion(&self.staging[dst], 0, 0, row, 0, &tex, 0, Some(&b));
                    }
                    row += 1;
                }
            }
            let _ = dup.ReleaseFrame();
            self.seen = true;
            self.pending = Some(dst);
            self.next = 1 - dst;
            true
        }
    }

    /// Reads a texture copied earlier, if the GPU has finished (otherwise None, I retry later).
    fn read(&self, i: usize, requests: &[(Vec<i32>, Vec<i32>)]) -> Option<Vec<Vec<Rgb>>> {
        unsafe {
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&self.staging[i], 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut map)).ok()?;
            let pitch = map.RowPitch as usize / 4;
            let px = std::slice::from_raw_parts(map.pData as *const u32, pitch * MAX_ROWS as usize);
            let (w, _) = self.size;
            let mut row = 0usize;
            let out = requests
                .iter()
                .map(|(xs, ys)| {
                    let x0 = xs.iter().cloned().min().unwrap_or(0).clamp(0, w - 1);
                    let mut v = Vec::with_capacity(xs.len() * ys.len());
                    for _ in ys {
                        for &x in xs {
                            let col = (x.clamp(0, w - 1) - x0) as usize;
                            let p = if row < MAX_ROWS as usize && col < STRIP_W as usize { px[row * pitch + col] } else { 0 };
                            v.push(((p >> 16) as u8, (p >> 8) as u8, p as u8));
                        }
                        row += 1;
                    }
                    v
                })
                .collect();
            self.context.Unmap(&self.staging[i], 0);
            Some(out)
        }
    }
}

/// Slow read with GDI (every copy from the screen waits for the compositor): only the first time.
fn gdi_sample(xs: &[i32], ys: &[i32]) -> Vec<Rgb> {
    use windows::Win32::Graphics::Gdi::{GetDC, GetPixel, ReleaseDC};
    unsafe {
        let dc = GetDC(None);
        let out = ys
            .iter()
            .flat_map(|&y| xs.iter().map(move |&x| (x, y)))
            .map(|(x, y)| {
                let c = GetPixel(dc, x, y).0; // 0x00BBGGRR
                (c as u8, (c >> 8) as u8, (c >> 16) as u8)
            })
            .collect();
        ReleaseDC(None, dc);
        out
    }
}
