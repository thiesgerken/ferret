//! One mail as a document: identity, thread, metadata and its text without quotes.

use chrono::{DateTime, Utc};
use ferret_api::{ContentType, DocumentPut};
use mail_parser::{Addr, Address, HeaderValue, Message, MessageParser, MimeHeaders};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, json};

use super::quotes;

/// What `{message_id}` leaves unencoded: enough for `mid:` URLs (RFC 2392) to stay readable.
const MESSAGE_ID: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'@');

/// A Message-ID as ferret keeps it: without the angle brackets.
pub fn message_id(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_owned()
}

/// When a mail was written according to its `Date` header, as mail-parser reads it.
pub fn written(date: &str) -> Option<DateTime<Utc>> {
    let date = mail_parser::DateTime::parse_rfc822(date)?;
    DateTime::from_timestamp(date.to_timestamp(), 0)
}

/// The document of a mail, `id` being its Message-ID (or what stands in for it) and `written`
/// its date as the listing saw it, for mails without a readable `Date`. The text is stripped of quote containers and
/// markers, the thread diff is up to the caller. `None` when `raw` isn't a mail.
pub fn document(
    raw: &[u8],
    id: &str,
    written: DateTime<Utc>,
    source: &str,
    url_template: Option<&str>,
) -> Option<DocumentPut> {
    let message = MessageParser::default().parse(raw)?;
    let created_at = message
        .date()
        .and_then(|date| DateTime::from_timestamp(date.to_timestamp(), 0))
        .unwrap_or(written);
    let mut meta = Map::new();
    for (key, addresses) in [("to", message.to()), ("cc", message.cc())] {
        let addresses: Vec<String> = addresses.into_iter().flat_map(with_addresses).collect();
        if !addresses.is_empty() {
            meta.insert(key.to_owned(), json!(addresses));
        }
    }
    let attachments: Vec<&str> = message
        .attachments()
        .filter_map(|part| part.attachment_name())
        .collect();
    if !attachments.is_empty() {
        meta.insert("attachments".to_owned(), json!(attachments));
    }
    Some(DocumentPut {
        source: source.to_owned(),
        external_id: id.to_owned(),
        content_type: ContentType::Email,
        title: message.subject().map(str::to_owned),
        text: text(&message),
        url: url_template.map(|template| {
            template.replace(
                "{message_id}",
                &utf8_percent_encode(id, MESSAGE_ID).to_string(),
            )
        }),
        thread: Some(thread(&message).unwrap_or_else(|| id.to_owned())),
        authors: message.from().into_iter().flat_map(names).collect(),
        created_at,
        modified_at: created_at,
        meta,
    })
}

/// The first Message-ID of `References`, else `In-Reply-To`: the thread's first mail as far as
/// the mail knows it.
fn thread(message: &Message) -> Option<String> {
    let first = |value: &HeaderValue| match value {
        HeaderValue::Text(id) => Some(message_id(id)),
        HeaderValue::TextList(ids) => ids.first().map(|id| message_id(id)),
        _ => None,
    };
    first(message.references()).or_else(|| first(message.in_reply_to()))
}

/// The body's text without quotes. Forwards keep theirs: the forwarded mail is new to the reader.
fn text(message: &Message) -> String {
    let forward = message.subject().is_some_and(is_forward);
    // HTML when there is some: its quote containers are more reliable than text markers.
    let has_html = message.html_bodies().any(|part| part.is_text_html());
    let bodies: Vec<String> = if has_html {
        (0..message.html_body_count())
            .filter_map(|at| message.body_html(at))
            .map(|html| quotes::html_text(&html, forward))
            .collect()
    } else {
        (0..message.text_body_count())
            .filter_map(|at| message.body_text(at))
            .map(|text| text.into_owned())
            .collect()
    };
    let text = bodies.join("\n\n");
    if forward {
        text.trim().to_owned()
    } else {
        quotes::without_quote_markers(&text)
    }
}

fn is_forward(subject: &str) -> bool {
    let subject = subject.trim_start().to_lowercase();
    ["fwd:", "fw:", "wg:"]
        .iter()
        .any(|prefix| subject.starts_with(prefix))
}

/// The authors' names, or their addresses where a name is missing.
fn names<'a>(address: &'a Address) -> impl Iterator<Item = String> + 'a {
    address
        .iter()
        .filter_map(|addr| addr.name().or(addr.address()).map(str::to_owned))
}

