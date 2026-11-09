//! Equipped nameplates, from modules 628856, 641886 and 746002.
use crate::Id;
use serde_json::Value;

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
