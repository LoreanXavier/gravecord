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

    /// Payment source picker dropdown & trial notice (`Ay` of module 637141).
    pub(in crate::ui) fn payment_source_picker_layer(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let picker = self.payment_source_picker.as_ref()?;
        let hide_pii = picker.hide_personal_info;
        let selected_id = picker.selected_id.as_deref();
        let selected_src = picker
            .sources
            .iter()
            .find(|s| Some(s.id.as_str()) == selected_id)
            .or_else(|| picker.sources.first());
        let selected_label = selected_src
            .map(|s| picker_option_label(s, hide_pii))
            .unwrap_or_else(|| dc_intl::t("eQ2bLp").to_owned());
        let notice = selected_src.and_then(|s| picker_trial_notice(s, picker.is_trial));
        let dropdown_open = picker.dropdown_open;

        let unsupported = picker.unsupported_card_variant;
        let notice_6151 = picker.notice_message.clone();
        let header_title = if unsupported {
            dc_intl::t("u+Cw58")
        } else {
            dc_intl::tr("Payment Methods")
        };
        let empty_add_label = if unsupported {
            dc_intl::t("rNF29q")
        } else {
            dc_intl::t("eQ2bLp")
        };

        let dialog = div()
            .id("payment-source-picker-modal")
            .occlude()
            .w(px(460.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-surface-high"))
            .shadow(vec![gpui::BoxShadow {
                color: gpui::hsla(0., 0., 0., 0.3),
                offset: gpui::point(px(0.), px(12.)),
                blur_radius: px(28.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(24.))
                    .pt(px(20.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(18.))
                            .line_height(px(24.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(header_title),
                    )
                    .child(
                        div()
                            .id("payment-picker-close")
                            .size(px(32.))
                            .rounded(px(8.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(crate::ui::icon(
                                "modal_close",
                                18.,
                                "--interactive-icon-default",
                            ))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.payment_source_picker = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .px(px(24.))
                    .pb(px(20.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .when(picker.sources.is_empty(), |col| {
                        col.child(
                            div()
                                .id("payment-picker-add-empty")
                                .min_h(px(if unsupported { 56. } else { 40. }))
                                .px(px(14.))
                                .w_full()
                                .rounded(px(8.))
                                .when(unsupported, |d| {
                                    d.border_1()
                                        .border_color(color("--border-subtle"))
                                        .bg(color("--background-mod-subtle"))
                                        .hover(|s| s.bg(color("--background-mod-normal")))
                                        .justify_between()
                                })
                                .when(!unsupported, |d| {
                                    d.bg(color("--control-primary-background-default"))
                                        .justify_center()
                                })
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .font_weight(FontWeight(500.))
                                .text_color(if unsupported {
                                    color("--text-strong")
                                } else {
                                    color("--white")
                                })
                                .child(empty_add_label)
                                .when(unsupported, |d| {
                                    d.child(crate::ui::icon(
                                        "shop_arrow_right",
                                        16.,
                                        "--text-muted",
                                    ))
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.payment_source_picker = None;
                                    this.billing_address_modal = Some(
                                        crate::ui::billing_address::BillingAddressModal::demo(
                                            "US", false,
                                        ),
                                    );
                                    cx.notify();
                                })),
                        )
                    })
                    .when_some(notice_6151, |col, msg| {
                        // Module 6151 `.noticeMessageContainer_sFISJa`.
                        col.child(
                            div()
                                .p(px(10.))
                                .rounded(px(8.))
                                .bg(color("--background-mod-subtle"))
                                .border_1()
                                .border_color(color("--border-subtle"))
                                .flex()
                                .items_start()
                                .gap(px(8.))
                                .child(crate::ui::icon("circle_info", 16., "--text-feedback-info"))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(12.))
                                        .line_height(px(16.))
                                        .text_color(color("--text-feedback-info"))
                                        .child(msg),
                                ),
                        )
                    })
                    .when(!picker.sources.is_empty(), |col| {
                        col.child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(12.))
                                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                    if let Some(p) = this.payment_source_picker.as_mut()
                                        && p.dropdown_open
                                    {
                                        p.dropdown_open = false;
                                        cx.notify();
                                    }
                                }))
                                .child(
                                    div()
                                        .id("payment-picker-select")
                                        .h(px(44.))
                                        .px(px(12.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--input-border-default"))
                                        .bg(color("--input-background-default"))
                                        .cursor_pointer()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(px(14.))
                                                .font_weight(FontWeight(500.))
                                                .text_color(color("--text-strong"))
                                                .child(selected_label),
                                        )
                                        .child(crate::ui::icon(
                                            "chevron_down",
                                            16.,
                                            "--interactive-icon-default",
                                        ))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if let Some(p) = this.payment_source_picker.as_mut() {
                                                p.dropdown_open = !p.dropdown_open;
                                            }
                                            cx.notify();
                                        })),
                                )
                                .when(dropdown_open, |col| {
                                    col.child(
                                        div()
                                            .id("payment-picker-list")
                                            // `maxOptionsVisible` 5 (module 691885): five 36px rows, 2px gaps, 4px padding.
                                            .max_h(px(196.))
                                            .overflow_y_scroll()
                                            .p(px(4.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-base-lower"))
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.))
                                            .on_click(|_, _, cx| cx.stop_propagation())
                                            .children(picker.sources.iter().enumerate().map(
                                                |(i, src)| {
                                                    let is_sel = selected_src
                                                        .is_some_and(|s| s.id == src.id);
                                                    let label = picker_option_label(src, hide_pii);
                                                    let id_str = src.id.clone();
                                                    div()
                                                        .id(gpui::SharedString::from(format!(
                                                            "payment-picker-opt-{i}"
                                                        )))
                                                        .flex_none()
                                                        .h(px(36.))
                                                        .px(px(10.))
                                                        .rounded(px(6.))
                                                        .when(is_sel, |d| {
                                                            d.bg(color("--background-mod-strong"))
                                                        })
                                                        .hover(|s| {
                                                            s.bg(color("--background-mod-subtle"))
                                                        })
                                                        .cursor_pointer()
                                                        .flex()
                                                        .items_center()
                                                        .justify_between()
                                                        .text_size(px(14.))
                                                        .text_color(color("--text-strong"))
                                                        .child(label)
                                                        .when(is_sel, |d| {
                                                            d.child(crate::ui::icon(
                                                    "select_check",
                                                    14.,
                                                    "--control-primary-background-default",
                                                ))
                                                        })
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                if let Some(p) = this
                                                                    .payment_source_picker
                                                                    .as_mut()
                                                                {
                                                                    p.selected_id =
                                                                        Some(id_str.clone());
                                                                    p.dropdown_open = false;
                                                                }
                                                                cx.notify();
                                                            },
                                                        ))
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .flex_none()
                                                    .my(px(4.))
                                                    .h(px(1.))
                                                    .bg(color("--border-subtle")),
                                            )
                                            .child(
                                                div()
                                                    .id(NEW_PAYMENT_SOURCE_ID)
                                                    .flex_none()
                                                    .h(px(36.))
                                                    .px(px(10.))
                                                    .rounded(px(6.))
                                                    .hover(|s| {
                                                        s.bg(color("--background-mod-subtle"))
                                                    })
                                                    .cursor_pointer()
                                                    .flex()
                                                    .items_center()
                                                    .text_size(px(14.))
                                                    .font_weight(FontWeight(500.))
                                                    .text_color(color("--text-link"))
                                                    .child(dc_intl::t("eQ2bLp"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.payment_source_picker = None;
                                                        let _ = this.requests.send(
                                                            payment_source_creation_context_request(
                                                            ),
                                                        );
                                                        this.add_payment_method_modal = Some(
                                                            Box::new(AddPaymentMethodState::new()),
                                                        );
                                                        cx.notify();
                                                    })),
                                            ),
                                    )
                                }),
                        )
                    })
                    .when_some(notice, |col, (text, is_error)| {
                        col.child(
                            div()
                                .p(px(10.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(if is_error {
                                    color("--text-feedback-critical")
                                } else {
                                    color("--border-subtle")
                                })
                                .bg(color("--background-base-lower"))
                                .text_size(px(12.))
                                .line_height(px(16.))
                                .text_color(if is_error {
                                    color("--text-feedback-critical")
                                } else {
                                    color("--text-muted")
                                })
                                .child(text),
                        )
                    }),
            );

        Some(
            gpui::deferred(
                div()
                    .id("payment-picker-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(gpui::rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.payment_source_picker = None;
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_payment_picker(
        &mut self,
        arg: &str,
        cx: &mut gpui::Context<Self>,
    ) -> String {
        use serde_json::json;
        let trimmed = arg.trim();
        let unsupported = matches!(trimmed, "unsupported" | "unsupported-empty");
        let sources: Vec<PaymentSource> = if trimmed == "unsupported-empty" {
            Vec::new()
        } else {
            [
                json!({"id": "1", "type": 1, "brand": "visa", "last_4": "4242", "expires_month": 3, "expires_year": 2029, "default": true}),
                json!({"id": "2", "type": 2, "email": "billing@example.com"}),
                json!({"id": "3", "type": 16, "bank": "rabobank"}),
                json!({"id": "4", "type": 15, "bank": "bank_austria"}),
                json!({"id": "5", "type": 20}),
                json!({"id": "6", "type": 21}),
            ]
            .iter()
            .filter_map(PaymentSource::from_server)
            .collect()
        };
        self.payment_source_picker = Some(PaymentSourcePickerState {
            sources,
            selected_id: Some("1".to_owned()),
            hide_personal_info: trimmed == "streamer",
            is_trial: !unsupported,
            dropdown_open: trimmed != "unsupported-empty",
            unsupported_card_variant: unsupported,
            notice_message: unsupported.then(|| {
                "Your current payment method isn't supported for this purchase. Please add a new payment option.".to_owned()
            }),
        });
        cx.notify();
        format!("payment-picker: {arg}")
    }
}

/// Module 362111 (`671334`): steps of the standalone "Add a new payment method" modal (`eQ2bLp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(in crate::ui) enum AddPaymentStep {
    PaymentElement,
    CreditCard,
    ExternalMethod,
    Address,
    AwaitingAuth,
}

/// Module 362111: selectable payment method types in `AddPaymentMethodState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(in crate::ui) enum AddPaymentKind {
    Card,
    PayPal,
    Venmo,
    CashApp,
    Ideal,
    Pix,
}

impl AddPaymentKind {
    pub(in crate::ui) const ALL: [Self; 6] = [
        Self::Card,
        Self::PayPal,
        Self::Venmo,
        Self::CashApp,
        Self::Ideal,
        Self::Pix,
    ];

    pub(in crate::ui) fn label(self) -> &'static str {
        match self {
            Self::Card => dc_intl::t("ei5/p8"),
            Self::PayPal => dc_intl::t("QQBAos"),
            Self::Venmo => dc_intl::t("jYOezc"),
            Self::CashApp => dc_intl::t("+rbTmL"),
            Self::Ideal => dc_intl::t("nSbwqC"),
            Self::Pix => dc_intl::t("JG3WQU"),
        }
    }

    pub(in crate::ui) fn connect_cta(self) -> &'static str {
        match self {
            Self::PayPal => dc_intl::t("Djzd7L"),
            Self::Venmo => dc_intl::t("4KoTLM"),
            Self::CashApp => dc_intl::t("9ALP8w"),
            _ => dc_intl::t("PDTjLN"),
        }
    }
}

