//! The settings sidebar search (module 276530 `te`, matching in module 122088 `E`).
//!
//! A node matches when the lower-cased query is a subsequence (module 91871) of its lower-cased title or of any string
//! its `useSearchTerms` returns. A match keeps the node, everything under it and the nodes above it; a sidebar item
//! stays listed while anything inside its panel matches. Descriptions and option labels are not searched.

use gpui::AppContext as _;

use super::Page;
use super::model::Node;
use super::pages;
use super::search_terms::TERMS;

/// Module 91871: every character of `query` appears in `text` in order.
pub(super) fn fuzzy(query: &str, text: &str) -> bool {
    let mut rest = text.chars();
    query
        .chars()
        .all(|wanted| rest.any(|found| found == wanted))
}

fn matches_any(query: &str, titles: &[&str]) -> bool {
    titles
        .iter()
        .any(|title| fuzzy(query, &title.to_lowercase()))
}

/// The captured Account page is drawn by hand, so its titles are listed here (`account_page`, `account_panels`).
const ACCOUNT_TITLES: &[&str] = &[
    "Account Info",
    "Username",
    "Email",
    "Phone Number",
    "Password & Security",
    "Password",
    "Multi-Factor Authentication",
    "Logged-in Devices",
    "Age Group",
    "Account Standing",
    "Account Status",
    "Family Center",
    "Set up Family Center",
    "Account Removal",
    "Disable your account",
    "Close your account",
];

fn terms_of(key: &str) -> &'static [&'static str] {
    TERMS
        .iter()
        .find(|(known, _)| *known == key)
        .map_or(&[], |(_, terms)| *terms)
}

fn node_matches(node: &Node, query: &str) -> bool {
    let mut titles: Vec<&str> = Vec::new();
    let children: &[Node] = match node {
        Node::Category {
            title,
            group,
            legend,
            children,
            ..
        } => {
            titles.extend(title.iter().chain(group).chain(legend));
            children
        }
        Node::Accordion {
            label, children, ..
        } => {
            titles.push(label);
            children
        }
        Node::Switch { label, .. }
        | Node::Slider { label, .. }
        | Node::Radio { label, .. }
        | Node::Select { label, .. }
        | Node::Control { label, .. } => {
            titles.push(label);
            &[]
        }
        _ => &[],
    };
    matches_any(query, &titles)
        || matches_any(query, terms_of(node.key()))
        || children.iter().any(|child| node_matches(child, query))
}

/// Whether the sidebar item of `page`, titled `label`, survives a search for `query`.
pub(super) fn page_matches(page: Page, label: &str, query: &str) -> bool {
    if query.is_empty() || fuzzy(query, &label.to_lowercase()) {
        return true;
    }
    if page == Page::Account {
        return matches_any(query, ACCOUNT_TITLES);
    }
    pages::nodes(page)
        .iter()
        .any(|node| node_matches(node, query))
}

/// The text style of a settings input (`input_0PLWhS`: 16px on a 20px line, `--input-placeholder-text-default`).
pub(super) fn field_style() -> crate::editor::EditorStyle {
    let mut style = crate::ui::editor_style();
    style.line_height = 20.;
    style.placeholder = crate::ui::hsla("--input-placeholder-text-default");
    style
}

impl crate::ui::Root {
    /// The search box exists from the time settings open (module 276530 `te`).
    pub(super) fn settings_search_ready(&mut self, cx: &mut gpui::Context<Self>) {
        if self.settings_extra.search.is_some() {
            return;
        }
        let editor = cx.new(|cx| {
            let mut editor = crate::editor::Editor::new(field_style(), cx);
            editor.set_single_line(true);
            editor.set_placeholder("Search".to_owned());
            editor
        });
        self.settings_extra.search_sub = Some(
            cx.subscribe(&editor, |_, _, _: &crate::editor::EditorEvent, cx| {
                cx.notify()
            }),
        );
        self.settings_extra.search = Some(editor);
    }

    /// The query as the bundle matches it: the typed text, lower-cased (`E`, module 122088).
    pub(super) fn settings_query(&self, cx: &gpui::App) -> String {
        self.settings_extra
            .search
            .as_ref()
            .map(|editor| editor.read(cx).text().to_lowercase())
            .unwrap_or_default()
    }

    /// Escape clears the field before it closes Settings.
    pub(in crate::ui) fn settings_escape(&mut self, cx: &mut gpui::Context<Self>) {
        let typed = self.settings_query(cx);
        match self.settings_extra.search.clone() {
            Some(editor) if !typed.is_empty() => {
                editor.update(cx, |editor, _| editor.set_text(""));
                cx.notify();
            }
            _ => self.close_settings(cx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_like_the_bundle() {
        assert!(fuzzy("vce", "voice"));
        assert!(!fuzzy("ecv", "voice"));
        assert!(fuzzy("", "anything"));
    }

    /// `search-voice-darker-cozy@1.png`: the pages the official sidebar keeps for "voice".
    #[test]
    fn voice_keeps_the_captured_pages() {
        let kept: Vec<&str> = super::super::SECTIONS
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .filter(|entry| page_matches(entry.page, entry.label, "voice"))
            .map(|entry| entry.label)
            .collect();
        assert_eq!(
            kept,
            [
                "Data & Privacy",
                "Notifications",
                "Voice & Video",
                "Appearance",
                "Accessibility",
                "System",
                "Activity Privacy",
                "Game Overlay",
            ]
        );
    }

    #[test]
    fn zzzz_keeps_nothing() {
        let any = super::super::SECTIONS
            .iter()
            .flat_map(|(_, entries)| entries.iter())
            .any(|entry| page_matches(entry.page, entry.label, "zzzz"));
        assert!(!any);
    }
}
