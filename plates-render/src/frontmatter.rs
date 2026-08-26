//! Reading the metadata block of a source document.
//!
//! A thin reading layer over [`prov::Document`]: rendering only ever *reads*
//! frontmatter, so the write half — serialize, set or remove a property, splice
//! a body — has no counterpart here. A caller that needs it has prov's editor.

use prov::{Mapping, Value};

/// A path handed to [`prov::Document::parse`] purely to steer it away from
/// *whole-file* metadata detection (`.yaml`/`.json`/`.figl` extensions — see
/// `prov::document::whole_file_format`). A source document's own path is not
/// used: rendering is handed text whose metadata is always a fenced block,
/// never a bare config file.
///
/// It stays `.md` even now that a body may be Djot or HTML, and that is not an
/// oversight. The collector re-fences every gathered source's metadata as delimited
/// YAML regardless of the carrier the document had on disk (`plates::collect`),
/// so what arrives here is always a `---` block — which is exactly what this extension tells prov to expect. The
/// *body's* grammar is read from the real path, one layer up in
/// [`crate::site`], and never from this.
const DOC_PATH: &str = "frontmatter.md";

/// A document split into its metadata mapping and its body.
#[derive(Debug, Clone, Default)]
pub struct ParsedFile {
    /// The metadata block as an ordered map. Empty when the document has none.
    pub frontmatter: Mapping,
    /// Everything after the metadata block.
    pub body: String,
}

/// Parse a document, treating "no metadata block" as an empty one.
///
/// Only a malformed metadata block is an error; a document with none at all
/// parses to an empty mapping and a body of the whole text.
pub fn parse_or_empty(content: &str) -> Result<ParsedFile, prov::Error> {
    let doc = prov::Document::parse(DOC_PATH, content)?;
    Ok(ParsedFile {
        frontmatter: doc.meta.as_mapping().cloned().unwrap_or_default(),
        body: doc.body,
    })
}

/// A string-valued property, when present and actually a string.
pub fn get_string<'a>(frontmatter: &'a Mapping, key: &str) -> Option<&'a str> {
    frontmatter.get(key).and_then(|v| v.as_str())
}

/// A sequence-valued property, as its string elements. Empty when the key is
/// absent, is not a sequence, or holds no strings.
pub fn get_string_array(frontmatter: &Mapping, key: &str) -> Vec<String> {
    match frontmatter.get(key) {
        Some(Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_metadata_block_is_an_empty_mapping_and_a_whole_body() {
        let parsed = parse_or_empty("# Just a heading\n").unwrap();
        assert!(parsed.frontmatter.is_empty());
        assert_eq!(parsed.body, "# Just a heading\n");
    }

    #[test]
    fn reads_scalars_and_sequences() {
        let parsed =
            parse_or_empty("---\ntitle: Hi\ncontents:\n  - a.md\n  - b.md\n---\n\nbody\n").unwrap();
        assert_eq!(get_string(&parsed.frontmatter, "title"), Some("Hi"));
        assert_eq!(
            get_string_array(&parsed.frontmatter, "contents"),
            vec!["a.md".to_string(), "b.md".to_string()]
        );
        assert_eq!(parsed.body.trim(), "body");
    }

    #[test]
    fn a_missing_or_wrongly_typed_key_reads_as_absent() {
        let parsed = parse_or_empty("---\ntitle: Hi\ncount: 3\n---\n").unwrap();
        assert_eq!(get_string(&parsed.frontmatter, "nope"), None);
        // A non-string scalar is not a string.
        assert_eq!(get_string(&parsed.frontmatter, "count"), None);
        // A non-sequence is not a sequence.
        assert!(get_string_array(&parsed.frontmatter, "title").is_empty());
    }
}
