//! The application mark: a white letter G on a rounded square in the brand colour, drawn from geometry so the repository
//! carries no binary art. Shared by the build scripts (executable icon) and the setup window (logo and screenshots).

/// `#5865f2`, Discord's brand colour (`--background-brand`).
const BRAND: [u8; 3] = [0x58, 0x65, 0xf2];

/// Coverage of the mark at a point in the unit square: `(background, letter)`.
fn sample(x: f32, y: f32) -> (bool, bool) {
    // Rounded square, radius 0.22.
    let (cx, cy) = (
        (x - 0.5).abs() - (0.5 - 0.22),
        (y - 0.5).abs() - (0.5 - 0.22),
    );
    let inside = cx.max(0.0).hypot(cy.max(0.0)) + cx.max(cy).min(0.0) <= 0.22;
    let (dx, dy) = (x - 0.5, y - 0.5);
    let radius = dx.hypot(dy);
    let angle = dy.atan2(dx).to_degrees();
    // The ring of the G is open on the upper right, between -50 degrees and the crossbar at 0.
    let ring = (0.19..=0.31).contains(&radius) && !(-50.0..0.0).contains(&angle);
    let bar = (0.5..=0.81).contains(&x) && (0.5 - 0.06..=0.5 + 0.06).contains(&y);
    (inside, ring || bar)
}

/// Straight-alpha RGBA, `size` x `size`, 4x4 supersampled.
pub fn render_rgba(size: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    const N: u32 = 4;
    for py in 0..size {
        for px in 0..size {
            let (mut back, mut letter) = (0u32, 0u32);
            for sy in 0..N {
                for sx in 0..N {
                    let x = (px as f32 + (sx as f32 + 0.5) / N as f32) / size as f32;
                    let y = (py as f32 + (sy as f32 + 0.5) / N as f32) / size as f32;
                    let (b, l) = sample(x, y);
                    back += b as u32;
                    if b && l {
                        letter += 1;
                    }
                }
            }
            let alpha = back * 255 / (N * N);
            let mix = (letter * 255).checked_div(back).unwrap_or(0);
            for channel in BRAND {
                out.push(((channel as u32 * (255 - mix) + 255 * mix) / 255) as u8);
            }
            out.push(alpha as u8);
        }
    }
    out
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

/// An `.ico` with PNG-compressed images (Windows Vista and later).
pub fn ico() -> Vec<u8> {
    const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|size| png(*size, *size, &render_rgba(*size)))
        .collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend((SIZES.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (size, image) in SIZES.iter().zip(&images) {
        let byte = if *size >= 256 { 0 } else { *size as u8 };
        out.extend([byte, byte, 0, 0, 1, 0, 32, 0]);
        out.extend((image.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend(image);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_has_transparent_corners_and_a_white_letter() {
        let pixels = render_rgba(64);
        assert_eq!(pixels[3], 0, "corner is transparent");
        let at = |x: usize, y: usize| pixels[(y * 64 + x) * 4..(y * 64 + x) * 4 + 3].to_vec();
        assert_eq!(at(32, 14), [255, 255, 255], "top of the ring");
        assert_eq!(at(32, 24), BRAND.to_vec(), "inside the ring is empty");
    }

    #[test]
    fn png_and_ico_have_valid_framing() {
        let image = png(2, 2, &[255; 16]);
        assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&image[image.len() - 8..image.len() - 4], b"IEND");
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        assert_eq!(&ico()[..6], [0, 0, 1, 0, 7, 0]);
    }
}
