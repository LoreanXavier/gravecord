//! The selected application mark and the setup frame's PNG encoder.

/// Straight-alpha RGBA from the supplied optical raster at the nearest larger size.
pub fn render_rgba(size: u32) -> Vec<u8> {
    let bytes: &[u8] = if size <= 64 {
        include_bytes!("../../../app/assets/branding/gravecord/installer/gravecord_64.png")
    } else {
        include_bytes!("../../../app/assets/branding/gravecord/installer/gravecord_128.png")
    };
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .expect("bundled installer icon")
        .into_rgba8();
    if image.width() == size {
        image.into_raw()
    } else {
        image::imageops::resize(&image, size, size, image::imageops::FilterType::Lanczos3)
            .into_raw()
    }
}
fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in data {
        a = (a + *byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend((body.len() as u32).to_be_bytes());
    let mut checked = kind.to_vec();
    checked.extend(body);
    out.extend(&checked);
    out.extend(crc32(&checked).to_be_bytes());
}

/// A PNG of straight-alpha RGBA pixels. The zlib stream uses stored blocks: no compressor is needed for icons and shots.
pub fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(rgba.len() + height as usize);
    for row in rgba.chunks((width * 4) as usize) {
        raw.push(0);
        raw.extend(row);
    }
    let mut z = vec![0x78, 0x01];
    let blocks = raw.chunks(65535).collect::<Vec<_>>();
    for (index, block) in blocks.iter().enumerate() {
        z.push((index + 1 == blocks.len()) as u8);
        z.extend((block.len() as u16).to_le_bytes());
        z.extend((!(block.len() as u16)).to_le_bytes());
        z.extend(*block);
    }
    z.extend(adler32(&raw).to_be_bytes());

    let mut header = Vec::new();
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    out
}

pub fn ico() -> Vec<u8> {
    include_bytes!("../../../app/assets/branding/gravecord/app/gravecord.ico").to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_keeps_the_supplied_pixels_and_transparent_corners() {
        let pixels = render_rgba(64);
        assert_eq!(pixels[3], 0, "corner is transparent");
        let supplied = image::load_from_memory(include_bytes!(
            "../../../app/assets/branding/gravecord/installer/gravecord_64.png"
        ))
        .unwrap()
        .into_rgba8();
        assert_eq!(pixels, supplied.into_raw());
    }

    #[test]
    fn png_and_ico_have_valid_framing() {
        let image = png(2, 2, &[255; 16]);
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&image[image.len() - 8..image.len() - 4], b"IEND");
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(&ico()[..6], [0, 0, 1, 0, 8, 0]);
    }
}
