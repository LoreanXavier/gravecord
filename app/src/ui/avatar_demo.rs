//! Capture only: `avatar demo` gives a few people, the open server's icon and one profile banner an animated picture
//! (the file named by `GRAVECORD_DEMO_AVATAR_FILE`, an animated WebP; the splash animation of the snapshot when unset),
//! in memory only, so that every surface that plays an animated avatar on hover can be seen at rest and hovered
//! without a real animated account.

use super::{Root, avatar_play};
use dc_state::Id;
use gpui::{Context, Image, ImageFormat};
use serde_json::json;
use std::sync::Arc;

const HASH: &str = "a_demo";
const SIDES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];

impl Root {
    pub(crate) fn script_avatar_demo(&mut self, cx: &mut Context<Self>) -> String {
        let path = std::env::var_os("GRAVECORD_DEMO_AVATAR_FILE").unwrap_or_else(|| {
            "design/snapshot/desktop/bootstrap/splash/videos/connecting.webp".into()
        });
        let Ok(bytes) = std::fs::read(&path) else {
            return format!("avatar demo: cannot read {path:?}");
        };
        let image = Arc::new(Image::from_bytes(ImageFormat::Webp, bytes));
        let mut people: Vec<Id> = Vec::new();
        people.extend(self.stores.current_user.iter().map(|user| user.id));
        people.extend(
            self.stores
                .private_channels
                .iter()
                .filter_map(|channel| self.stores.dm_recipient(channel))
                .map(|user| user.id)
                .take(3),
        );
        people.extend(self.stores.friends().iter().map(|user| user.id).take(3));
        people.dedup();
        for id in &people {
            if let Some(user) = self.stores.users.get_mut(id) {
                user.avatar = Some(HASH.to_owned());
            }
            if let Some(user) = self.stores.current_user.as_mut()
                && user.id == *id
            {
                user.avatar = Some(HASH.to_owned());
            }
            self.demo_picture(format!("avatar:{id}:{HASH}"), &image);
            for side in SIDES {
                let base = format!("https://cdn.discordapp.com/avatars/{id}/{HASH}");
                self.demo_picture(format!("avatar-live:{base}:{side}"), &image);
            }
        }
        // The open server's icon, for the rail.
        let guild = self
            .guild
            .or_else(|| self.stores.guilds.first().map(|guild| guild.id));
        if let Some(id) = guild
            && let Some(known) = self.stores.guilds.iter_mut().find(|known| known.id == id)
        {
            known.icon = Some(HASH.to_owned());
            self.demo_picture(format!("guild:{id}"), &image);
            for side in SIDES {
                self.demo_picture(format!("guild-live:{id}:{HASH}:{side}"), &image);
            }
        }
        // The banner of the first person who is not the account (the profile card of a direct message).
        let other = people.get(1).copied();
        if let Some(user) = other {
            let mut body = self
                .fetched
                .get(&format!("profile:{user}"))
                .cloned()
                .unwrap_or_else(|| json!({"user": {"id": user.to_string()}}));
            body["user_profile"]["banner"] = json!(HASH);
            self.fetched.insert(format!("profile:{user}"), body);
            let base = format!("https://cdn.discordapp.com/banners/{user}/{HASH}");
            self.demo_picture(format!("profile-banner:{base}.png?size=300"), &image);
            for side in SIDES {
                self.demo_picture(format!("banner-live:{base}:{side}"), &image);
            }
        }
        debug_assert!(avatar_play::is_animated(HASH));
        cx.notify();
        format!(
            "avatar demo: {} people, guild {guild:?}, banner of {other:?}",
            people.len()
        )
    }

    fn demo_picture(&mut self, key: String, image: &Arc<Image>) {
        self.images.insert(key.clone(), image.clone());
        self.requested.insert(key);
    }
}
