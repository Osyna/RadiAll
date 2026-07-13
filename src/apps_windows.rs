//! Windows app source: the Start Menu. The analogue of the freedesktop
//! desktop-entry scan — it walks the per-user and all-users Start Menu
//! `Programs` trees for `.lnk` shortcuts and turns each into a `DesktopApp`:
//!
//! - `name`  = the shortcut's file stem ("Google Chrome").
//! - `exec`  = `[<shortcut path>]`; launched with `ShellExecuteW`, which
//!             resolves the shortcut (target, args, working dir, verbs).
//! - `startup_wm_class` = the *target executable's* stem, lowercased
//!             ("chrome"). That equals the `app_id` the Win32 window adapter
//!             reports (the owning process's exe stem), so the same
//!             `icon_for_class` / open-window matching used on Linux works.
//! - `icon`  = an absolute path to a PNG extracted from the shortcut's icon,
//!             cached under `<config>/iconcache/`, so it flows through the
//!             ordinary decode pipeline in `apps.rs`.

use crate::apps::DesktopApp;
use crate::config::config_dir;
use image::RgbaImage;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::ffi::c_void;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::core::{GUID, PWSTR};
use windows_sys::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    HGDIOBJ,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::UI::Shell::{
    SHGetFileInfoW, SHGetKnownFolderPath, ShellExecuteW, SHFILEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};

// FOLDERID GUIDs (constructed inline so we don't depend on the KNOWNFOLDERID
// constants being re-exported).
const FOLDERID_PROGRAMS: GUID = GUID {
    data1: 0xA77F_5D77,
    data2: 0x2E2B,
    data3: 0x44C3,
    data4: [0xA6, 0xA2, 0xAB, 0xA6, 0x01, 0x05, 0x4A, 0x51],
};
const FOLDERID_COMMON_PROGRAMS: GUID = GUID {
    data1: 0x0139_D44E,
    data2: 0x6AFE,
    data3: 0x49F2,
    data4: [0x86, 0x90, 0x3D, 0xAF, 0xCA, 0xE6, 0xFF, 0xB8],
};
const SHGFI_ICON: u32 = 0x0000_0100;
const SHGFI_LARGEICON: u32 = 0x0000_0000;
const SW_SHOWNORMAL: i32 = 1;

/// Enumerate Start-Menu shortcuts into desktop-app records, sorted by name.
pub fn scan() -> Vec<DesktopApp> {
    let mut out: Vec<DesktopApp> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for root in [known_folder(&FOLDERID_PROGRAMS), known_folder(&FOLDERID_COMMON_PROGRAMS)]
        .into_iter()
        .flatten()
    {
        collect_lnks(&root, &mut out, &mut seen);
    }
    out.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

fn collect_lnks(dir: &Path, out: &mut Vec<DesktopApp>, seen: &mut HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_lnks(&path, out, seen);
            continue;
        }
        if !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
        {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        // Dedup by name across the two Start-Menu roots (user shadows all-users).
        if !seen.insert(name.to_lowercase()) {
            continue;
        }
        out.push(DesktopApp {
            id: name.to_lowercase(),
            icon: cache_icon(&path).unwrap_or_default(),
            exec: vec![path.to_string_lossy().into_owned()],
            startup_wm_class: target_stem(&path),
            actions: Vec::new(),
            name,
        });
    }
}

