//! Renders the glucose value into a tray icon with GDI.
use crate::render::Rgb;
use std::ptr::null_mut;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Renders `text` as large as fits into a `size`×`size` icon.
/// Returns premultiplied BGRA pixels, top-down.
pub fn icon_pixels(text: &str, rgb: Rgb, size: i32, dim: bool) -> Vec<u8> {
    const EM_FACTOR: f32 = 1.12;
    let n = (size * size) as usize;
    let mut out = vec![0u8; n * 4];
    unsafe {
        let hdc = CreateCompatibleDC(null_mut());
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = size;
        bmi.bmiHeader.biHeight = -size;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        let mut bits: *mut core::ffi::c_void = null_mut();
        let dib = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if dib.is_null() || bits.is_null() {
            DeleteDC(hdc);
            return out;
        }
        let old_bmp = SelectObject(hdc, dib);
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, 0x00FF_FFFF);

        let wide: Vec<u16> = text.encode_utf16().collect();
        let face = super::w("Segoe UI");
        // Tall digits are what make the number readable at 16 px, so keep the height and
        // let GDI squeeze the glyphs horizontally until the text fits the icon width.
        let em = (size as f32 * EM_FACTOR).round() as i32;
        let mut width = 0; // 0 = natural proportions
        // Three squeezed digits in a 16 px icon turn into blobs when bold, so those are
        // drawn in regular weight with a pixel of extra spacing.
        let tight = size < 24 && wide.len() >= 3;
        let weight = if tight { 400 } else { 600 };
        if tight {
            SetTextCharacterExtra(hdc, 1);
        }
        loop {
            let font = CreateFontW(-em, width, 0, 0, weight, 0, 0, 0, 0, 0, 0, ANTIALIASED_QUALITY as u32, 0, face.as_ptr());
            let old_font = SelectObject(hdc, font);
            let mut sz = std::mem::zeroed();
            GetTextExtentPoint32W(hdc, wide.as_ptr(), wide.len() as i32, &mut sz);
            let mut tm: TEXTMETRICW = std::mem::zeroed();
            GetTextMetricsW(hdc, &mut tm);
            if sz.cx <= size || width == 2 {
                let digit_h = tm.tmAscent - tm.tmInternalLeading;
                let x = (size - sz.cx) / 2;
                let y = (size - digit_h + 1) / 2 - tm.tmInternalLeading;
                TextOutW(hdc, x, y, wide.as_ptr(), wide.len() as i32);
                SelectObject(hdc, old_font);
                DeleteObject(font);
                break;
            }
            width = if width == 0 { (tm.tmAveCharWidth * size / sz.cx).max(2) } else { width - 1 };
            SelectObject(hdc, old_font);
            DeleteObject(font);
        }
        GdiFlush();

        // GDI drew white-on-black without alpha: use the luminance as coverage.
        let src = std::slice::from_raw_parts(bits as *const u8, n * 4);
        let k = if dim { 150u32 } else { 255u32 };
        for i in 0..n {
            let a = src[i * 4].max(src[i * 4 + 1]).max(src[i * 4 + 2]) as u32 * k / 255;
            out[i * 4] = (rgb.2 as u32 * a / 255) as u8;
            out[i * 4 + 1] = (rgb.1 as u32 * a / 255) as u8;
            out[i * 4 + 2] = (rgb.0 as u32 * a / 255) as u8;
            out[i * 4 + 3] = a as u8;
        }
        SelectObject(hdc, old_bmp);
        DeleteObject(dib);
        DeleteDC(hdc);
    }
    out
}

pub fn icon_from_pixels(pixels: &[u8], size: i32) -> HICON {
    unsafe {
        let color = CreateBitmap(size, size, 1, 32, pixels.as_ptr() as *const _);
        let mask = CreateBitmap(size, size, 1, 1, std::ptr::null());
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}