/// Module 362111 (`AddPaymentMethodModal`, CSS `998973`/`24404`): standalone add payment method state.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub(in crate::ui) struct AddPaymentMethodState {
    pub step: AddPaymentStep,
    pub selected_kind: AddPaymentKind,
    pub card_number: String,
    pub exp_date: String,
    pub cvc: String,
    pub cardholder_name: String,
    pub country_locked: bool,
    pub error_message: Option<String>,
    pub submitting: bool,
}

impl AddPaymentMethodState {
    pub(in crate::ui) fn new() -> Self {
        Self {
            step: AddPaymentStep::PaymentElement,
            selected_kind: AddPaymentKind::Card,
            card_number: String::new(),
            exp_date: String::new(),
            cvc: String::new(),
            cardholder_name: String::new(),
            country_locked: false,
            error_message: None,
            submitting: false,
        }
    }

    /// Module 362111 (`671334`): validates credit card fields returning official intl error keys.
    pub(in crate::ui) fn validate_card(&self) -> Result<(), &'static str> {
        let digits: String = self
            .card_number
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        if digits.is_empty() {
            return Err(dc_intl::t("eOIfuy"));
        }
        if digits.len() < 13 || digits.len() > 19 {
            return Err(dc_intl::t("x4pWtJ"));
        }
        let exp = self.exp_date.trim();
        if !exp.contains('/') || exp.len() < 4 {
            return Err(dc_intl::t("9/zZdl"));
        }
        let cvc_digits = self.cvc.chars().filter(|c| c.is_ascii_digit()).count();
        if !(3..=4).contains(&cvc_digits) {
            return Err(dc_intl::t("ro4isZ"));
        }
        if self.cardholder_name.trim().is_empty() {
            return Err(dc_intl::t("lIkVsi"));
        }
        Ok(())
    }
}

