//! What the composer refuses before a file joins the upload row: `openUploadError` of module 16407 with the checks of
//! module 518960 (`R`, in this order: an empty file, a file or message over the size limit, more than ten files).
//! A refused batch adds nothing to the row. The limit is module 453771 `m` (the user's, or the guild feature's, at
//! least 20 MiB, module 409481), the sizes of modules 682901 and 292348.

use super::Root;
use super::modal::{self, Tone};
use super::nitro::oklab;
use dc_state::premium::PremiumType;
use gpui::{AnyElement, Context, IntoElement, RenderImage, SharedString, div, prelude::*};
use std::sync::{Arc, OnceLock};

const MIB: u64 = 1024 * 1024;
/// `TbF` (module 652215): what an account without Nitro may send.
const BASE_LIMIT: u64 = 10 * MIB;
/// `t` of module 409481: no limit is ever shown below this.
const FLOOR_LIMIT: u64 = 20 * MIB;
/// `eW` and `0x3200000` of module 682901: Nitro Basic and Nitro Classic, and the guild's `MAX_FILE_SIZE_50_MB`.
const MID_LIMIT: u64 = 50 * MIB;
/// `eX` of module 682901: the guild's `MAX_FILE_SIZE_100_MB`.
const GUILD_100_LIMIT: u64 = 100 * MIB;
/// What Nitro sends: the experiment value of `getUserMaxFileSize`, read as "1GB" in the capture's upsell subtitle
/// (design/golden/windows/upload/too-large-modal); without that experiment the module's constant is 500 MB.
const NITRO_LIMIT: u64 = 1024 * MIB;
/// `jS` of module 292348 without its experiment: the most a message may carry in all.
const MESSAGE_LIMIT: u64 = 524_288_000;
/// Files a message takes (module 518960: `getUploadCount + files > 10`).
pub(super) const MAX_FILES: usize = 10;

