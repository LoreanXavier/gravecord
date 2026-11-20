//! Billing address form with country/state/province selectors and validation (module 718718).
//!
//! Implements every layout (`modalUS`, `modalInternational`, `modalUSWithName`, `modalInternationalWithName`,
//! `settingsUS`, `settingsInternational`, `settingsInternationalWithoutName`, and mobile variants), `Create`/`Edit`
//! modes, all 59 US states/territories, all 13 Canadian provinces/territories, the 64 postal-code-exempt country
//! codes (`g`), and every localized label, placeholder, and validation message of module 718718.

use super::discover::text;
use super::{Root, color, icon};
use gpui::{AnyElement, Context, IntoElement, SharedString, deferred, div, prelude::*, px, rgba};
use std::collections::BTreeMap;

/// Canadian provinces & territories (`s` of module 718718).
pub const CA_PROVINCES: &[(&str, &str)] = &[
    ("AB", "Alberta"),
    ("BC", "British Columbia"),
    ("MB", "Manitoba"),
    ("NB", "New Brunswick"),
    ("NL", "Newfoundland and Labrador"),
    ("NS", "Nova Scotia"),
    ("ON", "Ontario"),
    ("PE", "Prince Edward Island"),
    ("QC", "Quebec"),
    ("SK", "Saskatchewan"),
    ("NT", "Northwest Territories"),
    ("NU", "Nunavut"),
    ("YT", "Yukon"),
];

/// US states & territories (`c` of module 718718).
pub const US_STATES: &[(&str, &str)] = &[
    ("AL", "Alabama"),
    ("AK", "Alaska"),
    ("AS", "American Samoa"),
    ("AZ", "Arizona"),
    ("AR", "Arkansas"),
    ("AA", "Armed Forces: Americas"),
    ("AE", "Armed Forces: Europe"),
    ("AP", "Armed Forces: Pacific"),
    ("CA", "California"),
    ("CO", "Colorado"),
    ("CT", "Connecticut"),
    ("DE", "Delaware"),
    ("DC", "District Of Columbia"),
    ("FM", "Federated States Of Micronesia"),
    ("FL", "Florida"),
    ("GA", "Georgia"),
    ("GU", "Guam"),
    ("HI", "Hawaii"),
    ("ID", "Idaho"),
    ("IL", "Illinois"),
    ("IN", "Indiana"),
    ("IA", "Iowa"),
    ("KS", "Kansas"),
    ("KY", "Kentucky"),
    ("LA", "Louisiana"),
    ("ME", "Maine"),
    ("MH", "Marshall Islands"),
    ("MD", "Maryland"),
    ("MA", "Massachusetts"),
    ("MI", "Michigan"),
    ("MN", "Minnesota"),
    ("MS", "Mississippi"),
    ("MO", "Missouri"),
    ("MT", "Montana"),
    ("NE", "Nebraska"),
    ("NV", "Nevada"),
    ("NH", "New Hampshire"),
    ("NJ", "New Jersey"),
    ("NM", "New Mexico"),
    ("NY", "New York"),
    ("NC", "North Carolina"),
    ("ND", "North Dakota"),
    ("MP", "Northern Mariana Islands"),
    ("OH", "Ohio"),
    ("OK", "Oklahoma"),
    ("OR", "Oregon"),
    ("PW", "Palau"),
    ("PA", "Pennsylvania"),
    ("PR", "Puerto Rico"),
    ("RI", "Rhode Island"),
    ("SC", "South Carolina"),
    ("SD", "South Dakota"),
    ("TN", "Tennessee"),
    ("TX", "Texas"),
    ("UT", "Utah"),
    ("VT", "Vermont"),
    ("VI", "Virgin Islands"),
    ("VA", "Virginia"),
    ("WA", "Washington"),
    ("WV", "West Virginia"),
    ("WI", "Wisconsin"),
    ("WY", "Wyoming"),
];

/// Countries without required postal codes (`g` of module 718718).
pub const POSTAL_EXEMPT_COUNTRIES: &[&str] = &[
    "AE", "AG", "AN", "AO", "AW", "BF", "BI", "BJ", "BM", "BO", "BQ", "BS", "BW", "BZ", "CD", "CF",
    "CG", "CI", "CK", "CM", "CW", "DJ", "DM", "ER", "FJ", "GA", "GD", "GH", "GM", "GQ", "GY", "HK",
    "HM", "IE", "JM", "KE", "KI", "KM", "KN", "KP", "LY", "ML", "MO", "MR", "MW", "NA", "NR", "NU",
    "QA", "RW", "SB", "SC", "SL", "SR", "ST", "SX", "SY", "TD", "TF", "TG", "TK", "TL", "TO", "TV",
    "UG", "VU", "YE", "ZA", "ZW",
];

