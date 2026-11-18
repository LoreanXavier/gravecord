//! A saved payment source as the official client reads it and draws it.
//!
//! - The record (module 202613 `createFromServer`): the common fields, then by `type` (`hes` of module 652215) the
//!   card's brand, last four digits and expiry, the email of PayPal, Sofort, SEPA, Przelewy24 and Pix, the username of
//!   Venmo and Cash App, the bank of Przelewy24, EPS and iDEAL.
//! - Its row text (module 219887): the label (`getLabel`, the bold runs of the message), the Default, Subscription
//!   and Invalid tags, and the subtext (`renderSubText`: the card's expiry, else the email or the username).
//! - Its icon type (`typeString` with the card brands of module 812745 `getType`) and the 32 by 22 card picture of
//!   stylesheet `cardIcon_clvei5` (module 509115).
//!
//! The Billing page lists them as module 158935 `o9` does: a divider over each row but the first, the row
//! (`paymentSourceRow_d3LvfJ`: centred, the ends apart, 16px above and below) with the source and an Edit button.

use crate::ui::{Root, color};
use gpui::{AnyElement, FontWeight, IntoElement, div, img, prelude::*, px};
use serde_json::Value;

/// `hes` of module 652215.
pub(in crate::ui) mod kind {
    pub const CARD: u64 = 1;
    pub const PAYPAL: u64 = 2;
    pub const GIROPAY: u64 = 3;
    pub const SOFORT: u64 = 4;
    pub const PRZELEWY24: u64 = 5;
    pub const SEPA_DEBIT: u64 = 6;
    pub const PAYSAFE_CARD: u64 = 7;
    pub const GCASH: u64 = 8;
    pub const GRABPAY_MY: u64 = 9;
    pub const MOMO_WALLET: u64 = 10;
    pub const VENMO: u64 = 11;
    pub const GOPAY_WALLET: u64 = 12;
    pub const KAKAOPAY: u64 = 13;
    pub const BANCONTACT: u64 = 14;
    pub const EPS: u64 = 15;
    pub const IDEAL: u64 = 16;
    pub const CASH_APP: u64 = 17;
    pub const APPLE: u64 = 18;
    pub const PIX: u64 = 20;
    pub const PIX_AUTOMATICO: u64 = 21;
}

/// The bank names of module 887555 (`o`: iDEAL, `j`: EPS).
const IDEAL_BANKS: &[(&str, &str)] = &[
    ("abn_amro", "ABN AMRO"),
    ("asn_bank", "ASN Bank"),
    ("bunq", "Bunq"),
    ("ing ", "ING"),
    ("knab", "Knab"),
    ("n26", "N26"),
    ("nn", "Nationale-Nederlanden"),
    ("rabobank", "Rabobank"),
    ("revolut", "Revolut"),
    ("regiobank", "RegioBank"),
    ("sns_bank", "SNS Bank (De Volksbank)"),
    ("triodos_bank", "Triodos Bank"),
    ("van_lanschot", "Van Lanschot"),
    ("yoursafe", "Yoursafe"),
    ("handelsbanken", "Handelsbanken"),
];
const EPS_BANKS: &[(&str, &str)] = &[
    ("arzte_und_apotheker_bank", "Ärzte- und Apothekerbank"),
    ("austrian_anadi_bank_ag", "Austrian Anadi Bank AG"),
    ("bank_austria", "Bank Austria"),
    ("bankhaus_carl_spangler", "Bankhaus Carl Spängler & Co.AG"),
    (
        "bankhaus_schelhammer_und_schattera_ag",
        "Bankhaus Schelhammer & Schattera AG",
    ),
    ("bawag_psk_ag", "BAWAG P.S.K. AG"),
    ("bks_bank_ag", "BKS Bank AG"),
    ("brull_kallmus_bank_ag", "Brüll Kallmus Bank AG"),
    ("btv_vier_lander_bank", "BTV VIER LÄNDER BANK"),
    (
        "capital_bank_grawe_gruppe_ag",
        "Capital Bank Grawe Gruppe AG",
    ),
    ("dolomitenbank", "Dolomitenbank"),
    ("easybank_ag", "Easybank AG"),
    ("erste_bank_und_sparkassen", "Erste Bank und Sparkassen"),
    (
        "hypo_alpeadriabank_international_ag",
        "Hypo Alpe-Adria-Bank International AG",
    ),
    (
        "hypo_noe_lb_fur_niederosterreich_u_wien",
        "HYPO NOE LB für Niederösterreich u. Wien",
    ),
    (
        "hypo_oberosterreich_salzburg_steiermark",
        "HYPO Oberösterreich, Salzburg, Steiermark",
    ),
    ("hypo_tirol_bank_ag", "Hypo Tirol Bank AG"),
    ("hypo_vorarlberg_bank_ag", "Hypo Vorarlberg Bank AG"),
    (
        "hypo_bank_burgenland_aktiengesellschaft",
        "HYPO-BANK BURGENLAND Aktiengesellschaft",
    ),
    ("marchfelder_bank", "Marchfelder Bank"),
    ("oberbank_ag", "Oberbank AG"),
    (
        "raiffeisen_bankengruppe_osterreich",
        "Raiffeisen Bankengruppe Österreich",
    ),
    ("schoellerbank_ag", "Schoellerbank AG"),
    ("sparda_bank_wien", "Sparda-Bank Wien"),
    ("volksbank_gruppe", "Volksbank Gruppe"),
    ("volkskreditbank_ag", "Volkskreditbank AG"),
    ("vr_bank_braunau", "VR-Bank Braunau"),
];