/// The dialog on screen.
pub(super) enum UploadError {
    /// A title, a line of help and "Got It" (`n.a` in module 16407).
    Plain { title: &'static str, help: String },
    /// "Uh oh, this file exceeds the size limit.", with the Nitro offer (`R` in module 16407).
    Upsell { max: u64 },
}

/// The size a file may have for this account in this guild: the largest of the user's limit, the guild's feature
/// limit and 20 MiB (`C(o2(guild))`).
pub(super) fn limit(premium: PremiumType, guild_features: &[String]) -> u64 {
    let user = match premium {
        PremiumType::None => BASE_LIMIT,
        PremiumType::Classic | PremiumType::Basic => MID_LIMIT,
        PremiumType::Nitro => NITRO_LIMIT,
    };
    let guild = |feature: &str| guild_features.iter().any(|name| name == feature);
    let from_guild = if guild("MAX_FILE_SIZE_100_MB") {
        GUILD_100_LIMIT
    } else if guild("MAX_FILE_SIZE_50_MB") {
        MID_LIMIT
    } else {
        BASE_LIMIT
    };
    user.max(from_guild).max(FLOOR_LIMIT)
}

/// `Hb` (the `filesize` library, base 2): "20 MB", "1.5 GB", at most two decimals.
fn file_size(bytes: u64) -> String {
    let (value, unit) = scaled(bytes);
    format!("{value} {unit}")
}

/// `Xq` of kibibytes: the same size without the space ("20MB", "1GB").
fn compact_size(bytes: u64) -> String {
    let (value, unit) = scaled(bytes);
    format!("{value}{unit}")
}

fn scaled(bytes: u64) -> (String, &'static str) {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut index = 0;
    while value >= 1024. && index + 1 < units.len() {
        value /= 1024.;
        index += 1;
    }
    let text = format!("{:.2}", (value * 100.).round() / 100.);
    let text = text.trim_end_matches('0').trim_end_matches('.').to_owned();
    (text, units[index])
}

/// The check of module 518960 on a batch of `sizes`, with `existing` files already in the row.
pub(super) fn check(
    sizes: &[u64],
    existing: usize,
    premium: PremiumType,
    guild_features: &[String],
) -> Result<(), UploadError> {
    if sizes.contains(&0) {
        return Err(UploadError::Plain {
            title: dc_intl::tr("Upload Failed"),
            help: dc_intl::tr("File cannot be empty").to_owned(),
        });
    }
    let limit = limit(premium, guild_features);
    if sizes.iter().any(|&size| size > limit) {
        if premium == PremiumType::Nitro {
            // Only Nitro itself sees the plain dialog (`showPremiumUpsell` is off for it); everyone else gets the
            // upsell below.
            return Err(UploadError::Plain {
                title: dc_intl::tr("Your files are too powerful"),
                help: format!("Max file size is {}, please.", file_size(limit)),
            });
        }
        return Err(UploadError::Upsell { max: limit });
    }
    if sizes.iter().sum::<u64>() > MESSAGE_LIMIT {
        return Err(UploadError::Plain {
            title: dc_intl::tr("Your files are too powerful"),
            help: format!(
                "Total message size must be less than {}",
                file_size(MESSAGE_LIMIT)
            ),
        });
    }
    if existing + sizes.len() > MAX_FILES {
        return Err(UploadError::Plain {
            title: dc_intl::tr("Too many uploads!"),
            help: format!("You can only upload {MAX_FILES} files at a time!"),
        });
    }
    Ok(())
}

/// `.container_pmhtZL.nitro-pink`'s `::before`: `linear-gradient(270deg in oklab, end 0, start 100%)` at 40%,
/// masked by `radial-gradient(100% 100% at 50% 100%, transparent 60%, #000 100%)`, as pixels. The ends are the
/// capture's `oklab(0.395045 -0.0154365 -0.253271)` and `oklab(0.449387 0.167872 -0.086863)`.
pub(super) fn header_glow(width: u32, height: u32) -> Arc<RenderImage> {
    type GlowCache = std::sync::Mutex<std::collections::HashMap<(u32, u32), Arc<RenderImage>>>;
    static GLOW: OnceLock<GlowCache> = OnceLock::new();
    let cache = GLOW.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cached) = map.get(&(width, height)) {
        return cached.clone();
    }
    let image = {
        let end = [0.395_045_f32, -0.015_436_5, -0.253_271];
        let start = [0.449_387_f32, 0.167_872, -0.086_863];
        let mut pixels = image::RgbaImage::new(width, height);
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            // 270deg runs right to left: `end` at the right edge.
            let along = 1. - (x as f32 + 0.5) / width as f32;
            let mix = |index: usize| end[index] + (start[index] - end[index]) * along;
            let rgb = oklab(mix(0), mix(1), mix(2), 1.).to_rgb();
            let dx = (x as f32 + 0.5 - width as f32 / 2.) / width as f32;
            let dy = (y as f32 + 0.5 - height as f32) / height as f32;
            let ring = (dx * dx + dy * dy).sqrt();
            let mask = ((ring - 0.6) / 0.4).clamp(0., 1.);
            let channel = |value: f32| (value.clamp(0., 1.) * 255. + 0.5) as u8;
            // The atlas takes BGRA, straight alpha.
            *pixel = image::Rgba([
                channel(rgb.b),
                channel(rgb.g),
                channel(rgb.r),
                channel(0.4 * mask),
            ]);
        }
        Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]))
    };
    map.insert((width, height), image.clone());
    image
}

impl Root {
    /// Puts the dialog up for a refused batch, or reports that the batch may join the row.
    pub(super) fn refuse_upload(&mut self, sizes: &[u64]) -> bool {
        let features = self
            .guild
            .and_then(|current| self.stores.guilds.iter().find(|guild| guild.id == current))
            .map(|guild| guild.features.clone())
            .unwrap_or_default();
        match check(sizes, self.pending_files.len(), self.premium(), &features) {
            Ok(()) => false,
            Err(error) => {
                self.upload_error = Some(error);
                true
            }
        }
    }

