//! Owner-selected application artwork. This preference is local to the active profile.
use crate::storage;
use gpui::{Image, ImageFormat};
use std::{path::Path, sync::Arc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IconSet {
    #[default]
    Gravecord,
    Original,
}
impl IconSet {
    pub fn from_name(name: Option<&str>) -> Self {
        if name == Some("original") {
            Self::Original
        } else {
            Self::Gravecord
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Gravecord => "gravecord",
            Self::Original => "original",
        }
    }
}

pub fn selected() -> IconSet {
    IconSet::from_name(
        storage::get("appearance.icon_set")
            .as_ref()
            .and_then(|v| v.as_str()),
    )
}
pub fn select(set: IconSet) {
    storage::set("appearance.icon_set", serde_json::json!(set.name()));
}
pub fn home_path() -> &'static str {
    match selected() {
        IconSet::Gravecord => "branding/gravecord/ui/rail_home.svg",
        IconSet::Original => "icons/home.svg",
    }
}
pub fn preview(set: IconSet) -> Arc<Image> {
    let bytes: &'static [u8] = match set {
        IconSet::Gravecord => include_bytes!("../assets/branding/gravecord/app/gravecord_32.png"),
        IconSet::Original => include_bytes!("../assets/branding/original/app/app_32.png"),
    };
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes.to_vec()))
}
pub fn login_logo() -> Arc<Image> {
    Arc::new(Image::from_bytes(
        ImageFormat::Svg,
        include_bytes!("../assets/branding/gravecord/login/mark_and_wordmark.svg").to_vec(),
    ))
}
pub fn qr_logo() -> Arc<Image> {
    Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../assets/branding/gravecord/app/gravecord_64.png").to_vec(),
    ))
}

fn premultiplied(image: image::RgbaImage) -> Vec<u8> {
    let mut bytes = image.into_raw();
    for pixel in bytes.as_chunks_mut::<4>().0 {
        for channel in 0..3 {
            pixel[channel] = ((pixel[channel] as u16 * pixel[3] as u16 + 127) / 255) as u8;
        }
    }
    bytes
}

/// Isolate an existing ICO frame before decoding, preserving its optical size and AND mask.
fn ico_frame(bytes: &[u8], size: u32) -> Option<Vec<u8>> {
    if bytes.get(..4)? != [0, 0, 1, 0] {
        return None;
    }
    let count = u16::from_le_bytes(bytes.get(4..6)?.try_into().ok()?) as usize;
    for i in 0..count {
        let entry = bytes.get(6 + i * 16..22 + i * 16)?;
        let width = if entry[0] == 0 {
            256
        } else {
            u32::from(entry[0])
        };
        let height = if entry[1] == 0 {
            256
        } else {
            u32::from(entry[1])
        };
        if width != size || height != size {
            continue;
        }
        let length = u32::from_le_bytes(entry[8..12].try_into().ok()?) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().ok()?) as usize;
        let data = bytes.get(offset..offset.checked_add(length)?)?;
        let mut single = vec![0, 0, 1, 0, 1, 0];
        single.extend_from_slice(entry);
        single[18..22].copy_from_slice(&22u32.to_le_bytes());
        single.extend_from_slice(data);
        return Some(single);
    }
    None
}

pub fn app_pixels(set: IconSet, size: u32) -> Option<Vec<u8>> {
    let root = crate::find_assets()?;
    let file = match set {
        IconSet::Gravecord => "gravecord/app/gravecord.ico",
        IconSet::Original => "original/app/app.ico",
    };
    decode_icon(&root.join("branding").join(file), size)
}
fn decode_icon(path: &Path, size: u32) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    let frame = ico_frame(&bytes, size)?;
    let image = image::load_from_memory_with_format(&frame, image::ImageFormat::Ico)
        .ok()?
        .into_rgba8();
    Some(premultiplied(image))
}
#[cfg(feature = "capture")]
pub fn badge_pixels(set: IconSet, index: usize) -> Option<Vec<u8>> {
    badge_at(&crate::find_assets()?, set, index)
}

