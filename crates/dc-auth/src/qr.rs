//! The login QR code: `https://discord.com/ra/{fingerprint}` at error correction level M (`level: "M"` in module 414121).

use qrcode::{Color, EcLevel, QrCode};

/// The page that opens when a phone scans the code (module 785161).
pub fn login_url(fingerprint: &str) -> String {
    format!("https://discord.com/ra/{fingerprint}")
}

/// A square grid of modules, row by row, `true` where dark. No quiet zone: the card draws its own 8px of white.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    pub size: usize,
    pub dark: Vec<bool>,
}

impl Matrix {
    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        self.dark[y * self.size + x]
    }
}

pub fn matrix(text: &str) -> Option<Matrix> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
    Some(Matrix {
        size: code.width(),
        dark: code
            .to_colors()
            .into_iter()
            .map(|c| c == Color::Dark)
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fingerprint_url_is_a_version_5_code() {
        // 23 characters of URL and a 43 character fingerprint of mixed case: 66 bytes, level M, version 5, 37 modules a
        // side, as the pinned page's golden has (viewBox 0 0 37 37).
        let fingerprint = "aZ3-xQ_kLm9pR2sT7vW1yB4cD6eF8gH0jN5uOaZ3xQk";
        assert_eq!(fingerprint.len(), 43);
        let grid = matrix(&login_url(fingerprint)).unwrap();
        assert_eq!(grid.size, 37);
        assert_eq!(grid.dark.len(), 37 * 37);
        // The three finder patterns: dark corners, and the same text always gives the same grid.
        assert!(grid.is_dark(0, 0) && grid.is_dark(36, 0) && grid.is_dark(0, 36));
        assert_eq!(grid, matrix(&login_url(fingerprint)).unwrap());
    }
}