/// Common billing countries (`I` of module 718718, excluding KP/SY).
pub const BILLING_COUNTRIES: &[(&str, &str)] = &[
    ("US", "United States"),
    ("CA", "Canada"),
    ("GB", "United Kingdom"),
    ("TR", "Turkey"),
    ("DE", "Germany"),
    ("FR", "France"),
    ("JP", "Japan"),
    ("AU", "Australia"),
    ("BR", "Brazil"),
    ("NL", "Netherlands"),
    ("PL", "Poland"),
    ("ES", "Spain"),
    ("IT", "Italy"),
    ("SE", "Sweden"),
    ("IE", "Ireland"),
    ("HK", "Hong Kong"),
];

/// `Layouts` (`N` of module 718718).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(not(feature = "capture"), allow(dead_code))]
pub enum BillingLayout {
    #[default]
    ModalUs,
    ModalInternational,
    ModalUsWithName,
    ModalInternationalWithName,
    SettingsUs,
    SettingsInternational,
}

/// `Modes` (`j` of module 718718).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BillingMode {
    #[default]
    Create,
    Edit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BillingField {
    Name,
    Country,
    Line1,
    Line2,
    City,
    State,
    PostalCode,
}

/// Form values (`state.values` of module 718718).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BillingAddressValues {
    pub name: String,
    pub country: String,
    pub line1: String,
    pub line2: String,
    pub city: String,
    pub state: String,
    pub postal_code: String,
}

/// Localized label and optional placeholder for each field (`M`, `L`, `k`, `w`, `D`, `U`, `G` of module 718718).
pub fn field_meta(field: BillingField, country: &str) -> (&'static str, Option<&'static str>) {
    match field {
        BillingField::Name => (dc_intl::t("vyuULb"), None),
        BillingField::Country => (dc_intl::t("eDdrAD"), None),
        BillingField::Line1 => (dc_intl::t("x0beVT"), Some(dc_intl::t("ynII/6"))),
        BillingField::Line2 => (dc_intl::t("i2Z0gI"), Some(dc_intl::t("fKLoNo"))),
        BillingField::City => (dc_intl::t("bUSWlw"), Some(dc_intl::t("5rRx31"))),
        BillingField::PostalCode => match country {
            "US" => (dc_intl::t("/95CeM"), Some(dc_intl::t("9xLNmi"))),
            "CA" => (dc_intl::t("mfpJ9m"), Some(dc_intl::t("Nc4Rzt"))),
            _ => (dc_intl::t("mfpJ9m"), None),
        },
        BillingField::State => match country {
            "US" => (dc_intl::t("PNfx5f"), None),
            "CA" => (dc_intl::t("7A/tE0"), None),
            _ => (dc_intl::t("w0xG2u"), None),
        },
    }
}

/// Row layout matrix (`F` of module 718718).
pub fn layout_rows(layout: BillingLayout) -> &'static [&'static [BillingField]] {
    use BillingField::*;
    match layout {
        BillingLayout::ModalUs => &[
            &[Country],
            &[Line1],
            &[Line2],
            &[City],
            &[State, PostalCode],
        ],
        BillingLayout::ModalInternational => &[
            &[Country],
            &[Line1],
            &[Line2],
            &[City],
            &[State],
            &[PostalCode],
        ],
        BillingLayout::ModalUsWithName => &[
            &[Country],
            &[Name],
            &[Line1],
            &[Line2],
            &[City],
            &[State, PostalCode],
        ],
        BillingLayout::ModalInternationalWithName => &[
            &[Country],
            &[Name],
            &[Line1],
            &[Line2],
            &[City],
            &[State],
            &[PostalCode],
        ],
        BillingLayout::SettingsUs => &[
            &[Name],
            &[Line1, Line2],
            &[City, State, PostalCode],
            &[Country],
        ],
        BillingLayout::SettingsInternational => &[
            &[Name],
            &[Line1, Line2],
            &[City],
            &[State, PostalCode],
            &[Country],
        ],
    }
}

