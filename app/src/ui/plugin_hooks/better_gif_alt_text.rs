//! BetterGifAltText (Vencord `src/plugins/betterGifAltText`, GPL-3.0-or-later): a GIF's alt text reads "GIF - <words of
//! its name>" instead of the bare "GIF" or nothing. The original patches the zoomable image (module 383055 `R`, whose
//! props go on to the viewer's item, so a GIF attachment without a description gets the viewer's View Details
//! "Description (Alt Text)" row) and the GIFV embed's default `alt` (module 265705 `E`, the `<video>`'s
//! `aria-label`). Here the same text is the viewer's row and the accessible name of the attachment picture and of the
//! GIFV embed. Not part of the pinned client.

use std::borrow::Cow;

const ID: &str = "BetterGifAltText";

/// `altify`: the alt text of a picture of `content_type` whose own alt is `alt`, from the file at `url`. Anything
/// that is not a GIF, or already has its own alt text, keeps it.
///
/// One change from the original: the query and fragment of the address are left out. The original reads the name
/// from the address as it is, and Discord's attachment addresses now end in a signed `?ex=...&is=...&hm=...` that
/// would end up in the words.
fn altify<'a>(content_type: Option<&str>, alt: Option<&'a str>, url: &str) -> Option<Cow<'a, str>> {
    if content_type.is_some_and(|kind| !kind.is_empty() && kind != "image/gif") {
        return alt.map(Cow::Borrowed);
    }
    let alt = alt.unwrap_or("GIF");
    if alt != "GIF" {
        return Some(Cow::Borrowed(alt));
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    // `decodeURI`; an address that does not decode is used as it is.
    let decoded = percent_encoding::percent_decode_str(path)
        .decode_utf8()
        .map_or_else(|_| path.to_owned(), Cow::into_owned);
    let last = decoded.rsplit('/').next().unwrap_or("");
    let stripped: String = last.chars().filter(|c| !c.is_ascii_digit()).collect();
    let stripped = stripped.strip_suffix(".gif").unwrap_or(&stripped);
    // `.split(/[,\-_ ]+/g).slice(0, 20).join(" ")`: a JavaScript split keeps the empty pieces at both ends.
    let mut pieces = Vec::new();
    let mut piece = String::new();
    let mut in_separator = false;
    for c in stripped.chars() {
        if matches!(c, ',' | '-' | '_' | ' ') {
            if !in_separator {
                pieces.push(std::mem::take(&mut piece));
            }
            in_separator = true;
        } else {
            piece.push(c);
            in_separator = false;
        }
    }
    pieces.push(piece);
    let mut name = pieces.into_iter().take(20).collect::<Vec<_>>().join(" ");
    // `name.length > 300`: JavaScript counts UTF-16 units.
    if name.encode_utf16().count() > 300 {
        let mut kept = String::new();
        let mut units = 0;
        for c in name.chars() {
            units += c.len_utf16();
            if units > 300 {
                break;
            }
            kept.push(c);
        }
        name = kept + "...";
    }
    Some(Cow::Owned(if name.is_empty() {
        alt.to_owned()
    } else {
        format!("{alt} - {name}")
    }))
}

fn on() -> bool {
    !crate::plugins::bypassed() && crate::plugins::on(ID)
}

/// The alt text of a picture: `alt` as it is while the plugin is off, else [`altify`]'s.
pub(in crate::ui) fn better_gif_alt_text<'a>(
    content_type: Option<&str>,
    alt: Option<&'a str>,
    url: &str,
) -> Option<Cow<'a, str>> {
    if !on() {
        return alt.map(Cow::Borrowed);
    }
    crate::plugins::note(ID);
    altify(content_type, alt, url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(content_type: Option<&str>, alt: Option<&str>, url: &str) -> Option<String> {
        altify(content_type, alt, url).map(Cow::into_owned)
    }

    #[test]
    fn a_gif_reads_the_words_of_its_name() {
        assert_eq!(
            text(
                Some("image/gif"),
                None,
                "https://cdn.discordapp.com/attachments/1/2/happy_cat-dance2024.gif?ex=1&is=2&hm=ab"
            )
            .as_deref(),
            Some("GIF - happy cat dance")
        );
        // A GIFV embed reads its page address, whose name ends in a separator after the digits go.
        assert_eq!(
            text(None, None, "https://tenor.com/view/cat-dance-gif-12345").as_deref(),
            Some("GIF - cat dance gif ")
        );
        assert_eq!(
            text(None, None, "https://example.com/My%20Cat.gif").as_deref(),
            Some("GIF - My Cat")
        );
        // Only digits: no words.
        assert_eq!(
            text(None, None, "https://example.com/123.gif").as_deref(),
            Some("GIF")
        );
    }

    #[test]
    fn other_pictures_and_written_alt_text_stay() {
        assert_eq!(text(Some("image/png"), None, "https://x/a_b.png"), None);
        assert_eq!(
            text(Some("image/png"), Some("mine"), "https://x/a_b.png").as_deref(),
            Some("mine")
        );
        assert_eq!(
            text(Some("image/gif"), Some("a dog"), "https://x/a_b.gif").as_deref(),
            Some("a dog")
        );
    }

    #[test]
    fn at_most_twenty_words_and_three_hundred_units() {
        let long = (0..30).map(|_| "word").collect::<Vec<_>>().join("-");
        let alt = text(None, None, &format!("https://x/{long}.gif")).unwrap();
        assert_eq!(alt, format!("GIF - {}", vec!["word"; 20].join(" ")));
        let huge = "a".repeat(400);
        let alt = text(None, None, &format!("https://x/{huge}.gif")).unwrap();
        assert_eq!(alt, format!("GIF - {}...", "a".repeat(300)));
    }

    #[test]
    fn plugin_off_keeps_the_alt_as_it_is() {
        assert_eq!(
            better_gif_alt_text(Some("image/gif"), None, "https://x/cat.gif"),
            None
        );
    }
}
