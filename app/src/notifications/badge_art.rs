//! Procedural taskbar marks; the original desktop ICO files are reference-only.
//! Native 16px references supply the 7px red circle, 3px white unread dot and RGB(243,90,90).
//! Counts use the pinned web number badge's gg sans 600 face at 12px; native digit rasterization is unverified.
use resvg::{tiny_skia, usvg};

pub(super) fn raster(index: usize, font: Option<&[u8]>) -> Option<Vec<u8>> {
    let center = if index == 10 {
        "<circle cx=\"8\" cy=\"8\" r=\"3\" fill=\"white\"/>".to_owned()
    } else {
        let label = if index == 9 {
            "9+".to_owned()
        } else {
            (index + 1).to_string()
        };
        format!(
            "<text x=\"8\" y=\"12\" text-anchor=\"middle\" font-family=\"gg sans\" font-size=\"12\" font-weight=\"600\" fill=\"white\">{label}</text>"
        )
    };
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\"><circle cx=\"8\" cy=\"8\" r=\"7\" fill=\"#f35a5a\"/>{center}</svg>"
    );
    let mut options = usvg::Options::default();
    if index != 10 {
        options.fontdb_mut().load_font_data(font?.to_vec());
    }
    let tree = usvg::Tree::from_str(&svg, &options).ok()?;
    let mut pixels = tiny_skia::Pixmap::new(16, 16)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    Some(pixels.take())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unread_art_has_a_transparent_edge_and_white_center_without_fonts() {
        let pixels = raster(10, None).unwrap();
        assert_eq!(pixels.len(), 16 * 16 * 4);
        assert_eq!(&pixels[..4], &[0, 0, 0, 0]);
        assert_eq!(&pixels[(8 * 16 + 8) * 4..(8 * 16 + 9) * 4], &[255; 4]);
        assert!(raster(0, None).is_none());
    }
}