/// The shortcut target's executable stem ("chrome"), matching the window
/// adapter's `app_id`. Empty when the target can't be read.
fn target_stem(lnk: &Path) -> String {
    match lnk::ShellLink::open(lnk, lnk::encoding::WINDOWS_1252) {
        Ok(sl) => sl.link_target().map(|t| exe_stem(&t)).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

fn exe_stem(path: &str) -> String {
    let base = path.rsplit(['\\', '/']).next().unwrap_or(path);
    base.strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base)
        .to_lowercase()
}

/// Extract the shortcut's icon to a cached PNG (once) and return its path.
fn cache_icon(lnk: &Path) -> Option<String> {
    let dir = config_dir().join("iconcache");
    let mut hasher = DefaultHasher::new();
    lnk.to_string_lossy().hash(&mut hasher);
    let out = dir.join(format!("{:016x}.png", hasher.finish()));
    if out.exists() {
        return Some(out.to_string_lossy().into_owned());
    }
    let (w, h, rgba) = extract_icon_rgba(lnk)?;
    std::fs::create_dir_all(&dir).ok()?;
    let img = RgbaImage::from_raw(w, h, rgba)?;
    img.save(&out).ok()?;
    Some(out.to_string_lossy().into_owned())
}

/// `ShellExecuteW` the program, so `.lnk` shortcuts, `.exe`, documents and URLs
/// all open with their proper handlers.
pub fn launch(argv: &[String]) {
    let Some((program, args)) = argv.split_first() else {
        log::warn!("launch: empty argv");
        return;
    };
    if program.is_empty() {
        log::warn!("launch: empty program name");
        return;
    }
    let file = wide(program);
    let op = wide("open");
    let params = (!args.is_empty()).then(|| wide(&args.join(" ")));
    let params_ptr = params.as_ref().map_or(null(), |p| p.as_ptr());
    let r = unsafe {
        ShellExecuteW(null_mut(), op.as_ptr(), file.as_ptr(), params_ptr, null(), SW_SHOWNORMAL)
    };
    if (r as isize) <= 32 {
        log::error!("launch {program} failed (ShellExecute returned {})", r as isize);
    } else {
        log::info!("launched {program}");
    }
}

// ------------------------------------------------------------- Win32 helpers

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn wide_to_string(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

/// Resolve a KNOWNFOLDERID to a filesystem path.
fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let mut p: PWSTR = null_mut();
        let hr = SHGetKnownFolderPath(id, 0, null_mut(), &mut p);
        if hr < 0 || p.is_null() {
            return None;
        }
        let s = wide_to_string(p);
        CoTaskMemFree(p as *const c_void);
        (!s.is_empty()).then(|| PathBuf::from(s))
    }
}

/// Load the shortcut's large icon and rasterise it to RGBA.
fn extract_icon_rgba(lnk: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let path = wide(&lnk.to_string_lossy());
    unsafe {
        let mut info: SHFILEINFOW = std::mem::zeroed();
        let r = SHGetFileInfoW(
            path.as_ptr(),
            0,
            &mut info,
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if r == 0 || info.hIcon.is_null() {
            return None;
        }
        let out = icon_to_rgba(info.hIcon);
        DestroyIcon(info.hIcon);
        out
    }
}

/// HICON -> top-down RGBA via GetDIBits on its colour bitmap. Icons with no
/// per-pixel alpha (rare, older) are treated as fully opaque.
unsafe fn icon_to_rgba(hicon: HICON) -> Option<(u32, u32, Vec<u8>)> {
    let mut ii: ICONINFO = std::mem::zeroed();
    if GetIconInfo(hicon, &mut ii) == 0 {
        return None;
    }
    let hbm = ii.hbmColor;
    let mut bm: BITMAP = std::mem::zeroed();
    let got = GetObjectW(
        hbm as HGDIOBJ,
        std::mem::size_of::<BITMAP>() as i32,
        &mut bm as *mut BITMAP as *mut c_void,
    );
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        free_icon_bitmaps(&ii);
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = w;
    bi.bmiHeader.biHeight = -h; // negative => top-down rows
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = 0; // BI_RGB

    let hdc = GetDC(null_mut());
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    let lines = GetDIBits(
        hdc,
        hbm,
        0,
        h as u32,
        buf.as_mut_ptr() as *mut c_void,
        &mut bi,
        0, // DIB_RGB_COLORS
    );
    ReleaseDC(null_mut(), hdc);
    free_icon_bitmaps(&ii);
    if lines == 0 {
        return None;
    }

    let any_alpha = buf.chunks_exact(4).any(|px| px[3] != 0);
    let mut rgba = vec![0u8; buf.len()];
    for (i, px) in buf.chunks_exact(4).enumerate() {
        rgba[i * 4] = px[2]; // B -> R
        rgba[i * 4 + 1] = px[1]; // G
        rgba[i * 4 + 2] = px[0]; // R -> B
        rgba[i * 4 + 3] = if any_alpha { px[3] } else { 255 };
    }
    Some((w as u32, h as u32, rgba))
}

unsafe fn free_icon_bitmaps(ii: &ICONINFO) {
    if !ii.hbmColor.is_null() {
        DeleteObject(ii.hbmColor as HGDIOBJ);
    }
    if !ii.hbmMask.is_null() {
        DeleteObject(ii.hbmMask as HGDIOBJ);
    }
}
