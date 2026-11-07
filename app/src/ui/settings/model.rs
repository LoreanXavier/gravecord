//! The shape of a settings page, as `tools/gen-settings-pages.py` writes it.

/// Text with link runs: `(text, is_link)`.
pub(in crate::ui) type Runs = &'static [(&'static str, bool)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum ButtonKind {
    Primary,
    Secondary,
    CriticalSecondary,
    CriticalPrimary,
}

pub(in crate::ui) struct RadioOption {
    pub label: &'static str,
    pub description: Runs,
}

pub(in crate::ui) enum Node {
    Category {
        key: &'static str,
        title: Option<&'static str>,
        /// `heading-sm/semibold` over a group without a title, like "Related Settings".
        group: Option<&'static str>,
        /// The `legend` of a fieldset group, like "Notify me when...".
        legend: Option<&'static str>,
        description: Runs,
        children: &'static [Node],
    },
    Divider,
    Switch {
        key: &'static str,
        label: &'static str,
        badge: bool,
        description: Runs,
        on: bool,
    },
    Slider {
        key: &'static str,
        label: &'static str,
        badge: bool,
        description: Runs,
        /// Mark text and its place along the track, 0 to 1.
        marks: &'static [(&'static str, f32)],
        default_mark: Option<usize>,
        value: f32,
    },
    Radio {
        key: &'static str,
        label: &'static str,
        badge: bool,
        description: Runs,
        options: &'static [RadioOption],
        selected: usize,
    },
    Select {
        key: &'static str,
        label: &'static str,
        badge: bool,
        description: Runs,
        value: &'static str,
        vertical: bool,
        width: f32,
    },
    Control {
        key: &'static str,
        label: &'static str,
        badge: bool,
        description: Runs,
        icon: Option<&'static str>,
        value: &'static str,
        buttons: &'static [(&'static str, ButtonKind)],
        clickable: bool,
    },
    Notice {
        key: &'static str,
        text: Runs,
    },
    /// A component not modelled yet, holding its measured height so the page below keeps its place.
    Custom {
        key: &'static str,
        height: f32,
    },
}

impl Node {
    pub fn key(&self) -> &'static str {
        match self {
            Node::Category { key, .. }
            | Node::Switch { key, .. }
            | Node::Slider { key, .. }
            | Node::Radio { key, .. }
            | Node::Select { key, .. }
            | Node::Control { key, .. }
            | Node::Notice { key, .. }
            | Node::Custom { key, .. } => key,
            Node::Divider => "",
        }
    }
}

/// What the user changed on a page, by setting key. Unchanged settings show the captured value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui) enum Choice {
    On(bool),
    Index(usize),
}
