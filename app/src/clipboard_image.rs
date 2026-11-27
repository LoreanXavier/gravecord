//! Copy Image on Windows. GPUI writes a picture only under the registered "PNG" format, which many programs never read
//! (Paint, Office, most chat and mail clients paste a device-independent bitmap). The official client, through
//! Chromium, puts the picture on the clipboard both ways: `CF_DIB` (a 32-bit bottom-up bitmap) and "PNG". This module builds both from the decoded picture; [`dib`] is pure and tested without touching the
//! clipboard.

use image::RgbaImage;

/// `BITMAPINFOHEADER` followed by bottom-up BGRA rows: what `CF_DIB` holds. Alpha is kept in the fourth byte with
/// `BI_RGB`, as Chromium writes it.
pub(crate) fn dib(picture: &RgbaImage) -> Vec<u8> {
    const HEADER: u32 = 40;
    let (width, height) = picture.dimensions();
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity(HEADER as usize + stride * height as usize);
    out.extend_from_slice(&HEADER.to_le_bytes()); // biSize
    out.extend_from_slice(&(width as i32).to_le_bytes()); // biWidth
    out.extend_from_slice(&(height as i32).to_le_bytes()); // biHeight > 0: bottom-up
    out.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
    out.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
    out.extend_from_slice(&((stride * height as usize) as u32).to_le_bytes()); // biSizeImage
    out.extend_from_slice(&0i32.to_le_bytes()); // biXPelsPerMeter
    out.extend_from_slice(&0i32.to_le_bytes()); // biYPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrUsed
    out.extend_from_slice(&0u32.to_le_bytes()); // biClrImportant
    for row in (0..height).rev() {
        for column in 0..width {
            let [r, g, b, a] = picture.get_pixel(column, row).0;
            out.extend_from_slice(&[b, g, r, a]);
        }
    }
    out
}

/// The picture as PNG bytes, for the "PNG" format.
pub(crate) fn png(picture: &RgbaImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    picture
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

/// Decodes a cached picture (any format the image crate reads; an animation gives its first frame).
pub(crate) fn decode(bytes: &[u8]) -> Option<RgbaImage> {
    image::load_from_memory(bytes)
        .ok()
        .map(|picture| picture.to_rgba8())
}

/// Puts `picture` on the clipboard as `CF_DIB` and "PNG". In inert capture runs nothing is written: the system
/// clipboard is never touched by tests.
#[cfg(windows)]
pub(crate) fn write(picture: &RgbaImage) -> Result<(), &'static str> {
    if crate::inert::on() {
        tracing::info!(
            width = picture.width(),
            height = picture.height(),
            "inert: clipboard image not written"
        );
        return Ok(());
    }
    let png = png(picture).ok_or("PNG encoding failed")?;
    let dib = dib(picture);
    unsafe { set(&[(CF_DIB, &dib), (png_format()?, &png)]) }
}

#[cfg(not(windows))]
pub(crate) fn write(_picture: &RgbaImage) -> Result<(), &'static str> {
    Err("clipboard images are written on Windows only")
}

#[cfg(windows)]
const CF_DIB: u32 = 8;

#[cfg(windows)]
fn png_format() -> Result<u32, &'static str> {
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    let id = unsafe { RegisterClipboardFormatW(windows::core::w!("PNG")) };
    if id == 0 {
        Err("PNG clipboard format could not be registered")
    } else {
        Ok(id)
    }
}

/// Empties the clipboard and gives it each `(format, bytes)`. The clipboard owns each block once it is set.
#[cfg(windows)]
unsafe fn set(entries: &[(u32, &[u8])]) -> Result<(), &'static str> {
    use windows::Win32::Foundation::{GlobalFree, HANDLE};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    unsafe {
        // Another program may hold the clipboard for a moment.
        let mut opened = false;
        for _ in 0..10 {
            if OpenClipboard(None).is_ok() {
                opened = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !opened {
            return Err("the clipboard is in use");
        }
        let mut result = if EmptyClipboard().is_ok() {
            Ok(())
        } else {
            Err("the clipboard could not be emptied")
        };
        for (format, bytes) in entries {
            if result.is_err() {
                break;
            }
            let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else {
                result = Err("out of memory for the clipboard");
                break;
            };
            let target = GlobalLock(memory);
            if target.is_null() {
                let _ = GlobalFree(Some(memory));
                result = Err("clipboard memory could not be locked");
                break;
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
            let _ = GlobalUnlock(memory);
            if SetClipboardData(*format, Some(HANDLE(memory.0))).is_err() {
                let _ = GlobalFree(Some(memory));
                result = Err("the clipboard refused the picture");
            }
        }
        let _ = CloseClipboard();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dib_is_a_bottom_up_32_bit_bgra_bitmap_with_alpha() {
        // 2 x 2: top row red, half-transparent green; bottom row blue, white.
        let mut picture = RgbaImage::new(2, 2);
        picture.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        picture.put_pixel(1, 0, image::Rgba([0, 255, 0, 128]));
        picture.put_pixel(0, 1, image::Rgba([0, 0, 255, 255]));
        picture.put_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        let bytes = dib(&picture);
        assert_eq!(bytes.len(), 40 + 16);
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        let u16_at = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
        assert_eq!(u32_at(0), 40, "BITMAPINFOHEADER");
        assert_eq!(
            (u32_at(4), u32_at(8)),
            (2, 2),
            "positive height: bottom-up rows"
        );
        assert_eq!((u16_at(12), u16_at(14)), (1, 32));
        assert_eq!(u32_at(16), 0, "BI_RGB");
        assert_eq!(u32_at(20), 16);
        // First stored row is the picture's bottom row, in BGRA.
        assert_eq!(&bytes[40..48], &[255, 0, 0, 255, 255, 255, 255, 255]);
        assert_eq!(&bytes[48..56], &[0, 0, 255, 255, 0, 255, 0, 128]);
    }

    #[test]
    fn png_and_decode_round_trip_the_picture() {
        let mut picture = RgbaImage::new(3, 1);
        picture.put_pixel(2, 0, image::Rgba([10, 20, 30, 40]));
        let bytes = png(&picture).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(decode(&bytes).unwrap(), picture);
    }
}