/// The card brands module 812745 knows (`Types`), as `getType` matches them: the brand with everything but letters,
/// digits and `_` removed, upper-cased, must be one of the keys.
const CARD_TYPES: &[(&str, &str)] = &[
    ("VISA", "visa"),
    ("DISCOVER", "discover"),
    ("MASTERCARD", "mastercard"),
    ("AMEX", "amex"),
    ("PAYPAL", "paypal"),
    ("DINERS", "diners"),
    ("JCB", "jcb"),
    ("UNIONPAY", "unionpay"),
];

/// The pictures of `cardIcon_clvei5` by type (dark theme), the plain card for any other.
const CARD_ART: &[(&str, &str)] = &[
    ("visa", "4c049942fdde71f8"),
    ("amex", "e415725e0e3a87fb"),
    ("discover", "9a5413553cab204a"),
    ("mastercard", "ba865ff6773d1876"),
    ("paypal", "0b87743361cfb797"),
    ("sofort", "eb22e43f6f710d3b"),
    ("przelewy24", "3d961164938e62d7"),
    ("giropay", "fcb798c4bf712284"),
    ("paysafecard", "0b47f9360f12b1fa"),
    ("gcash", "7e5579c4c73ca390"),
    ("grabpay", "224f56365db6763e"),
    ("momo_wallet", "0dbebb1dde8de002"),
    ("venmo", "9bb2cc8906a18ba2"),
    ("kakaopay", "2a5fa2783cb39764"),
    ("gopay_wallet", "637bf6beddb5589b"),
    ("bancontact", "ce90dd2b5978b623"),
    ("eps", "3f80791089be55d4"),
    ("cash_app", "3de97fb87787f20b"),
    ("apple", "824341c879ac6d59"),
];
/// `.cardIcon_clvei5:before`: the plain card under every type without its own picture.
const CARD_PLAIN: &str = "22f7eeb9ea9904c1";

/// The fields module 219887 reads of a source record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct PaymentSource {
    pub id: String,
    pub kind: u64,
    pub invalid: bool,
    pub is_default: bool,
    pub brand: String,
    pub last4: String,
    pub expires_month: u32,
    pub expires_year: u32,
    pub email: String,
    pub username: String,
    pub bank: Option<String>,
}

impl PaymentSource {
    /// `createFromServer`; a type the client does not know is no record (`xb` throws).
    pub(in crate::ui) fn from_server(value: &Value) -> Option<Self> {
        let kind = value["type"].as_u64()?;
        if !(1..=21).contains(&kind) {
            return None;
        }
        let text = |field: &str| value[field].as_str().unwrap_or_default().to_owned();
        let mut source = PaymentSource {
            id: text("id"),
            kind,
            invalid: value["invalid"].as_bool().unwrap_or(false),
            is_default: value["default"].as_bool().unwrap_or(false),
            ..Default::default()
        };
        match kind {
            kind::CARD => {
                source.brand = text("brand");
                source.last4 = text("last_4");
                source.expires_month = value["expires_month"].as_u64().unwrap_or(0) as u32;
                source.expires_year = value["expires_year"].as_u64().unwrap_or(0) as u32;
            }
            kind::PAYPAL | kind::SOFORT | kind::SEPA_DEBIT | kind::PIX | kind::PIX_AUTOMATICO => {
                source.email = text("email")
            }
            kind::PRZELEWY24 => {
                source.email = text("email");
                source.bank = value["bank"].as_str().map(str::to_owned);
            }
            kind::EPS | kind::IDEAL => source.bank = value["bank"].as_str().map(str::to_owned),
            kind::VENMO | kind::CASH_APP => source.username = text("username"),
            _ => {}
        }
        Some(source)
    }