/// `Name <address>`, or what of it there is.
fn with_addresses<'a>(address: &'a Address) -> impl Iterator<Item = String> + 'a {
    address
        .iter()
        .filter_map(|addr: &Addr| match (addr.name(), addr.address()) {
            (Some(name), Some(address)) => Some(format!("{name} <{address}>")),
            (name, address) => name.or(address).map(str::to_owned),
        })
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;
    use serde_json::Value;

    use super::*;

    fn fixture(name: &str) -> DocumentPut {
        let raw = std::fs::read(format!("fixtures/imap/{name}.eml")).unwrap();
        document(
            &raw,
            "listed-id",
            DateTime::UNIX_EPOCH,
            "mail/test",
            Some("mid:{message_id}"),
        )
        .unwrap()
    }

    #[test]
    fn a_mail_carries_its_text_metadata_and_a_link_back() {
        let original = fixture("01-original");

        assert_eq!(original.external_id, "listed-id");
        assert_eq!(original.content_type, ContentType::Email);
        assert_eq!(
            original.title.as_deref(),
            Some("Hausratversicherung kündigen")
        );
        assert_eq!(
            original.thread.as_deref(),
            Some("listed-id"),
            "a thread of its own"
        );
        assert_eq!(original.authors, ["Alice Example"]);
        assert_eq!(
            original.created_at.to_rfc3339(),
            "2025-01-13T08:12:00+00:00"
        );
        assert_eq!(original.modified_at, original.created_at);
        assert_eq!(original.url.as_deref(), Some("mid:listed-id"));
        assert_eq!(
            Value::Object(original.meta),
            json!({
                "to": ["Bob Builder <bob@gmail.example>"],
                "cc": ["Carol <carol@outlook.example>"],
                "attachments": ["police.pdf"],
            })
        );
        assert!(
            original.text.starts_with("Hallo Bob,\n"),
            "{}",
            original.text
        );
        assert!(
            original.text.ends_with("Viele Grüße\nAlice"),
            "{}",
            original.text
        );
    }

    #[test]
    fn replies_lose_what_they_quote_in_every_client() {
        for (name, text) in [
            (
                "02-gmail-reply",
                "Hi Alice,\n\nklingt gut, die Kündigungsfrist ist drei Monate.\nIch schicke sie heute raus.\n\nBob",
            ),
            (
                "03-apple-mail-reply",
                "Danke dir!\nDann melde ich mich bei der neuen Versicherung.",
            ),
            (
                "04-outlook-reply",
                "Kleiner Hinweis: die neue Police gilt erst ab 01.04.",
            ),
            (
                "05-thunderbird-reply",
                "Gut zu wissen, danke!\nDann lasse ich die alte bis Ende März laufen.",
            ),
            ("10-original-message", "Passt so, bis Dienstag."),
        ] {
            assert_eq!(fixture(name).text, text, "{name}");
        }
    }

    #[test]
    fn a_reply_belongs_to_the_thread_of_its_first_reference() {
        assert_eq!(
            fixture("05-thunderbird-reply").thread.as_deref(),
            Some("original@alice.example")
        );
        assert_eq!(
            fixture("10-original-message").thread.as_deref(),
            Some("termin@alice.example")
        );
    }

    #[test]
    fn an_html_only_mail_keeps_its_lines() {
        assert_eq!(
            fixture("07-html-only").text,
            "Ihre Rechnung\n\nRechnungsnummer: RE-2025-0117\n\nBetrag: 84,20 €\n\nDer Betrag wird am 30.01.2025 abgebucht."
        );
    }

    #[test]
    fn html_non_content_is_removed_even_from_forwards() {
        for subject in ["Invoice", "Fwd: Invoice", "Fw: Invoice", "WG: Invoice"] {
            let raw = format!(
                "From: sender@example.com\r\n\
                 Subject: {subject}\r\n\
                 MIME-Version: 1.0\r\n\
                 Content-Type: text/html; charset=utf-8\r\n\r\n\
                 <html><head>\
                 <style type=\"text/css\" media=\"screen\">body {{ margin: 0; }}</style>\
                 </head><body>\
                 <p>Your invoice.</p>\
                 <style type=\"text/css\">td {{ padding: 4px; }}</style>\
                 <STYLE media=\"print\">p {{ color: red; }}</STYLE>\
                 <script type=\"text/javascript\">alert('not mail content');</script>\
                 <template id=\"receipt\"><div>Unrendered receipt.</div>\
                 <style type=\"text/css\">div {{ display: none; }}</style></template>\
                 <blockquote type=\"cite\"><p>Forwarded content.</p></blockquote>\
                 <p>Amount: 84.20 EUR.</p>\
                 </body></html>"
            );
            let mail = document(
                raw.as_bytes(),
                "html-invoice@example.com",
                DateTime::UNIX_EPOCH,
                "mail/test",
                None,
            )
            .unwrap();
            let expected = if subject == "Invoice" {
                "Your invoice. Amount: 84.20 EUR."
            } else {
                "Your invoice. Forwarded content. Amount: 84.20 EUR."
            };

            assert_eq!(
                mail.text.split_whitespace().collect::<Vec<_>>().join(" "),
                expected,
                "{subject}: {:?}",
                mail.text
            );
        }
    }

    #[test]
    fn a_forward_keeps_the_forwarded_mail() {
        let forward = fixture("08-forward");

        assert!(forward.text.starts_with("Zur Info"), "{}", forward.text);
        assert!(
            forward.text.contains("From: Stadtwerke"),
            "{}",
            forward.text
        );
        assert!(
            forward.text.ends_with("Betrag: 84,20 €"),
            "{}",
            forward.text
        );
    }

    #[test]
    fn a_message_id_is_percent_encoded_in_the_link() {
        let raw = std::fs::read("fixtures/imap/09-no-message-id.eml").unwrap();
        let put = document(
            &raw,
            "a/b c@d",
            DateTime::UNIX_EPOCH,
            "mail/test",
            Some("https://mail.example/?id={message_id}"),
        )
        .unwrap();

        assert_eq!(
            put.url.as_deref(),
            Some("https://mail.example/?id=a%2Fb%20c@d")
        );
    }
}