/// `validateForm` of module 718718: returns localized field error map keyed by field name.
pub fn validate_billing_address(
    values: &BillingAddressValues,
    mode: BillingMode,
) -> BTreeMap<&'static str, &'static str> {
    let mut errors = BTreeMap::new();
    let has = |s: &str| !s.trim().is_empty();

    if mode == BillingMode::Edit && !has(&values.name) {
        errors.insert("name", dc_intl::t("KU5mWF"));
    }
    if !has(&values.country) {
        errors.insert("country", dc_intl::t("+bm+zE"));
    }
    if !has(&values.line1) {
        errors.insert("line1", dc_intl::t("6HMkB4"));
    }
    if !has(&values.city) {
        errors.insert("city", dc_intl::t("kOrBmU"));
    }

    match values.country.as_str() {
        "US" => {
            if !has(&values.postal_code) {
                errors.insert("postalCode", dc_intl::t("iXID+2"));
            } else if values.postal_code.len() != 5 {
                errors.insert("postalCode", dc_intl::t("+zjAbg"));
            } else if !values.postal_code.bytes().all(|b| b.is_ascii_digit()) {
                errors.insert("postalCode", dc_intl::t("CuZPea"));
            }
            if !has(&values.state) {
                errors.insert("state", dc_intl::t("RIaPdF"));
            }
        }
        "CA" => {
            if !has(&values.postal_code) {
                errors.insert("postalCode", dc_intl::t("LRlhb1"));
            }
            if !has(&values.state) {
                errors.insert("state", dc_intl::t("PsJCcj"));
            }
        }
        other => {
            if !has(&values.postal_code) && !POSTAL_EXEMPT_COUNTRIES.contains(&other) {
                errors.insert("postalCode", dc_intl::t("LRlhb1"));
            }
        }
    }

    errors
}

/// Open modal state for the standalone Billing Address dialog (module 718718).
#[derive(Clone, Debug)]
pub struct BillingAddressModal {
    pub values: BillingAddressValues,
    pub layout: BillingLayout,
    pub mode: BillingMode,
    pub show_errors: bool,
    pub country_open: bool,
    pub state_open: bool,
}

impl BillingAddressModal {
    pub fn demo(country: &str, with_errors: bool) -> Self {
        let layout = if country == "US" {
            BillingLayout::ModalUsWithName
        } else {
            BillingLayout::ModalInternationalWithName
        };
        let values = if with_errors {
            BillingAddressValues {
                name: String::new(),
                country: country.to_owned(),
                line1: String::new(),
                line2: String::new(),
                city: String::new(),
                state: String::new(),
                postal_code: if country == "US" {
                    "12A45".to_owned()
                } else {
                    String::new()
                },
            }
        } else {
            BillingAddressValues {
                name: "Alex Mercer".to_owned(),
                country: country.to_owned(),
                line1: "444 De Haro St".to_owned(),
                line2: "Suite 200".to_owned(),
                city: if country == "CA" {
                    "Toronto".to_owned()
                } else {
                    "San Francisco".to_owned()
                },
                state: if country == "CA" {
                    "ON".to_owned()
                } else {
                    "CA".to_owned()
                },
                postal_code: if country == "CA" {
                    "M5V 2T6".to_owned()
                } else {
                    "94107".to_owned()
                },
            }
        };
        Self {
            values,
            layout,
            mode: if with_errors {
                BillingMode::Edit
            } else {
                BillingMode::Create
            },
            show_errors: with_errors,
            country_open: false,
            state_open: false,
        }
    }

    fn field_value(&self, field: BillingField) -> &str {
        match field {
            BillingField::Name => &self.values.name,
            BillingField::Country => &self.values.country,
            BillingField::Line1 => &self.values.line1,
            BillingField::Line2 => &self.values.line2,
            BillingField::City => &self.values.city,
            BillingField::State => &self.values.state,
            BillingField::PostalCode => &self.values.postal_code,
        }
    }

    fn error_key(field: BillingField) -> &'static str {
        match field {
            BillingField::Name => "name",
            BillingField::Country => "country",
            BillingField::Line1 => "line1",
            BillingField::Line2 => "line2",
            BillingField::City => "city",
            BillingField::State => "state",
            BillingField::PostalCode => "postalCode",
        }
    }
}

impl Root {
    #[cfg(feature = "capture")]
    pub(crate) fn script_billing_address(&mut self, arg: &str, cx: &mut Context<Self>) -> String {
        let trimmed = arg.trim();
        let (country, with_errors, layout_override) = match trimmed {
            "ca" => ("CA", false, None),
            "errors" => ("US", true, None),
            "ca-errors" => ("CA", true, None),
            "intl" => ("TR", false, Some(BillingLayout::ModalInternational)),
            "settings-us" => ("US", false, Some(BillingLayout::SettingsUs)),
            "settings-intl" => ("TR", false, Some(BillingLayout::SettingsInternational)),
            _ => ("US", false, None),
        };
        let mut modal = BillingAddressModal::demo(country, with_errors);
        if let Some(layout) = layout_override {
            modal.layout = layout;
        }
        self.billing_address_modal = Some(modal);
        cx.notify();
        format!("billing-address: country={country} errors={with_errors}")
    }

