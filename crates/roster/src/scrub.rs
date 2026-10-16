//! Strips prose out of a `.ros` file so it can be committed as a fixture.
//!
//! The output is the input byte-for-byte except that:
//! - every `<description>` is emptied;
//! - every `Description` characteristic is emptied, except the `Leader`
//!   ability's, which keeps only the comma-separated unit list;
//! - a `Transport` profile's `Capacity` keeps only the number.
//!
//! [`crate::parse_ros`] reads a scrubbed file to the same `Army` as the
//! original.

use crate::{parse_capacity, parse_leader_list, RosterError};
use roxmltree::{Document, Node};
use std::ops::Range;

/// Returns the scrubbed document.
pub fn scrub(xml: &str) -> Result<String, RosterError> {
    let doc = Document::parse(xml)?;
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();

    for node in doc.descendants().filter(|n| n.is_element()) {
        let replacement = match node.tag_name().name() {
            "description" => String::new(),
            "characteristic" => match (node.attribute("name"), profile_of(node)) {
                (Some("Description"), Some(p)) => {
                    if p.attribute("typeName") == Some("Abilities")
                        && p.attribute("name") == Some("Leader")
                    {
                        parse_leader_list(node.text().unwrap_or("")).join(", ")
                    } else {
                        String::new()
                    }
                }
                (Some("Capacity"), Some(p)) if p.attribute("typeName") == Some("Transport") => {
                    parse_capacity(node.text().unwrap_or(""))
                        .map(|n| n.to_string())
                        .unwrap_or_default()
                }
                _ => continue,
            },
            _ => continue,
        };
        if let Some(range) = content_range(node) {
            edits.push((range, escape(&replacement)));
        }
    }

    edits.sort_by_key(|(r, _)| r.start);
    let mut out = String::with_capacity(xml.len());
    let mut pos = 0;
    for (range, text) in edits {
        out.push_str(&xml[pos..range.start]);
        out.push_str(&text);
        pos = range.end;
    }
    out.push_str(&xml[pos..]);
    Ok(out)
}

/// The `<profile>` two levels above a `<characteristic>`.
fn profile_of<'a, 'i>(characteristic: Node<'a, 'i>) -> Option<Node<'a, 'i>> {
    characteristic
        .parent_element()
        .and_then(|c| c.parent_element())
        .filter(|p| p.tag_name().name() == "profile")
}

/// The byte range of everything between an element's start and end tags, or
/// `None` for an element with no children.
fn content_range(node: Node) -> Option<Range<usize>> {
    let first = node.first_child()?;
    let last = node.last_child()?;
    Some(first.range().start..last.range().end)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