    pub(super) fn upload_error_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        Some(match self.upload_error.as_ref()? {
            UploadError::Plain { title, help } => modal::render(
                modal::Spec {
                    id: "upload-error",
                    title: (*title).into(),
                    subtitle: Some(div().child(help.clone()).into_any_element()),
                    body: None,
                    buttons: vec![modal::button(
                        "upload-error-ok",
                        dc_intl::tr("Got It"),
                        Tone::Primary,
                        |this, _, _| this.upload_error = None,
                    )],
                    close: Box::new(|this, _, _| this.upload_error = None),
                },
                cx,
            ),
            UploadError::Upsell { max } => self.upload_upsell(*max, cx),
        })
    }

    /// `R` of module 16407 in the premium upsell dialog (module 831318, `premium_upsell.rs`): "Uh oh, this file
    /// exceeds the size limit.", the limits, View All Nitro Perks and Get Nitro.
    fn upload_upsell(&self, max: u64, cx: &mut Context<Self>) -> AnyElement {
        let subtitle: SharedString = format!(
            "The max file size is {}. Upgrade to Discord Nitro to upload files up to {}.",
            compact_size(max),
            compact_size(NITRO_LIMIT)
        )
        .into();
        self.premium_upsell(
            super::premium_upsell::Spec {
                id: "upload-upsell",
                ids: [
                    "upload-upsell-close",
                    "upload-upsell-perks",
                    "upload-upsell-get",
                ],
                header: super::premium_upsell::Header::Video,
                title: dc_intl::tr("Uh oh, this file exceeds the size limit.").into(),
                subtitle,
                secondary: dc_intl::tr("View All Nitro Perks").into(),
                on_secondary: Box::new(|this, _, cx| {
                    this.upload_error = None;
                    this.open_nitro(cx);
                }),
                on_primary: Box::new(|this, _, cx| {
                    this.upload_error = None;
                    this.open_purchase(false, cx);
                }),
                close: Box::new(|this, _, _| this.upload_error = None),
                priority: 1,
            },
            cx,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_limit_is_the_largest_of_the_user_the_guild_and_twenty_mib() {
        assert_eq!(limit(PremiumType::None, &[]), 20 * MIB);
        assert_eq!(limit(PremiumType::Classic, &[]), 50 * MIB);
        assert_eq!(limit(PremiumType::Nitro, &[]), 1024 * MIB);
        assert_eq!(
            limit(PremiumType::None, &features(&["MAX_FILE_SIZE_50_MB"])),
            50 * MIB
        );
        assert_eq!(
            limit(
                PremiumType::Classic,
                &features(&["MAX_FILE_SIZE_100_MB", "MAX_FILE_SIZE_50_MB"])
            ),
            100 * MIB
        );
    }

    #[test]
    fn sizes_read_as_the_official_strings_do() {
        assert_eq!(compact_size(20 * MIB), "20MB");
        assert_eq!(compact_size(1024 * MIB), "1GB");
        assert_eq!(file_size(MESSAGE_LIMIT), "500 MB");
        assert_eq!(file_size(1_572_864), "1.5 MB");
    }

    #[test]
    fn an_oversized_file_without_nitro_gets_the_upsell_and_nothing_joins() {
        let big = 600 * MIB;
        match check(&[1024, big], 0, PremiumType::None, &[]) {
            Err(UploadError::Upsell { max }) => assert_eq!(max, 20 * MIB),
            _ => panic!("expected the upsell"),
        }
        match check(&[2 * 1024 * MIB], 0, PremiumType::Nitro, &[]) {
            Err(UploadError::Plain { title, help }) => {
                assert_eq!(title, "Your files are too powerful");
                assert_eq!(help, "Max file size is 1 GB, please.");
            }
            _ => panic!("expected the plain dialog"),
        }
    }

    #[test]
    fn empty_files_the_total_and_the_count_each_have_their_dialog() {
        let help = |result: Result<(), UploadError>| match result {
            Err(UploadError::Plain { help, .. }) => help,
            _ => panic!("expected a plain dialog"),
        };
        assert_eq!(
            help(check(&[0], 0, PremiumType::None, &[])),
            "File cannot be empty"
        );
        assert_eq!(
            help(check(&[500 * MIB, 100 * MIB], 0, PremiumType::Nitro, &[])),
            "Total message size must be less than 500 MB"
        );
        assert_eq!(
            help(check(&[1; 3], 8, PremiumType::None, &[])),
            "You can only upload 10 files at a time!"
        );
        assert!(check(&[1; 2], 8, PremiumType::None, &[]).is_ok());
    }
}