    /// `isStripeLinkBankAccount`: a Link bank account saved as a card.
    pub(in crate::ui) fn is_link_bank(&self) -> bool {
        self.kind == kind::CARD && self.brand == "link" && self.last4 == "0000"
    }

    /// `typeString`: the icon type.
    pub(in crate::ui) fn type_string(&self) -> &'static str {
        match self.kind {
            kind::PAYPAL => "paypal",
            kind::SOFORT | kind::SEPA_DEBIT => "sofort",
            kind::CARD if self.is_link_bank() => "bank",
            kind::CARD => card_type(&self.brand),
            kind::GIROPAY => "giropay",
            kind::PRZELEWY24 => "przelewy24",
            kind::PAYSAFE_CARD => "paysafecard",
            kind::GCASH => "gcash",
            kind::GRABPAY_MY => "grabpay",
            kind::MOMO_WALLET => "momo_wallet",
            kind::VENMO => "venmo",
            kind::KAKAOPAY => "kakaopay",
            kind::GOPAY_WALLET => "gopay_wallet",
            kind::BANCONTACT => "bancontact",
            kind::EPS => "eps",
            kind::IDEAL => "ideal",
            kind::CASH_APP => "cash_app",
            kind::APPLE => "apple",
            kind::PIX | kind::PIX_AUTOMATICO => "pix",
            _ => "unknown",
        }
    }

    /// `getLabel` as runs of text, the bold ones `$b` of the message. A SEPA debit is the Sofort record there, so it
    /// reads Sofort; the 3DS wallet has no label and reads Unknown.
    pub(in crate::ui) fn label(&self) -> Vec<(String, bool)> {
        let plain = |key: &str| vec![(dc_intl::t(key).to_owned(), false)];
        let runs = |key: &str, args: &dc_intl::Args<'_>| {
            dc_intl::format_runs(key, args)
                .into_iter()
                .map(|(text, tag)| (text, tag.is_some()))
                .collect::<Vec<_>>()
        };
        match self.kind {
            kind::CARD if self.is_link_bank() => {
                // `extractTimestamp(id)` as `toLocaleDateString()` (en-US: 3/9/2026), local time.
                let millis = (self.id.parse::<u64>().unwrap_or(0) >> 22) as i64 + 1_420_070_400_000;
                let date = chrono::DateTime::from_timestamp_millis(millis)
                    .map(|time| {
                        time.with_timezone(&chrono::Local)
                            .format("%-m/%-d/%Y")
                            .to_string()
                    })
                    .unwrap_or_default();
                vec![(dc_intl::format("ixNwPL", &[("date", date.into())]), false)]
            }
            kind::CARD => runs(
                "o/dsrL",
                &[
                    ("brand", capitalize(&self.brand).into()),
                    ("last_4", self.last4.clone().into()),
                ],
            ),
            kind::PAYPAL => plain("2dgEq+"),
            kind::SOFORT | kind::SEPA_DEBIT => plain("edKX/1"),
            kind::GIROPAY => plain("y+0MQZ"),
            kind::PRZELEWY24 => plain("u25uL0"),
            kind::PAYSAFE_CARD => plain("boznHN"),
            kind::GCASH => plain("PjehcF"),
            kind::GRABPAY_MY => plain("T5davE"),
            kind::MOMO_WALLET => plain("J0A1Vk"),
            kind::VENMO => plain("jYOezc"),
            kind::KAKAOPAY => plain("CSVexi"),
            kind::GOPAY_WALLET => plain("43J8JK"),
            kind::BANCONTACT => plain("1ITkfq"),
            kind::IDEAL => match &self.bank {
                None => plain("nSbwqC"),
                Some(bank) => runs("9kUlRU", &[("bank", bank_name(IDEAL_BANKS, bank).into())]),
            },
            kind::EPS => runs(
                "hSPoZw",
                &[(
                    "bank",
                    bank_name(EPS_BANKS, self.bank.as_deref().unwrap_or_default()).into(),
                )],
            ),
            kind::CASH_APP => plain("+rbTmL"),
            kind::APPLE => plain("RFi12i"),
            kind::PIX => plain("JG3WQU"),
            kind::PIX_AUTOMATICO => plain("JkKNss"),
            _ => plain("jdPblk"),
        }
    }

    /// `renderSubText`: "Expires {month} {year}" for a card (the short month name), the email of PayPal, Sofort and
    /// Przelewy24, "@username" for Venmo, the username for Cash App.
    pub(in crate::ui) fn subtext(&self) -> Option<String> {
        match self.kind {
            kind::CARD if !self.is_link_bank() => Some(dc_intl::format(
                "8rTTuf",
                &[
                    ("month", short_month(self.expires_month).into()),
                    ("year", (self.expires_year as i64).into()),
                ],
            )),
            kind::PAYPAL | kind::SOFORT | kind::SEPA_DEBIT | kind::PRZELEWY24 => {
                Some(self.email.clone())
            }
            kind::VENMO => Some(format!("@{}", self.username)),
            kind::CASH_APP => Some(self.username.clone()),
            _ => None,
        }
    }

    /// The card picture's address, or `None` for the types module 812745 draws with an icon of its own (a bank, a
    /// gift card, Pix and iDEAL).
    pub(in crate::ui) fn art(&self) -> Option<String> {
        let kind = self.type_string();
        if matches!(kind, "bank" | "pix" | "ideal") {
            // GAP: `BankIcon`, `PaymentTypePixLogoIcon` and `PaymentTypeIDealWeroIcon` are not in the icon set.
            return None;
        }
        let hash = CARD_ART
            .iter()
            .find(|(name, _)| *name == kind)
            .map_or(CARD_PLAIN, |(_, hash)| hash);
        Some(format!("https://discord.com/assets/{hash}.svg"))
    }
}

