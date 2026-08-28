//! A site's render config, read off the **term node** its gate names.
//!
//! A gate is a field and a value. When the archive declares that field's values
//! against a *reified* vocabulary — `fields.audience.vocabulary` pointing at an
//! index node, `reify: true` — the value is not a string in a table: it is a
//! document. That document is where the render-facing half of a site belongs,
//! because it is the one place in the archive that already means "this
//! audience", with a body to write down who they are and backlinks saying what
//! is published to them.
//!
//! ```yaml
//! # vocab/public.md
//! ---
//! title: Public
//! term: public
//! part_of: '[Audiences](/vocab/audiences.md)'
//! front_page: '[plates](/README.md)'
//! site:
//!   shell: .config/sites/docs/shell.html
//!   stylesheet: .config/sites/docs/style.css
//!   lang: en
//! ---
//! ```
//!
//! # Why this is not the caller's dialect
//!
//! The rest of a site's declaration arrives as a [`SiteSpec`](crate::SiteSpec) the
//! caller built, deliberately (see [`crate`]'s module docs). This pass is
//! different in kind:
//! it reads no vocabulary of its own. `fields`, `vocabulary`, `reify`, the
//! spanning relation the terms hang off and the term key itself are all prov's,
//! and the *only* keys named here are the two that carry payload prov declines
//! to interpret — [`FRONT_PAGE_KEY`] and [`TERM_SITE_KEY`]. So it needs a
//! workspace and prov's config rather than a config file, which is why it lives
//! beside [`crate::plan`] and [`crate::theme`] rather than in a binary.
//!
//! # Why the render keys are namespaced and the front page is not
//!
//! Tier-3 payload is unnamespaced by nature: a term node's frontmatter is the
//! author's, and a bare `shell:` on it is claimed by convention only — it
//! collides with a field the archive already had, and with the second consumer
//! that wants one. So everything plates reads for rendering sits under one
//! obviously-someone's-block key, `site:`, the way prov's own config nests under
//! `prov:`.
//!
//! `front_page:` is exempt because it is not payload. It is a **relation**, and
//! a declared relation is its own namespace registration: an archive that
//! declares it under `relations:` gets an inverse prov maintains and a target
//! prov checks, and one that has not still means the same thing by it. Nothing
//! here depends on which — the link is resolved through prov's link layer
//! either way.
//!
//! # Silence is the diagnosis
//!
//! Every "no" here is quiet: no field spec, no vocabulary, not reified, or no
//! term node carrying the gate's value. An unknown value on a closed field is
//! `prov check`'s `UnknownTerm` finding, on the field where a typo is a privacy
//! failure; repeating it here would say the same thing twice, in worse words,
//! at a moment when the author asked for a website rather than an audit.
//! [`TermConfig::warnings`] carries only what the term node *did* say and this
//! pass could not use.

use std::collections::BTreeMap;
use std::path::Path;

use prov::config::FieldSpec;
use prov::link::{Link, LinkStyle};
use prov::meta::{Mapping, Value};
use prov::{IdIndex, Storage, Target, Workspace};

/// The top-level key on a term node naming the site's front page.
///
/// A relation, not a setting — which is the whole reason it is spelled at top
/// level while everything else plates reads is under [`TERM_SITE_KEY`]. An
/// archive that declares it under `relations:` gets the inverse maintained and
/// the target checked; one that has not still means the same thing by it, and
/// this pass resolves it the same way either way.
pub const FRONT_PAGE_KEY: &str = "front_page";

/// The key on a term node the render settings are nested under.
pub const TERM_SITE_KEY: &str = "site";

/// The keys plates reads inside a term node's `site:` mapping.
///
/// Exported for the reason prov exports its own key lists: a key nobody reads is
/// invisible by design, and a listing of what *is* read is where that silence
/// gets broken. `label` is deliberately absent — the site's name is the site's,
/// and it comes from the export.
pub const TERM_SITE_KEYS: &[&str] = &["shell", "stylesheet", "lang", "syntaxes"];

/// What a term node contributes to the site gated on it.
///
/// Every field is what the term node said, unresolved and unread: a path is
/// still a path, and the front page is still a link. Folding this into a
/// [`SiteSpec`](crate::SiteSpec) is the caller's, and reading the files
/// [`crate::read_theme`]'s, because both already happen once per site whether a
/// term node exists or not.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TermConfig {
    /// The front page, already resolved against the **term node** and respelled
    /// root-absolute (`/README.md`) — see [`read_term_config`] for why the
    /// resolution happens here and the spelling travels.
    pub index: Option<String>,
    /// The site's shell template, as a vault-relative path.
    pub shell: Option<String>,
    /// The site's stylesheet, as a vault-relative path.
    pub stylesheet: Option<String>,
    /// BCP 47 language tag for every page's `<html lang="…">`.
    pub lang: Option<String>,
    /// Extra grammars, as vault-relative paths, in declaration order.
    pub syntaxes: Vec<String>,
    /// What the term node said that this pass could not use, in the words of
    /// whoever has to fix it. Never fatal — a misspelled key costs a site one
    /// setting.
    pub warnings: Vec<String>,
}

