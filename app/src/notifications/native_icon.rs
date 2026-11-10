//! Alpha icons from owned memory bitmaps. No window or desktop surface is created or read.
use windows::{
    Win32::{
        Foundation::{E_FAIL, E_INVALIDARG},
        Graphics::Gdi::{
            BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject,
            HBITMAP, RGBQUAD,
        },
        UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO},
    },
    core::{Error, Result},
};

struct Bitmap(HBITMAP);
impl Drop for Bitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

fn header(size: i32, depth: u16) -> BITMAPINFOHEADER {
    BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: size,
        biHeight: -size,
        biPlanes: 1,
        biBitCount: depth,
        biCompression: BI_RGB.0,
        ..Default::default()
    }
}

/// Takes premultiplied RGBA rows ordered from top to bottom, as tiny-skia produces them.
pub(crate) fn create(size: i32, rgba: &[u8]) -> Result<HICON> {
    let width = usize::try_from(size)
        .ok()
        .filter(|size| *size > 0)
        .ok_or_else(|| Error::new(E_INVALIDARG, "invalid icon size"))?;
    if width.checked_mul(width).and_then(|n| n.checked_mul(4)) != Some(rgba.len()) {
        return Err(Error::new(E_INVALIDARG, "invalid icon buffer"));
    }
    let info = BITMAPINFO {
        bmiHeader: header(size, 32),
        ..Default::default()
    };
    let mut color_bits = std::ptr::null_mut();
    let color =
        Bitmap(unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut color_bits, None, 0)? });
    if color_bits.is_null() {
        return Err(Error::new(E_FAIL, "icon color buffer unavailable"));
    }
    // The validated buffer length equals the allocated 32bpp DIB extent.
    let target = unsafe { std::slice::from_raw_parts_mut(color_bits.cast::<u8>(), rgba.len()) };
    target.copy_from_slice(rgba);
    for pixel in target.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }

    #[repr(C)]
    struct MaskInfo {
        header: BITMAPINFOHEADER,
        palette: [RGBQUAD; 2],
    }
    let mask_info = MaskInfo {
        header: header(size, 1),
        palette: [
            RGBQUAD::default(),
            RGBQUAD {
                rgbRed: 255,
                rgbGreen: 255,
                rgbBlue: 255,
                rgbReserved: 0,
            },
        ],
    };
    let mut mask_bits = std::ptr::null_mut();
    let mask = Bitmap(unsafe {
        CreateDIBSection(
            None,
            (&mask_info as *const MaskInfo).cast(),
            DIB_RGB_COLORS,
            &mut mask_bits,
            None,
            0,
        )?
    });
    if mask_bits.is_null() {
        return Err(Error::new(E_FAIL, "icon mask buffer unavailable"));
    }
    let stride = width.div_ceil(32) * 4;
    let mask_pixels =
        unsafe { std::slice::from_raw_parts_mut(mask_bits.cast::<u8>(), stride * width) };
    mask_pixels.fill(255);
    for (index, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] != 0 {
            let (x, y) = (index % width, index / width);
            mask_pixels[y * stride + x / 8] &= !(0x80 >> (x % 8));
        }
    }
    let icon = ICONINFO {
        fIcon: true.into(),
        hbmColor: color.0,
        hbmMask: mask.0,
        ..Default::default()
    };
    // CreateIconIndirect copies both bitmaps; their guards release them on success and error alike.
    unsafe { CreateIconIndirect(&icon) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::{
        Graphics::Gdi::{CreateCompatibleDC, DeleteDC, GetDIBits},
        UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo},
    };
    #[test]
    fn owned_native_icon_preserves_top_down_color_and_alpha() {
        let mut rgba = vec![0; 16 * 16 * 4];
        rgba[..4].copy_from_slice(&[255, 0, 0, 255]);
        rgba[(15 * 16) * 4..(15 * 16 + 1) * 4].copy_from_slice(&[0, 0, 128, 128]);
        let icon = create(16, &rgba).unwrap();
        let mut copied = ICONINFO::default();
        unsafe {
            GetIconInfo(icon, &mut copied).unwrap();
        }
        let color = Bitmap(copied.hbmColor);
        let _mask = Bitmap(copied.hbmMask);
        let mut info = BITMAPINFO {
            bmiHeader: header(16, 32),
            ..Default::default()
        };
        let mut pixels = vec![0u8; rgba.len()];
        let dc = unsafe { CreateCompatibleDC(None) };
        assert!(!dc.is_invalid());
        let rows = unsafe {
            GetDIBits(
                dc,
                color.0,
                0,
                16,
                Some(pixels.as_mut_ptr().cast()),
                &mut info,
                DIB_RGB_COLORS,
            )
        };
        unsafe {
            let _ = DeleteDC(dc);
            DestroyIcon(icon).unwrap();
        }
        assert_eq!(rows, 16);
        assert_eq!(&pixels[..4], &[0, 0, 255, 255]);
        assert_eq!(&pixels[(15 * 16) * 4..(15 * 16 + 1) * 4], &[128, 0, 0, 128]);
        assert_eq!(&pixels[4..8], &[0, 0, 0, 0]);
    }
}