/// `getType` of module 812745.
fn card_type(brand: &str) -> &'static str {
    let key: String = brand
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>()
        .to_uppercase();
    CARD_TYPES
        .iter()
        .find(|(name, _)| *name == key)
        .map_or("unknown", |(_, kind)| kind)
}

/// `$G` of module 23339: the first letter upper-cased.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn bank_name(table: &[(&str, &'static str)], bank: &str) -> String {
    table.iter().find(|(key, _)| *key == bank).map_or_else(
        || dc_intl::t("jdPblk").to_owned(),
        |(_, name)| (*name).to_owned(),
    )
}

/// `eS` of module 739508 in en-US: `toLocaleString(locale, {month: "short"})`.
fn short_month(month: u32) -> &'static str {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS[(month.clamp(1, 12) - 1) as usize]
}

/// The sources of a `GET /users/@me/billing/payment-sources` answer, those the client knows.
pub(in crate::ui) fn sources_of(body: Option<&Value>) -> Vec<PaymentSource> {
    body.and_then(Value::as_array)
        .map(|list| list.iter().filter_map(PaymentSource::from_server).collect())
        .unwrap_or_default()
}

/// `S` of module 219887: a tag in `body-xs/semibold`, upper-case, 14px high with 4px at the sides, 8px from what
/// is before it (`indicator_OrFQq1`).
fn indicator(text: &str, background: &'static str) -> AnyElement {
    div()
        .ml(px(8.))
        .h(px(14.))
        .px(px(4.))
        .rounded(px(3.))
        .flex()
        .items_center()
        .bg(color(background))
        .text_color(color("--white"))
        .text_size(px(12.))
        .line_height(px(14.))
        .font_weight(FontWeight(600.))
        .child(text.to_uppercase())
        .into_any_element()
}