/// Read the render config a site's gate value carries on its term node.
///
/// `fields` is prov's own `fields:` declaration
/// ([`WorkspaceConfig::fields`](prov::config::WorkspaceConfig::fields)), and
/// `gate_field`/`gate_value` are the site's gate —
/// [`SiteSpec::gate_field`](crate::SiteSpec::gate_field) and
/// [`SiteSpec::audience`](crate::SiteSpec::audience). An archive that declares
/// no vocabulary for that field, or one that is not reified, gets
/// [`TermConfig::default`] and no complaint: it is the case every archive is in
/// until it wants one custom thing.
///
/// The front page is resolved **here**, against the term node, because that is
/// the document the link was written in — `front_page: '[Home](daily.md)'` on
/// `vocab/public.md` means `vocab/daily.md`, and handing the text on to
/// [`plan_site`](crate::plan_site) would resolve it against the root document
/// instead and silently front the site with a different page. What travels is
/// the resolved path in prov's root-absolute spelling, which resolves to itself
/// from any base. A link that resolves to no path at all travels as written, so
/// that the site still fails with [`Error::UnresolvedSiteIndex`] naming the site
/// and the target its author typed.
///
/// [`Error::UnresolvedSiteIndex`]: crate::Error::UnresolvedSiteIndex
pub async fn read_term_config<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    root_doc: &Path,
    fields: &BTreeMap<String, FieldSpec>,
    gate_field: &str,
    gate_value: &str,
) -> TermConfig {
    let Some(spec) = fields.get(gate_field) else {
        return TermConfig::default();
    };
    // `reify` is the load-bearing half. A flat vocabulary's terms are rows in a
    // store, with nowhere to hang a stylesheet and no node to front a site from,
    // so there is nothing here to read.
    let Some(pointer) = spec.vocabulary.as_deref().filter(|_| spec.reify) else {
        return TermConfig::default();
    };
    // An unreadable archive is not this pass's to report: the same walk is about
    // to be made by the planner, with the whole build's exit code behind it.
    let Ok(Some(term_path)) = ws.reified_term_path(root_doc, pointer, gate_value).await else {
        return TermConfig::default();
    };
    let Ok(term) = ws.graph().document(&term_path).await else {
        return TermConfig::default();
    };

    let index = term
        .meta
        .get(FRONT_PAGE_KEY)
        .map(Value::link_strings)
        .unwrap_or_default()
        .into_iter()
        .find(|raw| !raw.trim().is_empty())
        .map(
            |raw| match ws.resolve_link(&term_path, &Link::parse(&raw)) {
                Target::Path(path) => {
                    prov::link::path_text(LinkStyle::PlainRoot, &term_path, &path)
                }
                _ => raw,
            },
        );

    let mut warnings = Vec::new();
    let site = match term.meta.get(TERM_SITE_KEY) {
        None => None,
        Some(value) => match value.as_mapping() {
            Some(map) => Some(map.clone()),
            None => {
                warnings.push(format!(
                    "{}: `{TERM_SITE_KEY}` is not a mapping of settings, so it was ignored",
                    term_path.display()
                ));
                None
            }
        },
    };
    let Some(site) = site else {
        return TermConfig {
            index,
            warnings,
            ..TermConfig::default()
        };
    };

    for key in site.keys() {
        if !TERM_SITE_KEYS.contains(&key.as_str()) {
            warnings.push(format!(
                "{}: `{TERM_SITE_KEY}.{key}` is not a setting plates reads (it reads {})",
                term_path.display(),
                TERM_SITE_KEYS.join(", ")
            ));
        }
    }

    TermConfig {
        index,
        shell: text(&site, "shell"),
        stylesheet: text(&site, "stylesheet"),
        lang: text(&site, "lang"),
        syntaxes: text_list(&site, "syntaxes"),
        warnings,
    }
}

/// A non-empty string setting, trimmed. An empty one is treated as absent,
/// because `shell:` with nothing after it is how a declaration says "not this
/// one" while leaving the key in place to remember it existed.
///
/// Public because the `sites:` block a caller may still read spells its settings
/// by the same rules, and two copies of this would be two rules the day one of
/// them was fixed. It stops being shared when that block goes.
pub fn text(entry: &Mapping, key: &str) -> Option<String> {
    entry
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A list-of-paths setting: a sequence, or a bare scalar for the one-item case.
///
/// Both spellings because `syntaxes: .config/sites/blog/wat.sublime-syntax` is
/// what someone with one grammar writes, and making them punctuate it as a list
/// to be understood is a paper cut with no purpose. Entries that are empty,
/// blank or not strings are dropped on the same reasoning as [`text`]: a key
/// left in place with nothing under it is a declaration remembering something
/// used to be there.
pub fn text_list(entry: &Mapping, key: &str) -> Vec<String> {
    let Some(value) = entry.get(key) else {
        return Vec::new();
    };
    match value.as_sequence() {
        Some(items) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        None => text(entry, key).into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(pairs: &[(&str, Value)]) -> Mapping {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    fn s(text: &str) -> Value {
        Value::String(text.to_string())
    }

    /// `shell:` with nothing after it is a declaration remembering that the key
    /// exists, not a request for a template named "".
    #[test]
    fn an_empty_setting_reads_as_absent() {
        assert_eq!(text(&mapping(&[("shell", s("   "))]), "shell"), None);
        assert_eq!(
            text(&mapping(&[("shell", s(" a.html "))]), "shell"),
            Some("a.html".to_string())
        );
    }

    /// One grammar needs no list punctuation around it, and a blank entry
    /// declares nothing.
    #[test]
    fn a_lone_syntax_may_be_written_as_a_scalar() {
        assert_eq!(
            text_list(
                &mapping(&[("syntaxes", s(" a.sublime-syntax "))]),
                "syntaxes"
            ),
            vec!["a.sublime-syntax"]
        );
        assert_eq!(
            text_list(
                &mapping(&[(
                    "syntaxes",
                    Value::Sequence(vec![s("  "), s("real.sublime-syntax"), Value::Null]),
                )]),
                "syntaxes"
            ),
            vec!["real.sublime-syntax"]
        );
        assert!(text_list(&mapping(&[]), "syntaxes").is_empty());
    }
}