/// Module 362111: `GET /users/@me/billing/payment-source-creation-context`.
pub(in crate::ui) fn payment_source_creation_context_request() -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some("billing:creation-context".to_owned()),
        method: reqwest::Method::GET,
        route: "/users/@me/billing/payment-source-creation-context".to_owned(),
        body: None,
    }
}

/// Module 362111: `POST /users/@me/billing/payment-sources/validate-billing-address`.
#[allow(dead_code)]
pub(in crate::ui) fn validate_billing_address_request(
    name: &str,
    line_1: &str,
    city: &str,
    postal_code: &str,
    country: &str,
) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some("billing:validate-address".to_owned()),
        method: reqwest::Method::POST,
        route: "/users/@me/billing/payment-sources/validate-billing-address".to_owned(),
        body: Some(serde_json::json!({
            "billing_address": {
                "name": name,
                "line_1": line_1,
                "city": city,
                "postal_code": postal_code,
                "country": country,
            }
        })),
    }
}

/// Module 362111: `POST /users/@me/billing/payment-sources?location={location}`.
#[allow(dead_code)]
pub(in crate::ui) fn create_payment_source_request(
    location: &str,
    payment_gateway: u32,
    token: &str,
    billing_address_token: Option<&str>,
    is_default: bool,
) -> crate::net::Request {
    let mut body = serde_json::json!({
        "payment_gateway": payment_gateway,
        "token": token,
        "default": is_default,
    });
    if let Some(bat) = billing_address_token {
        body["billing_address_token"] = serde_json::json!(bat);
    }
    crate::net::Request::Rest {
        key: Some("billing:create-payment-source".to_owned()),
        method: reqwest::Method::POST,
        route: format!("/users/@me/billing/payment-sources?location={location}"),
        body: Some(body),
    }
}

/// Module 302810: 4-step 3DS payment authentication state (`0=Required`, `1=Failed`, `2=Success`, `3=Canceled`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(in crate::ui) enum PaymentAuthStep {
    Required = 0,
    Failed = 1,
    Success = 2,
    Canceled = 3,
}

/// Module 302810 (`PAYMENT_AUTHENTICATION_MODAL`): 3DS payment authentication modal state.
#[derive(Clone, Debug)]
pub(in crate::ui) struct PaymentAuthModalState {
    pub payment_id: String,
    pub price_formatted: String,
    pub item_description: String,
    pub step: PaymentAuthStep,
    pub awaiting_auth: bool,
}