pub fn tray_pixels(set: IconSet, icon: crate::tray::Icon, size: u32) -> Option<(u32, Vec<u8>)> {
    tray_at(&crate::find_assets()?, set, icon, size)
}
fn tray_at(
    root: &Path,
    set: IconSet,
    icon: crate::tray::Icon,
    size: u32,
) -> Option<(u32, Vec<u8>)> {
    // Voice state uses the call indicator rather than an application-logo badge (desktop core systemtray/win32).
    let set = match icon {
        crate::tray::Icon::Default | crate::tray::Icon::Unread => set,
        _ => IconSet::Original,
    };
    // Native optical sizes supplied in both packs, matching the pinned Windows resources.
    let variants = [
        (16, ""),
        (20, "@1.25x"),
        (21, "@1.33x"),
        (22, "@1.4x"),
        (24, "@1.5x"),
        (29, "@1.8x"),
        (32, "@2x"),
        (40, "@2.5x"),
        (48, "@3x"),
        (64, "@4x"),
        (80, "@5x"),
    ];
    let (size, suffix) = variants
        .iter()
        .copied()
        .find(|(width, _)| *width >= size)
        .unwrap_or((80, "@5x"));
    let path = root.join(format!(
        "branding/{}/systemtray/win32/{}{}.png",
        set.name(),
        icon.file(),
        suffix
    ));
    let image = image::open(path).ok()?.into_rgba8();
    if image.dimensions() != (size, size) {
        return None;
    }
    Some((size, premultiplied(image)))
}
/// The taskbar's count badge. Both icon sets show the official client's badges (`images/badges` of desktop core):
/// the set chooses the app's own icon, and a count drawn in another weight and size reads as another program's.
#[cfg(any(test, feature = "capture"))]
fn badge_at(root: &Path, _set: IconSet, index: usize) -> Option<Vec<u8>> {
    if index >= 11 {
        return None;
    }
    decode_icon(
        &root.join(format!("branding/original/badges/badge-{}.ico", index + 1)),
        16,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn voice_indicators_keep_their_native_artwork_without_replacing_app_branding() {
        use crate::tray::Icon::*;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for (width, suffix) in [
            (16, ""),
            (20, "@1.25x"),
            (21, "@1.33x"),
            (22, "@1.4x"),
            (24, "@1.5x"),
            (29, "@1.8x"),
            (32, "@2x"),
            (40, "@2.5x"),
            (48, "@3x"),
            (64, "@4x"),
            (80, "@5x"),
        ] {
            for icon in [Connected, Speaking, Muted, Deafened] {
                let reference = image::open(root.join(format!(
                    "branding/original/systemtray/win32/{}{suffix}.png",
                    icon.file()
                )))
                .unwrap()
                .into_rgba8();
                let expected = (width, premultiplied(reference));
                for set in [IconSet::Gravecord, IconSet::Original] {
                    assert_eq!(tray_at(&root, set, icon, width).unwrap(), expected);
                }
            }
            for icon in [Default, Unread] {
                assert_ne!(
                    tray_at(&root, IconSet::Gravecord, icon, width),
                    tray_at(&root, IconSet::Original, icon, width),
                );
            }
        }
        assert_ne!(
            tray_at(&root, IconSet::Gravecord, Connected, 16),
            tray_at(&root, IconSet::Gravecord, Speaking, 16),
        );
    }

    #[test]
    fn both_tray_sets_supply_all_states_at_each_optical_size() {
        use crate::tray::Icon::*;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for set in [IconSet::Gravecord, IconSet::Original] {
            for icon in [Default, Unread, Connected, Speaking, Muted, Deafened] {
                for width in [16, 20, 21, 22, 24, 29, 32, 40, 48, 64, 80] {
                    let (size, pixels) = tray_at(&root, set, icon, width).unwrap();
                    assert_eq!(size, width);
                    assert_eq!(pixels.len(), (size * size * 4) as usize);
                    assert!(pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[3] != 0));
                }
            }
        }
    }
    #[test]
    fn unset_or_unknown_icon_choice_defaults_to_gravecord() {
        assert_eq!(IconSet::from_name(None), IconSet::Gravecord);
        assert_eq!(IconSet::from_name(Some("future")), IconSet::Gravecord);
        for set in [IconSet::Gravecord, IconSet::Original] {
            assert_eq!(IconSet::from_name(Some(set.name())), set);
        }
    }
    #[test]
    fn both_icon_sets_show_the_official_badges_at_their_native_size() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        for set in [IconSet::Gravecord, IconSet::Original] {
            for index in 0..11 {
                let pixels = badge_at(&root, set, index).expect("valid native ICO frame");
                assert_eq!(pixels.len(), 16 * 16 * 4);
                // The original 16px BMP frames retain white at alpha 1 in the corner: both sets give those frames.
                assert_eq!(pixels[3], 1);
                assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[3] == 255));
                assert!(
                    pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|p| p[..3].iter().all(|v| *v <= p[3]))
                );
            }
        }
        assert_eq!(
            badge_at(&root, IconSet::Gravecord, 0),
            badge_at(&root, IconSet::Original, 0)
        );
        assert!(ico_frame(&[0, 0, 1, 0, 255, 255], 16).is_none());
    }
}