impl Root {
    /// Module 219887 with `showPaymentSourceIcon`, `showLabels` and `showSubtext` (module 158935 `o9`): the 32 by 22
    /// picture, then 14px on (`descriptionWrapper_OrFQq1`) the label in `body-md/semibold` with its tags and the
    /// subtext 4px under it in `body-sm/normal`, `text-subtle`.
    pub(in crate::ui) fn payment_source_view(
        &self,
        source: &PaymentSource,
        subscription: bool,
    ) -> AnyElement {
        let label = div()
            .text_size(px(16.))
            .line_height(px(20.))
            .text_color(color("--text-default"))
            .flex()
            .flex_wrap()
            .children(source.label().into_iter().map(|(text, bold)| {
                div()
                    .font_weight(FontWeight(if bold { 700. } else { 600. }))
                    .child(text)
            }));
        let picture = source
            .art()
            .and_then(|url| self.images.get(&url).cloned())
            .map(|art| img(art).size_full().rounded(px(2.)));
        div()
            .flex()
            .items_center()
            .child(div().flex_none().w(px(32.)).h(px(22.)).children(picture))
            .child(
                div()
                    .ml(px(14.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .child(label)
                            .children(
                                source
                                    .is_default
                                    .then(|| indicator(dc_intl::t("bBvAEH"), "--primary-500")),
                            )
                            .children(
                                subscription
                                    .then(|| indicator(dc_intl::t("YCrcPL"), "--brand-500")),
                            )
                            .children(
                                source
                                    .invalid
                                    .then(|| indicator(dc_intl::t("851k93"), "--red-400")),
                            ),
                    )
                    .children(source.subtext().map(|text| {
                        div()
                            .mt(px(4.))
                            .text_size(px(14.))
                            .line_height(px(18.))
                            .text_color(color("--text-subtle"))
                            .child(text)
                    })),
            )
            .into_any_element()
    }

    /// The pictures of the saved sources, asked for once they are read.
    pub(in crate::ui) fn request_payment_source_art(&mut self) {
        for source in sources_of(self.nitro.sources.as_ref()) {
            if let Some(url) = source.art()
                && !self.images.contains_key(&url)
            {
                self.request_image(url.clone(), url);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn text(runs: &[(String, bool)]) -> String {
        runs.iter().map(|(text, _)| text.as_str()).collect()
    }

    #[test]
    fn a_card_reads_its_brand_digits_and_expiry() {
        let card = PaymentSource::from_server(&json!({
            "id": "1", "type": 1, "brand": "visa", "last_4": "4242", "expires_month": 3,
            "expires_year": 2029, "default": true, "invalid": false,
        }))
        .unwrap();
        assert_eq!(text(&card.label()), "Visa ending in 4242");
        assert_eq!(card.label().last(), Some(&("4242".to_owned(), true)));
        assert_eq!(card.subtext().as_deref(), Some("Expires Mar 2029"));
        assert_eq!(card.type_string(), "visa");
        assert!(card.is_default);
        assert_eq!(
            card.art().as_deref(),
            Some("https://discord.com/assets/4c049942fdde71f8.svg")
        );
    }

    #[test]
    fn every_kind_has_the_label_of_module_219887() {
        let label = |value: Value| text(&PaymentSource::from_server(&value).unwrap().label());
        assert_eq!(label(json!({"type": 2, "email": "a@b.c"})), "PayPal");
        assert_eq!(label(json!({"type": 4})), "Sofort");
        assert_eq!(label(json!({"type": 3})), "Giropay");
        assert_eq!(label(json!({"type": 5})), "Przelewy24");
        assert_eq!(label(json!({"type": 7})), "Paysafecard");
        assert_eq!(label(json!({"type": 11})), "Venmo");
        assert_eq!(label(json!({"type": 16})), "iDEAL");
        assert_eq!(
            label(json!({"type": 16, "bank": "rabobank"})),
            "iDEAL with Rabobank"
        );
        assert_eq!(
            label(json!({"type": 15, "bank": "bank_austria"})),
            "EPS with Bank Austria"
        );
        assert_eq!(
            label(json!({"type": 15, "bank": "nope"})),
            "EPS with Unknown"
        );
        assert_eq!(label(json!({"type": 17})), "Cash App Pay");
        assert_eq!(label(json!({"type": 19})), "Unknown");
        assert!(PaymentSource::from_server(&json!({"type": 99})).is_none());
    }

    #[test]
    fn the_subtext_is_the_email_or_the_username() {
        let source = |value: Value| PaymentSource::from_server(&value).unwrap();
        assert_eq!(
            source(json!({"type": 2, "email": "me@x.y"}))
                .subtext()
                .as_deref(),
            Some("me@x.y")
        );
        assert_eq!(
            source(json!({"type": 11, "username": "ven"}))
                .subtext()
                .as_deref(),
            Some("@ven")
        );
        assert_eq!(
            source(json!({"type": 17, "username": "cash"}))
                .subtext()
                .as_deref(),
            Some("cash")
        );
        assert_eq!(source(json!({"type": 3})).subtext(), None);
    }

    #[test]
    fn card_brands_map_like_get_type() {
        assert_eq!(card_type("American Express"), "unknown");
        assert_eq!(card_type("amex"), "amex");
        assert_eq!(card_type("MasterCard"), "mastercard");
        assert_eq!(card_type("diners"), "diners");
        assert_eq!(card_type(""), "unknown");
        let link = PaymentSource::from_server(
            &json!({"id": "1", "type": 1, "brand": "link", "last_4": "0000"}),
        )
        .unwrap();
        assert_eq!(link.type_string(), "bank");
        assert_eq!(link.subtext(), None);
        assert!(text(&link.label()).starts_with("Bank Account added on "));
    }
}