/// Module 302810 (`781300.N`): `POST /users/@me/billing/payments/{payment_id}/void`.
pub(in crate::ui) fn payment_void_request(payment_id: &str) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("billing:payment-void:{payment_id}")),
        method: reqwest::Method::POST,
        route: format!("/users/@me/billing/payments/{payment_id}/void"),
        body: None,
    }
}

/// Module 302810 (`558740.ap`): `GET /users/@me/billing/stripe/payment-intents/payments/{payment_id}`.
pub(in crate::ui) fn payment_intent_request(payment_id: &str) -> crate::net::Request {
    crate::net::Request::Rest {
        key: Some(format!("billing:payment-intent:{payment_id}")),
        method: reqwest::Method::GET,
        route: format!("/users/@me/billing/stripe/payment-intents/payments/{payment_id}"),
        body: None,
    }
}

impl crate::ui::Root {
    /// Renders module 302810 (`PAYMENT_AUTHENTICATION_MODAL`): 4-step 3DS authentication dialog.
    pub(in crate::ui) fn payment_auth_modal_layer(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        use crate::ui::{color, icon};
        use dc_intl::Arg;
        use gpui::{FontWeight, IntoElement, div, prelude::*, px, rgba};
        use std::borrow::Cow;

        let state = self.payment_auth_modal.as_ref()?;
        let (title, message, badge_icon, badge_bg, badge_fg) = match state.step {
            PaymentAuthStep::Required => (
                dc_intl::t("N65ZrW").to_owned(),
                dc_intl::format(
                    "RoxWET",
                    &[
                        ("price", Arg::Str(Cow::Borrowed(&state.price_formatted))),
                        ("item", Arg::Str(Cow::Borrowed(&state.item_description))),
                    ],
                ),
                "role_shield",
                rgba(0x5865f226),
                "--brand-400",
            ),
            PaymentAuthStep::Success => (
                dc_intl::t("DPgrRk").to_owned(),
                dc_intl::t("PZskfs").to_owned(),
                "circle_check",
                rgba(0x23a55a26),
                "--status-positive",
            ),
            PaymentAuthStep::Canceled => (
                dc_intl::t("xypuIv").to_owned(),
                dc_intl::format(
                    "j0tI/b",
                    &[("item", Arg::Str(Cow::Borrowed(&state.item_description)))],
                ),
                "warning_triangle",
                rgba(0xda373c26),
                "--status-danger",
            ),
            PaymentAuthStep::Failed => (
                dc_intl::t("uEoQdA").to_owned(),
                dc_intl::t("BAr3rB").to_owned(),
                "warning_triangle",
                rgba(0xda373c26),
                "--status-danger",
            ),
        };
        let payment_id = state.payment_id.clone();
        let step = state.step;
        let awaiting = state.awaiting_auth;

        let actions = if step == PaymentAuthStep::Required {
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(px(8.))
                .child(
                    div()
                        .id("payment-auth-cancel")
                        .h(px(38.))
                        .px(px(16.))
                        .rounded(px(8.))
                        .bg(color("--control-secondary-background-default"))
                        .hover(|s| s.bg(color("--control-secondary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(500.))
                        .text_color(color("--control-secondary-text-default"))
                        .child(dc_intl::t("2NeVoY"))
                        .on_click(cx.listener({
                            let pid = payment_id.clone();
                            move |this, _, _, cx| {
                                let _ = this.requests.send(payment_void_request(&pid));
                                if let Some(m) = this.payment_auth_modal.as_mut() {
                                    m.step = PaymentAuthStep::Canceled;
                                    m.awaiting_auth = false;
                                }
                                cx.notify();
                            }
                        })),
                )
                .child(
                    div()
                        .id("payment-auth-continue")
                        .h(px(38.))
                        .px(px(16.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .when(awaiting, |d| d.opacity(0.65))
                        .when(!awaiting, |d| {
                            d.cursor_pointer()
                                .hover(|s| s.bg(color("--control-primary-background-hover")))
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("VAqBoE"))
                        .on_click(cx.listener({
                            let pid = payment_id.clone();
                            move |this, _, _, cx| {
                                if let Some(m) = this.payment_auth_modal.as_mut() {
                                    if m.awaiting_auth {
                                        return;
                                    }
                                    m.awaiting_auth = true;
                                }
                                let _ = this.requests.send(payment_intent_request(&pid));
                                cx.notify();
                            }
                        })),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .justify_end()
                .child(
                    div()
                        .id("payment-auth-close")
                        .h(px(38.))
                        .px(px(18.))
                        .rounded(px(8.))
                        .bg(color("--control-primary-background-default"))
                        .hover(|s| s.bg(color("--control-primary-background-hover")))
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(14.))
                        .font_weight(FontWeight(600.))
                        .text_color(color("--white"))
                        .child(dc_intl::t("cpT0Cq"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.payment_auth_modal = None;
                            cx.notify();
                        })),
                )
                .into_any_element()
        };

        let dialog = div()
            .id("payment-auth-dialog")
            .w(px(440.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(20.))
                    .pt(px(20.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(title),
                    )
                    .child(
                        div()
                            .id("payment-auth-x")
                            .size(px(28.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--interactive-icon-default"))
                            .on_click(cx.listener({
                                let pid = payment_id.clone();
                                move |this, _, _, cx| {
                                    if step == PaymentAuthStep::Required {
                                        let _ = this.requests.send(payment_void_request(&pid));
                                    }
                                    this.payment_auth_modal = None;
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .child(
                div()
                    .px(px(20.))
                    .pb(px(20.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .size(px(72.))
                            .rounded_full()
                            .bg(badge_bg)
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(badge_icon, 36., badge_fg)),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .text_color(color("--text-default"))
                            .child(message),
                    ),
            )
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lower"))
                    .child(actions),
            );

        Some(
            gpui::deferred(
                div()
                    .id("payment-auth-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if step == PaymentAuthStep::Required {
                            let _ = this.requests.send(payment_void_request(&payment_id));
                        }
                        this.payment_auth_modal = None;
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_payment_auth(
        &mut self,
        arg: &str,
        cx: &mut gpui::Context<Self>,
    ) -> String {
        let mode = arg.trim();
        if mode == "close" {
            self.payment_auth_modal = None;
            cx.notify();
            return "payment-auth: closed".to_owned();
        }
        let step = match mode {
            "failed" => PaymentAuthStep::Failed,
            "success" => PaymentAuthStep::Success,
            "canceled" => PaymentAuthStep::Canceled,
            _ => PaymentAuthStep::Required,
        };
        self.payment_auth_modal = Some(Box::new(PaymentAuthModalState {
            payment_id: "pay-302810".to_owned(),
            price_formatted: "$9.99".to_owned(),
            item_description: "Nitro Monthly".to_owned(),
            step,
            awaiting_auth: false,
        }));
        cx.notify();
        format!("payment-auth: {mode}")
    }

    /// Module 362111 (`AddPaymentMethodModal`, CSS `998973`/`24404`): standalone "Add a new payment method" flow.
    pub(in crate::ui) fn add_payment_method_layer(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        use crate::ui::{color, icon};
        use gpui::{FontWeight, IntoElement, SharedString, div, prelude::*, px, rgba};

        let state = self.add_payment_method_modal.as_deref()?;
        let step = state.step;
        let selected_kind = state.selected_kind;
        let error_msg = state.error_message.clone();
        let country_locked = state.country_locked;

        let body: gpui::AnyElement = match step {
            AddPaymentStep::PaymentElement => div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .when_some(error_msg.clone(), |col, err| {
                    col.child(
                        div()
                            .p(px(10.))
                            .rounded(px(8.))
                            .bg(rgba(0xda373c1a))
                            .border_1()
                            .border_color(color("--status-danger"))
                            .text_size(px(13.))
                            .text_color(color("--text-feedback-critical"))
                            .child(err),
                    )
                })
                .child(div().flex().flex_wrap().gap(px(8.)).children(
                    AddPaymentKind::ALL.into_iter().map(|kind| {
                        let active = kind == selected_kind;
                        div()
                            .id(SharedString::from(format!("add-pm-kind-{kind:?}")))
                            .w(px(196.))
                            .h(px(44.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(color(if active {
                                "--control-primary-background-default"
                            } else {
                                "--border-subtle"
                            }))
                            .bg(color(if active {
                                "--background-mod-strong"
                            } else {
                                "--background-base-lower"
                            }))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(14.))
                            .font_weight(FontWeight(if active { 600. } else { 500. }))
                            .text_color(color("--text-strong"))
                            .child(kind.label())
                            .when(active, |d| {
                                d.child(icon("circle_check", 16., "--brand-400"))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(m) = this.add_payment_method_modal.as_deref_mut() {
                                    m.selected_kind = kind;
                                    m.error_message = None;
                                }
                                cx.notify();
                            }))
                    }),
                ))
                .when(country_locked, |col| {
                    col.child(
                        div()
                            .text_size(px(12.))
                            .text_color(color("--text-muted"))
                            .child(dc_intl::t("Pg2hU0")),
                    )
                })
                .into_any_element(),
            AddPaymentStep::CreditCard => div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .when_some(error_msg.clone(), |col, err| {
                    col.child(
                        div()
                            .p(px(10.))
                            .rounded(px(8.))
                            .bg(rgba(0xda373c1a))
                            .border_1()
                            .border_color(color("--status-danger"))
                            .text_size(px(13.))
                            .text_color(color("--text-feedback-critical"))
                            .child(err),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("cVyJ3o")),
                        )
                        .child(
                            div()
                                .h(px(38.))
                                .px(px(10.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--input-border-default"))
                                .bg(color("--input-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .text_color(color(if state.card_number.is_empty() {
                                    "--text-muted"
                                } else {
                                    "--text-strong"
                                }))
                                .child(if state.card_number.is_empty() {
                                    dc_intl::t("gPRHfw").to_owned()
                                } else {
                                    state.card_number.clone()
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("CeBa/4")),
                                )
                                .child(
                                    div()
                                        .h(px(38.))
                                        .px(px(10.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--input-border-default"))
                                        .bg(color("--input-background-default"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .text_color(color(if state.exp_date.is_empty() {
                                            "--text-muted"
                                        } else {
                                            "--text-strong"
                                        }))
                                        .child(if state.exp_date.is_empty() {
                                            dc_intl::t("xeEWQ6").to_owned()
                                        } else {
                                            state.exp_date.clone()
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight(600.))
                                        .text_color(color("--text-strong"))
                                        .child(dc_intl::t("Fd3rOz")),
                                )
                                .child(
                                    div()
                                        .h(px(38.))
                                        .px(px(10.))
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(color("--input-border-default"))
                                        .bg(color("--input-background-default"))
                                        .flex()
                                        .items_center()
                                        .text_size(px(14.))
                                        .text_color(color(if state.cvc.is_empty() {
                                            "--text-muted"
                                        } else {
                                            "--text-strong"
                                        }))
                                        .child(if state.cvc.is_empty() {
                                            dc_intl::t("wZz04F").to_owned()
                                        } else {
                                            state.cvc.clone()
                                        }),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight(600.))
                                .text_color(color("--text-strong"))
                                .child(dc_intl::t("VUlFdU")),
                        )
                        .child(
                            div()
                                .h(px(38.))
                                .px(px(10.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(color("--input-border-default"))
                                .bg(color("--input-background-default"))
                                .flex()
                                .items_center()
                                .text_size(px(14.))
                                .text_color(color(if state.cardholder_name.is_empty() {
                                    "--text-muted"
                                } else {
                                    "--text-strong"
                                }))
                                .child(if state.cardholder_name.is_empty() {
                                    dc_intl::t("yf7ms+").to_owned()
                                } else {
                                    state.cardholder_name.clone()
                                }),
                        ),
                )
                .into_any_element(),
            AddPaymentStep::ExternalMethod => {
                let helper = match selected_kind {
                    AddPaymentKind::PayPal => dc_intl::t("hopw7X"),
                    AddPaymentKind::Venmo => dc_intl::t("mIL6Jb"),
                    AddPaymentKind::CashApp => dc_intl::t("1MqcjI"),
                    _ => dc_intl::t("Pg2hU0"),
                };
                div()
                    .py(px(12.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(10.))
                    .child(icon("role_shield", 36., "--brand-400"))
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--text-strong"))
                            .child(selected_kind.label()),
                    )
                    .child(
                        div()
                            .text_center()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(color("--text-muted"))
                            .child(helper),
                    )
                    .into_any_element()
            }
            AddPaymentStep::Address | AddPaymentStep::AwaitingAuth => div()
                .p(px(12.))
                .rounded(px(8.))
                .bg(color("--background-base-lower"))
                .border_1()
                .border_color(color("--border-subtle"))
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(color("--text-default"))
                .child(if step == AddPaymentStep::AwaitingAuth {
                    dc_intl::t("eALkIR")
                } else {
                    dc_intl::t("Pg2hU0")
                })
                .into_any_element(),
        };

        let primary_label = match step {
            AddPaymentStep::ExternalMethod => selected_kind.connect_cta(),
            _ => dc_intl::t("PDTjLN"),
        };

        let dialog = div()
            .id("add-payment-method-dialog")
            .w(px(460.))
            .rounded(px(12.))
            .bg(color("--background-surface-high"))
            .border_1()
            .border_color(color("--border-subtle"))
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(20.))
                    .py(px(16.))
                    .border_b_1()
                    .border_color(color("--border-subtle"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight(700.))
                            .text_color(color("--text-strong"))
                            .child(dc_intl::t("eQ2bLp")),
                    )
                    .child(
                        div()
                            .id("add-pm-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("modal_close", 16., "--interactive-icon-default"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.add_payment_method_modal = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().p(px(20.)).child(body))
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lower"))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .id("add-pm-back")
                            .h(px(36.))
                            .px(px(14.))
                            .rounded(px(8.))
                            .bg(color("--control-secondary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(500.))
                            .text_color(color("--control-secondary-text-default"))
                            .child(dc_intl::t("ETE/oC"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(m) = this.add_payment_method_modal.as_deref_mut()
                                    && m.step != AddPaymentStep::PaymentElement
                                {
                                    m.step = AddPaymentStep::PaymentElement;
                                    m.error_message = None;
                                } else {
                                    this.add_payment_method_modal = None;
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("add-pm-next")
                            .h(px(36.))
                            .px(px(18.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(14.))
                            .font_weight(FontWeight(600.))
                            .text_color(color("--white"))
                            .child(primary_label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                match step {
                                    AddPaymentStep::PaymentElement => {
                                        if let Some(m) =
                                            this.add_payment_method_modal.as_deref_mut()
                                        {
                                            m.step = if m.selected_kind == AddPaymentKind::Card {
                                                AddPaymentStep::CreditCard
                                            } else {
                                                AddPaymentStep::ExternalMethod
                                            };
                                        }
                                    }
                                    AddPaymentStep::CreditCard => {
                                        if let Some(m) =
                                            this.add_payment_method_modal.as_deref_mut()
                                        {
                                            if let Err(err) = m.validate_card() {
                                                m.error_message = Some(err.to_owned());
                                            } else {
                                                m.error_message = None;
                                                m.step = AddPaymentStep::Address;
                                            }
                                        }
                                    }
                                    AddPaymentStep::ExternalMethod | AddPaymentStep::Address => {
                                        let _ = this.requests.send(create_payment_source_request(
                                            "user_settings",
                                            1,
                                            "tok_demo",
                                            None,
                                            true,
                                        ));
                                        this.add_payment_method_modal = None;
                                        this.show_success_toast(
                                            dc_intl::t("VJPg+l").to_owned(),
                                            cx,
                                        );
                                    }
                                    AddPaymentStep::AwaitingAuth => {
                                        this.add_payment_method_modal = None;
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            );

        Some(
            gpui::deferred(
                div()
                    .id("add-payment-method-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b8))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.add_payment_method_modal = None;
                        cx.notify();
                    }))
                    .child(dialog),
            )
            .with_priority(4)
            .into_any_element(),
        )
    }

    #[cfg(feature = "capture")]
    pub(crate) fn script_add_payment_method(
        &mut self,
        arg: &str,
        cx: &mut gpui::Context<Self>,
    ) -> String {
        match arg.trim() {
            "close" => {
                self.add_payment_method_modal = None;
                cx.notify();
                "add-payment: closed".to_owned()
            }
            "card" => {
                let mut m = AddPaymentMethodState::new();
                m.step = AddPaymentStep::CreditCard;
                m.card_number = "4242 4242 4242 4242".to_owned();
                m.exp_date = "08/29".to_owned();
                m.cvc = "424".to_owned();
                m.cardholder_name = "Lorean Xavier".to_owned();
                self.add_payment_method_modal = Some(Box::new(m));
                cx.notify();
                "add-payment: card".to_owned()
            }
            _ => {
                let mut m = AddPaymentMethodState::new();
                m.country_locked = true;
                self.add_payment_method_modal = Some(Box::new(m));
                cx.notify();
                "add-payment: methods".to_owned()
            }
        }
    }
}

/// Module 6151 (`a`): sentinel option ID for "Add new payment method".
pub(in crate::ui) const NEW_PAYMENT_SOURCE_ID: &str = "new_payment_source_id";

/// Interactive Payment Source Picker modal state (modules 637141 & 6151).
#[derive(Clone, Debug)]
pub(in crate::ui) struct PaymentSourcePickerState {
    pub sources: Vec<PaymentSource>,
    pub selected_id: Option<String>,
    pub hide_personal_info: bool,
    pub is_trial: bool,
    pub dropdown_open: bool,
    /// Module 6151: `variant: "unsupported_payment_modal_card"` (`56px` min-h card + `u+Cw58`/`rNF29q`).
    pub unsupported_card_variant: bool,
    /// Module 6151: optional info notice banner (`.noticeMessageContainer_sFISJa`).
    pub notice_message: Option<String>,
}

/// Official `T`/`Sm` option label formatter of module 637141 (all 18 payment source types + Streamer Mode masking).
pub(in crate::ui) fn picker_option_label(source: &PaymentSource, hide_pii: bool) -> String {
    use dc_intl::Arg;
    use std::borrow::Cow;
    const MASKED_EMAIL: &str = "***@***.***";
    match source.kind {
        kind::CARD => {
            if source.is_link_bank() {
                let date = if hide_pii { "********" } else { "2026-01-01" };
                dc_intl::format("ixNwPL", &[("date", Arg::Str(Cow::Borrowed(date)))])
            } else {
                let last4 = if hide_pii { "****" } else { &source.last4 };
                dc_intl::format("QvBAQk", &[("last4", Arg::Str(Cow::Borrowed(last4)))])
            }
        }
        kind::PAYPAL | kind::SOFORT | kind::PRZELEWY24 => {
            if hide_pii {
                MASKED_EMAIL.to_owned()
            } else {
                source.email.clone()
            }
        }
        kind::GIROPAY => dc_intl::t("y+0MQZ").to_owned(),
        kind::PAYSAFE_CARD => dc_intl::t("e3APTT").to_owned(),
        kind::GCASH => dc_intl::t("PjehcF").to_owned(),
        kind::GRABPAY_MY => dc_intl::t("T5davE").to_owned(),
        kind::MOMO_WALLET => dc_intl::t("J0A1Vk").to_owned(),
        kind::VENMO => {
            if hide_pii {
                "***".to_owned()
            } else {
                format!("@{}", source.username)
            }
        }
        kind::KAKAOPAY => dc_intl::t("CSVexi").to_owned(),
        kind::GOPAY_WALLET => dc_intl::t("43J8JK").to_owned(),
        kind::BANCONTACT => dc_intl::t("1ITkfq").to_owned(),
        kind::EPS => {
            let bank = bank_name(EPS_BANKS, source.bank.as_deref().unwrap_or(""));
            dc_intl::format("hSPoZw", &[("bank", Arg::Str(Cow::Owned(bank)))])
        }
        kind::IDEAL => {
            let raw = source.bank.as_deref().unwrap_or("");
            if raw.is_empty() {
                dc_intl::t("nSbwqC").to_owned()
            } else {
                let bank = bank_name(IDEAL_BANKS, raw);
                dc_intl::format("9kUlRU", &[("bank", Arg::Str(Cow::Owned(bank)))])
            }
        }
        kind::CASH_APP => {
            if hide_pii {
                "***".to_owned()
            } else {
                source.username.clone()
            }
        }
        kind::PIX => dc_intl::t("JG3WQU").to_owned(),
        kind::PIX_AUTOMATICO => dc_intl::t("JkKNss").to_owned(),
        _ => dc_intl::t("eQ2bLp").to_owned(),
    }
}

/// Official `H` trial notice of module 637141 (`SvheW9` ineligible, `d7ZLKA` authorization hold).
pub(in crate::ui) fn picker_trial_notice(
    source: &PaymentSource,
    is_trial: bool,
) -> Option<(String, bool)> {
    if !is_trial {
        return None;
    }
    let can_redeem_trial = matches!(source.kind, kind::CARD | kind::PAYPAL);
    if !can_redeem_trial {
        return Some((dc_intl::t("SvheW9").to_owned(), true));
    }
    let msg = dc_intl::format(
        "d7ZLKA",
        &[(
            "helpDeskArticle",
            dc_intl::Arg::Str(std::borrow::Cow::Borrowed(
                "https://support.discord.com/hc/en-us/articles/360017693772",
            )),
        )],
    );
    Some((msg, false))
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

    #[test]
    fn picker_labels_and_streamer_masking_match_637141() {
        let card = PaymentSource::from_server(
            &json!({"id": "1", "type": 1, "brand": "visa", "last_4": "4242"}),
        )
        .unwrap();
        assert_eq!(picker_option_label(&card, false), "Card ending in 4242");
        assert_eq!(picker_option_label(&card, true), "Card ending in ****");

        let paypal =
            PaymentSource::from_server(&json!({"id": "2", "type": 2, "email": "a@b.com"})).unwrap();
        assert_eq!(picker_option_label(&paypal, false), "a@b.com");
        assert_eq!(picker_option_label(&paypal, true), "***@***.***");

        let pix = PaymentSource::from_server(&json!({"id": "3", "type": 20})).unwrap();
        assert_eq!(picker_option_label(&pix, false), "Pix");
        assert!(picker_trial_notice(&pix, true).unwrap().1);
        assert!(!picker_trial_notice(&card, true).unwrap().1);

        for kind_id in [3u64, 7, 8, 9, 10, 12, 13, 14, 15, 16, 20, 21] {
            let src = PaymentSource::from_server(&json!({"id": "x", "type": kind_id})).unwrap();
            assert!(!picker_option_label(&src, false).is_empty());
        }
        assert_eq!(NEW_PAYMENT_SOURCE_ID, "new_payment_source_id");
        assert_eq!(dc_intl::t("u+Cw58"), "Payment method");
        assert_eq!(dc_intl::t("rNF29q"), "Add new payment method");
    }

    #[test]
    fn payment_auth_302810_routes_and_intl() {
        for key in [
            "N65ZrW", "DPgrRk", "xypuIv", "uEoQdA", "RoxWET", "PZskfs", "j0tI/b", "BAr3rB",
            "2NeVoY", "VAqBoE", "cpT0Cq",
        ] {
            assert!(dc_intl::has_key(key), "missing 302810 key {key}");
        }
        assert!(matches!(
            payment_void_request("p42"),
            crate::net::Request::Rest { method, route, .. }
                if method == reqwest::Method::POST
                    && route == "/users/@me/billing/payments/p42/void"
        ));
        assert!(matches!(
            payment_intent_request("p42"),
            crate::net::Request::Rest { method, route, .. }
                if method == reqwest::Method::GET
                    && route == "/users/@me/billing/stripe/payment-intents/payments/p42"
        ));
    }

    #[test]
    fn add_payment_method_362111_validation_routes_and_intl() {
        for key in [
            "eQ2bLp", "VJPg+l", "PDTjLN", "cVyJ3o", "gPRHfw", "CeBa/4", "xeEWQ6", "Fd3rOz",
            "wZz04F", "VUlFdU", "yf7ms+", "eOIfuy", "x4pWtJ", "9/zZdl", "ro4isZ", "lIkVsi",
            "4vnhKV", "vZ8y7l", "Pg2hU0", "QQBAos", "Djzd7L", "4KoTLM", "9ALP8w",
        ] {
            assert!(dc_intl::has_key(key), "missing 362111 key {key}");
        }
        let mut s = AddPaymentMethodState::new();
        assert_eq!(s.validate_card(), Err(dc_intl::t("eOIfuy")));
        s.card_number = "1234".into();
        assert_eq!(s.validate_card(), Err(dc_intl::t("x4pWtJ")));
        s.card_number = "4242424242424242".into();
        s.exp_date = "bad".into();
        assert_eq!(s.validate_card(), Err(dc_intl::t("9/zZdl")));
        s.exp_date = "08/29".into();
        s.cvc = "1".into();
        assert_eq!(s.validate_card(), Err(dc_intl::t("ro4isZ")));
        s.cvc = "424".into();
        assert_eq!(s.validate_card(), Err(dc_intl::t("lIkVsi")));
        s.cardholder_name = "Alex".into();
        assert!(s.validate_card().is_ok());

        assert!(matches!(
            payment_source_creation_context_request(),
            crate::net::Request::Rest { method, route, .. }
                if method == reqwest::Method::GET
                    && route == "/users/@me/billing/payment-source-creation-context"
        ));
        assert!(matches!(
            validate_billing_address_request("Alex", "123 Main", "NY", "10001", "US"),
            crate::net::Request::Rest { method, route, .. }
                if method == reqwest::Method::POST
                    && route == "/users/@me/billing/payment-sources/validate-billing-address"
        ));
        assert!(matches!(
            create_payment_source_request("billing", 1, "tok", Some("bat"), true),
            crate::net::Request::Rest { method, route, .. }
                if method == reqwest::Method::POST
                    && route == "/users/@me/billing/payment-sources?location=billing"
        ));
    }
}
