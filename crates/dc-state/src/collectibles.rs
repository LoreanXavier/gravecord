//! Equipped decorations and nameplates, from modules 821956, 628856, 641886 and 746002.
use crate::Id;
use serde_json::Value;

/// Equipped avatar decoration metadata (821956); expiry uses seconds since the epoch.
#[derive(Clone, Debug, PartialEq)]
pub struct AvatarDecoration {
    pub sku_id: Id,
    pub asset: Option<String>,
    pub expires_at: Option<f64>,
}

impl AvatarDecoration {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            sku_id: crate::id(&value["skuId"])
                .or_else(|| value["skuId"].as_u64())
                .or_else(|| crate::id(&value["sku_id"]))
                .or_else(|| value["sku_id"].as_u64())?,
            asset: value["asset"].as_str().map(str::to_owned),
            expires_at: value["expiresAt"]
                .as_f64()
                .or_else(|| value["expires_at"].as_f64()),
        })
    }

    /// Select the guild override before checking expiry (919395/601255).
    pub fn equipped<'a>(
        global: Option<&'a Self>,
        guild: Option<&'a Self>,
        now_ms: f64,
    ) -> Option<&'a Self> {
        guild.or(global).filter(|decoration| {
            !decoration
                .expires_at
                .is_some_and(|end| end * 1000. < now_ms)
        })
    }

    pub fn static_url(&self) -> String {
        format!(
            "https://cdn.discordapp.com/media/v1/collectibles-shop/{}/static",
            self.sku_id
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Nameplate {
    pub sku_id: Id,
    pub label: String,
    pub palette: String,
}

impl Nameplate {
    pub fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            sku_id: crate::id(&value["skuId"])
                .or_else(|| value["skuId"].as_u64())
                .or_else(|| crate::id(&value["sku_id"]))
                .or_else(|| value["sku_id"].as_u64())?,
            label: value["label"].as_str()?.to_owned(),
            palette: value["palette"].as_str()?.to_owned(),
        })
    }

    pub fn static_url(&self) -> String {
        format!(
            "https://cdn.discordapp.com/media/v1/collectibles-shop/{}/static",
            self.sku_id
        )
    }

    pub fn background(&self, light: bool) -> Option<u32> {
        let (dark, light_color) = match self.palette.as_str() {
            "crimson" => (0x900007, 0xE7040F),
            "berry" => (0x893A99, 0xB11FCF),
            "sky" => (0x0080B7, 0x56CCFF),
            "teal" => (0x086460, 0x7DEED7),
            "forest" => (0x2D5401, 0x6AA624),
            "bubble_gum" => (0xDC3E97, 0xF957B3),
            "violet" => (0x730BC8, 0x972FED),
            "cobalt" => (0x0131C2, 0x4278FF),
            "clover" => (0x047B20, 0x63CD5A),
            "lemon" => (0xF6CD12, 0xFED400),
            "white" => (0xFFFFFF, 0xFFFFFF),
            "black" => (0x000000, 0x000000),
            _ => return None,
        };
        Some(if light { light_color } else { dark })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decoration_override_expires_without_revealing_the_global_decoration() {
        let global =
            AvatarDecoration::from_value(&json!({"sku_id":"42", "asset":"global"})).unwrap();
        let guild =
            AvatarDecoration::from_value(&json!({"skuId":43, "expires_at":1, "expiresAt":2}))
                .unwrap();
        assert!(AvatarDecoration::from_value(&json!({"asset":"missing-sku"})).is_none());
        assert_eq!(
            AvatarDecoration::equipped(Some(&global), None, 3000.),
            Some(&global)
        );
        assert_eq!(
            AvatarDecoration::equipped(Some(&global), Some(&guild), 2000.),
            Some(&guild)
        );
        assert_eq!(
            AvatarDecoration::equipped(Some(&global), Some(&guild), 2001.),
            None
        );
        assert_eq!(
            guild.static_url(),
            "https://cdn.discordapp.com/media/v1/collectibles-shop/43/static"
        );
    }

    #[test]
    fn nameplates_validate_metadata_and_keep_unknown_palettes_renderable() {
        assert!(Nameplate::from_value(&json!({"sku_id":"42","palette":"sky"})).is_none());
        let plate =
            Nameplate::from_value(&json!({"skuId":42,"label":"Example","palette":"sky"})).unwrap();
        assert_eq!(
            plate.static_url(),
            "https://cdn.discordapp.com/media/v1/collectibles-shop/42/static"
        );
        assert_eq!(plate.background(false), Some(0x0080B7));
        assert_eq!(plate.background(true), Some(0x56CCFF));
        let unknown = Nameplate {
            palette: "future".into(),
            ..plate
        };
        assert_eq!(unknown.background(false), None);
    }
}
