//! Capture-only: the perf harness steps that need the main view's private state. `perf caches` prints how much each
//! cache of the app holds, and `go <place>` opens the places the rail and the sidebars open.

use super::{Root, discover, name_styles};
use gpui::Context;

impl Root {
    /// One line of `name=items/bytes` fields, one per cache the app keeps. The bytes are what the cache holds in
    /// memory (decoded pixels where it keeps them, encoded files where it keeps those).
    pub(crate) fn perf_caches(&self) -> String {
        let mut fields: Vec<(&str, crate::perf::Size)> = Vec::new();
        let mut images = crate::perf::Size::default();
        for image in self.images.values() {
            images.add(1, image.bytes.len());
        }
        fields.push(("images_encoded", images));
        // What gpui keeps once the pictures were drawn: width x height x 4 per picture, from the file headers.
        let mut decoded = crate::perf::Size::default();
        let mut shop = crate::perf::Size::default();
        let mut large = crate::perf::Size::default();
        let mut widest = 0;
        for (key, image) in &self.images {
            let dimensions = image::ImageReader::new(std::io::Cursor::new(&image.bytes))
                .with_guessed_format()
                .ok()
                .and_then(|reader| reader.into_dimensions().ok());
            if let Some((width, height)) = dimensions {
                let bytes = width as usize * height as usize * 4;
                decoded.add(1, bytes);
                if key.starts_with("shop:") {
                    shop.add(1, bytes);
                }
                if bytes >= 1024 * 1024 {
                    large.add(1, bytes);
                    widest = widest.max(width.max(height));
                }
            }
        }
        fields.push(("images_decoded_est", decoded));
        fields.push(("decoded_shop", shop));
        fields.push(("decoded_1mb_plus", large));
        fields.push((
            "longest_side",
            crate::perf::Size {
                items: widest as usize,
                bytes: 0,
            },
        ));
        let mut generated = crate::perf::Size::default();
        for image in self.generated_images.borrow().values() {
            generated.add(1, image.bytes.len());
        }
        fields.push(("generated", generated));
        // The decoded pictures the image cache tracks, and how many it dropped so far.
        let (tracked, evicted) = super::image_cache::perf_size();
        fields.push(("decoded_tracked", tracked));
        fields.push((
            "decoded_evicted",
            crate::perf::Size {
                items: evicted,
                bytes: 0,
            },
        ));
        let mut fetched = crate::perf::Size::default();
        for body in self.fetched.values() {
            fetched.add(1, body.to_string().len());
        }
        fields.push(("fetched_json", fetched));
        let mut messages = crate::perf::Size::default();
        for list in self.stores.messages.values() {
            messages.add(list.len(), 0);
        }
        fields.push(("messages", messages));
        let mut members = crate::perf::Size::default();
        for list in self.stores.member_lists.values() {
            members.add(list.rows.len(), 0);
        }
        fields.push(("member_rows", members));
        fields.push((
            "users",
            crate::perf::Size {
                items: self.stores.users.len(),
                bytes: 0,
            },
        ));
        fields.push((
            "requested",
            crate::perf::Size {
                items: self.requested.len(),
                bytes: 0,
            },
        ));
        fields.push(("member_avatars", self.member_avatars.borrow().perf_size()));
        fields.push(("nameplates", self.nameplates.borrow().perf_size()));
        fields.push(("profile_effects", self.profile_effects.borrow().perf_size()));
        fields.push(("inline_video", self.inline_videos.borrow().perf_size()));
        fields.push(("stickers", self.sticker_ui.player.borrow().perf_size()));
        fields.push(("name_styles", name_styles::perf_size()));
        let total: usize = fields.iter().map(|(_, size)| size.bytes).sum();
        let mut line = String::new();
        for (name, size) in &fields {
            line.push_str(&format!("{name}={size} "));
        }
        line.push_str(&format!("cache_bytes_total={total}"));
        line
    }

    /// Capture harness step `perf badge`: what one refresh of the unread badge costs on the account's servers, which
    /// every update of the client runs once.
    pub(crate) fn perf_badge(&mut self) -> String {
        let rounds = 200;
        let started = std::time::Instant::now();
        for _ in 0..rounds {
            self.refresh_notification_badge();
        }
        let each = started.elapsed().as_secs_f64() * 1000. / f64::from(rounds);
        let channels: usize = self
            .stores
            .guilds
            .iter()
            .map(|guild| guild.channels.len())
            .sum();
        format!(
            "badge refresh: ms_each={each:.4} guilds={} channels={channels}",
            self.stores.guilds.len()
        )
    }

    /// Capture harness step `go home|dm|shop|discover|apps|quests`: what a click on the rail or a sidebar row does.
    pub(crate) fn script_go(&mut self, words: &[&str], cx: &mut Context<Self>) -> String {
        match words {
            ["home"] => {
                self.select_home(None);
                cx.notify();
                "home".to_owned()
            }
            ["dm", index] => {
                let index = index.parse::<usize>().unwrap_or(0);
                let Some(channel) = self.stores.private_channels.get(index).map(|dm| dm.id) else {
                    return "no such dm".to_owned();
                };
                self.select_home(Some(channel));
                cx.notify();
                "dm opened".to_owned()
            }
            ["shop"] => {
                self.open_shop(cx);
                "shop".to_owned()
            }
            ["quests"] => {
                self.open_quests(cx);
                "quests".to_owned()
            }
            ["discover"] => {
                self.open_discover(discover::Place::Servers, cx);
                "discover".to_owned()
            }
            ["apps"] => {
                self.open_discover(discover::Place::Apps, cx);
                "discover apps".to_owned()
            }
            _ => "usage: go home | dm <n> | shop | quests | discover | apps".to_owned(),
        }
    }
}