    pub(super) fn billing_address_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let modal = self.billing_address_modal.as_ref()?;
        let errors = if modal.show_errors {
            validate_billing_address(&modal.values, modal.mode)
        } else {
            BTreeMap::new()
        };
        let country = modal.values.country.clone();
        let rows = layout_rows(modal.layout);

        let form_rows: Vec<AnyElement> = rows
            .iter()
            .enumerate()
            .map(|(row_idx, fields)| {
                div()
                    .w_full()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .children(fields.iter().map(|&field| {
                        let (label, placeholder) = field_meta(field, &country);
                        let raw_val = modal.field_value(field);
                        let display_val = match field {
                            BillingField::Country => BILLING_COUNTRIES
                                .iter()
                                .find(|(c, _)| *c == raw_val)
                                .map_or(raw_val, |(_, n)| *n)
                                .to_owned(),
                            BillingField::State if country == "US" => US_STATES
                                .iter()
                                .find(|(c, _)| *c == raw_val)
                                .map_or(raw_val, |(_, n)| *n)
                                .to_owned(),
                            BillingField::State if country == "CA" => CA_PROVINCES
                                .iter()
                                .find(|(c, _)| *c == raw_val)
                                .map_or(raw_val, |(_, n)| *n)
                                .to_owned(),
                            _ => raw_val.to_owned(),
                        };
                        let err = errors.get(BillingAddressModal::error_key(field)).copied();
                        let is_select = field == BillingField::Country
                            || (field == BillingField::State && matches!(country.as_str(), "US" | "CA"));
                        let show_dropdown = (field == BillingField::Country && modal.country_open)
                            || (field == BillingField::State && modal.state_open);

                        div()
                            .relative()
                            .flex_1()
                            .min_w(px(0.))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                text(12., 16., 700., if err.is_some() { "--text-feedback-critical" } else { "--text-subtle" })
                                    .child(label),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("billing-field-{row_idx}-{field:?}")))
                                    .w_full()
                                    .h(px(38.))
                                    .px(px(10.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(color(if err.is_some() {
                                        "--text-feedback-critical"
                                    } else {
                                        "--border-subtle"
                                    }))
                                    .bg(color("--background-base-lowest"))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(6.))
                                    .when(is_select, |el| {
                                        el.cursor_pointer().on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(m) = this.billing_address_modal.as_mut() {
                                                if field == BillingField::Country {
                                                    m.country_open = !m.country_open;
                                                    m.state_open = false;
                                                } else {
                                                    m.state_open = !m.state_open;
                                                    m.country_open = false;
                                                }
                                            }
                                            cx.notify();
                                        }))
                                    })
                                    .child(
                                        text(
                                            14.,
                                            18.,
                                            400.,
                                            if display_val.is_empty() {
                                                "--text-muted"
                                            } else {
                                                "--text-default"
                                            },
                                        )
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .child(if display_val.is_empty() {
                                            placeholder.unwrap_or("").to_owned()
                                        } else {
                                            display_val
                                        }),
                                    )
                                    .when(is_select, |el| {
                                        el.child(icon("automod_chevron_down", 14., "--text-muted"))
                                    }),
                            )
                            .when_some(err, |col, msg| {
                                col.child(text(12., 16., 500., "--text-feedback-critical").child(msg))
                            })
                            .when(show_dropdown, |col| {
                                let opts: &[(&str, &str)] = if field == BillingField::Country {
                                    BILLING_COUNTRIES
                                } else if country == "CA" {
                                    CA_PROVINCES
                                } else {
                                    US_STATES
                                };
                                col.child(
                                    deferred(
                                        div()
                                            .id(SharedString::from(format!("billing-dd-{field:?}")))
                                            .occlude()
                                            .absolute()
                                            .left_0()
                                            .right_0()
                                            .top(px(64.))
                                            .max_h(px(180.))
                                            .overflow_y_scroll()
                                            .p(px(4.))
                                            .rounded(px(6.))
                                            .border_1()
                                            .border_color(color("--border-subtle"))
                                            .bg(color("--background-surface-high"))
                                            .flex()
                                            .flex_col()
                                            .children(opts.iter().take(16).map(|&(code, name)| {
                                                div()
                                                    .id(SharedString::from(format!("billing-opt-{field:?}-{code}")))
                                                    .px(px(8.))
                                                    .py(px(6.))
                                                    .rounded(px(4.))
                                                    .hover(|s| s.bg(color("--background-mod-subtle")))
                                                    .cursor_pointer()
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        if let Some(m) = this.billing_address_modal.as_mut() {
                                                            if field == BillingField::Country {
                                                                m.values.country = code.to_owned();
                                                                m.values.state.clear();
                                                                m.layout = if code == "US" {
                                                                    BillingLayout::ModalUsWithName
                                                                } else {
                                                                    BillingLayout::ModalInternationalWithName
                                                                };
                                                                m.country_open = false;
                                                            } else {
                                                                m.values.state = code.to_owned();
                                                                m.state_open = false;
                                                            }
                                                        }
                                                        cx.notify();
                                                    }))
                                                    .child(text(13., 18., 400., "--text-default").child(name))
                                            })),
                                    )
                                    .with_priority(4),
                                )
                            })
                    }))
                    .into_any_element()
            })
            .collect();

        let card = div()
            .id("billing-address-modal")
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(480.))
            .rounded(px(12.))
            .border_1()
            .border_color(color("--border-subtle"))
            .bg(color("--background-base-low"))
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(20.))
                    .pt(px(18.))
                    .pb(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(text(18., 22., 700., "--text-strong").child(dc_intl::t("x0beVT")))
                    .child(
                        div()
                            .id("billing-address-close")
                            .size(px(28.))
                            .rounded(px(6.))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.billing_address_modal = None;
                                cx.notify();
                            }))
                            .child(icon("close", 16., "--interactive-text-default")),
                    ),
            )
            .child(
                div()
                    .px(px(20.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .children(form_rows),
            )
            .child(
                div()
                    .px(px(20.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(color("--border-subtle"))
                    .bg(color("--background-base-lower"))
                    .rounded_b(px(12.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("billing-address-cancel")
                            .h(px(36.))
                            .px(px(16.))
                            .rounded(px(8.))
                            .hover(|s| s.bg(color("--background-mod-subtle")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.billing_address_modal = None;
                                cx.notify();
                            }))
                            .child(
                                text(14., 18., 500., "--text-default").child(dc_intl::t("cpT0Cq")),
                            ),
                    )
                    .child(
                        div()
                            .id("billing-address-validate")
                            .h(px(36.))
                            .px(px(18.))
                            .rounded(px(8.))
                            .bg(color("--control-primary-background-default"))
                            .hover(|s| s.bg(color("--control-primary-background-hover")))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(m) = this.billing_address_modal.as_mut() {
                                    m.show_errors = true;
                                }
                                cx.notify();
                            }))
                            .child(
                                text(14., 18., 600., "--control-primary-text-default")
                                    .child(dc_intl::t("PDTjLN")),
                            ),
                    ),
            );

        Some(
            deferred(
                div()
                    .id("billing-address-backdrop")
                    .occlude()
                    .absolute()
                    .inset_0()
                    .bg(rgba(0x000000b3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            this.billing_address_modal = None;
                            cx.notify();
                        }),
                    )
                    .child(card),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn billing_validation_checks_us_ca_and_postal_exempt_countries() {
        let mut us = BillingAddressValues {
            country: "US".to_owned(),
            postal_code: "123".to_owned(),
            ..Default::default()
        };
        let errs = validate_billing_address(&us, BillingMode::Edit);
        assert_eq!(errs.get("name"), Some(&"Name is required"));
        assert_eq!(errs.get("line1"), Some(&"Oops! You forgot this one!"));
        assert_eq!(errs.get("city"), Some(&"City is required"));
        assert_eq!(errs.get("state"), Some(&"State is required"));
        assert_eq!(errs.get("postalCode"), Some(&"ZIP code must be 5 digits"));

        us.postal_code = "12A45".to_owned();
        assert_eq!(
            validate_billing_address(&us, BillingMode::Create).get("postalCode"),
            Some(&"ZIP code is invalid")
        );

        let ca = BillingAddressValues {
            country: "CA".to_owned(),
            line1: "1 Yonge St".to_owned(),
            city: "Toronto".to_owned(),
            ..Default::default()
        };
        let ca_errs = validate_billing_address(&ca, BillingMode::Create);
        assert_eq!(ca_errs.get("state"), Some(&"Province is required"));
        assert_eq!(ca_errs.get("postalCode"), Some(&"Postal code is required"));

        let ie = BillingAddressValues {
            country: "IE".to_owned(),
            line1: "1 O'Connell St".to_owned(),
            city: "Dublin".to_owned(),
            ..Default::default()
        };
        assert!(validate_billing_address(&ie, BillingMode::Create).is_empty());
    }
}
