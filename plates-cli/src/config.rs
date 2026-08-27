//! The `sites:` vocabulary — how *this* application spells a site declaration.
//!
//! [`plates::SiteSpec`] arrives at the library already built, on purpose: which
//! block an archive declares its sites in is a dialect, not a site's shape, and
//! two applications over the same archive format should be able to disagree
//! about the spelling and still produce the same website. So the spelling lives
//! here, in the binary, and is the one thing in this workspace that a different
//! host would replace wholesale.
//!
//! # Where it is read from
//!
//! The same two surfaces prov reads its own config from, in the same order —
//! the root document's frontmatter first, then the config document the root
//! links to, which wins:
//!
//! ```yaml
//! # In the config document, beside prov's own `views:` and `exports:`.
//! sites:
//!   blog:
//!     label: Field notes
//!     audience: public
//!     view: daily
//!     index: '[Home](id:7f3a91c)'
//!     shell: .config/sites/blog/shell.html
//!     stylesheet: .config/sites/blog/style.css
//!     lang: en
//! ```
//!
//! `audience` is the only required key: a site that does not say who it is for
//! is not a site. Everything else has a defensible default, and the defaults are
//! [`plates::SiteSpec`]'s.
//!
//! `gate_field` is the exception worth naming, because it decides what
//! `audience` is *compared against*: an archive whose disclosure control is
//! spelled `clearance` writes `gate_field: clearance` and its sites gate on
//! that. Absent, it is [`plates::AUDIENCE_FIELD`], which is what every archive
//! that never had to think about it wants.
//!
//! # The fallback, and why it is not a guess
//!
//! An archive that declares no `sites:` block gets one site per prov `exports:`
//! entry whose gate is on the [`plates::AUDIENCE_FIELD`] — same name, same
//! label, same view, same gate value. That is not plates inventing a site: an
//! export already *is* a named, closed set of documents that may leave the
//! archive, which is the whole of what a site needs to exist. What it lacks is
//! only the render-facing half — a front page, a shell, a stylesheet — and every
//! one of those has a default.
//!
//! An export gated on some *other* field is skipped rather than published under
//! a rule nothing showed anyone, and named in the warnings so the omission is
//! visible. Publishing it takes a `sites:` entry that says `gate_field:` out
//! loud — which is the point: the derivation stays the case nobody had to
//! think about.

use std::path::Path;

use plates::prov::meta::{Mapping, Value};
use plates::prov::{Document, ExportSpec};
use plates::{AUDIENCE_FIELD, SiteSpec};

/// The top-level key a site declaration lives under.
///
/// Deliberately far enough from every prov config axis that prov's own
/// near-miss lint reads it as a key belonging to someone else rather than as a
/// typo of one of its own.
pub const SITES_KEY: &str = "sites";

/// The keys valid inside one `sites.<name>` entry.
///
/// Exported for the same reason prov exports its own: a key nobody reads is
/// invisible by design, and a listing of what *is* read is where that silence
/// gets broken.
pub const SITE_KEYS: &[&str] = &[
    "label",
    "audience",
    "gate_field",
    "view",
    "index",
    "shell",
    "stylesheet",
    "lang",
    "syntaxes",
];

/// Every site the archive declares, plus what could not be read.
///
/// Warnings rather than errors throughout: a misspelled key costs a site one
/// setting, and refusing to build the other four sites over it would be a
/// worse answer than saying so. The one hard failure is a site with no
/// `audience`, which is not a site with a missing setting — it is a set of
/// documents with no gate, and publishing one is the mistake this whole layer
/// exists to make impossible.
#[derive(Debug, Default)]
pub struct Sites {
    pub specs: Vec<SiteSpec>,
    /// Where the declarations came from, for a message that has to say which
    /// file to edit.
    pub source: Source,
    pub warnings: Vec<String>,
}

/// Which surface a site list was read off.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// A `sites:` block, declared by hand.
    Declared,
    /// Derived from prov's own `exports:`, because no `sites:` block exists.
    Exports,
    /// Neither — the archive declares nothing to publish.
    #[default]
    None,
}

/// Read the archive's site declarations.
///
/// `root_dir` is the workspace root, `root_doc` the root document relative to
/// it, and `config_doc` the config document the root links to (relative to the
/// root) when there is one. The precedence — root frontmatter first, config
/// document over it — is prov's own, restated here so a reader who knows where
/// `views:` goes already knows where `sites:` goes.
pub fn read_sites(
    root_dir: &Path,
    root_doc: &Path,
    config_doc: Option<&Path>,
    exports: &[ExportSpec],
) -> Sites {
    let mut warnings = Vec::new();
    let mut block: Option<Mapping> = None;

    for rel in [Some(root_doc), config_doc].into_iter().flatten() {
        let Some(meta) = read_meta(root_dir, rel) else {
            continue;
        };
        let Some(value) = meta.get(SITES_KEY) else {
            continue;
        };
        match value.as_mapping() {
            // Later surface wins outright rather than merging key by key: a
            // half-overridden site declaration is a site nobody wrote, and
            // "the config document decides" is a rule someone can hold in
            // their head.
            Some(map) => block = Some(map.clone()),
            None => warnings.push(format!(
                "{}: `{SITES_KEY}` is not a mapping of site names, so it was ignored",
                rel.display()
            )),
        }
    }

    match block {
        Some(map) => {
            let specs = specs_from(&map, &mut warnings);
            Sites {
                specs,
                source: Source::Declared,
                warnings,
            }
        }
        None => {
            let specs = specs_from_exports(exports, &mut warnings);
            let source = if specs.is_empty() {
                Source::None
            } else {
                Source::Exports
            };
            Sites {
                specs,
                source,
                warnings,
            }
        }
    }
}

/// One document's metadata, or `None` if it could not be read or parsed.
///
/// Silent on failure because prov has already read both of these files by the
/// time this runs: a document that will not parse is a diagnosis prov's own
/// discovery owns, and a second, differently-worded copy of it here would only
/// say the same thing twice.
fn read_meta(root_dir: &Path, rel: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(root_dir.join(rel)).ok()?;
    Some(Document::parse(rel, &text).ok()?.meta)
}

/// Turn a `sites:` mapping into specs, in declaration order.
fn specs_from(map: &Mapping, warnings: &mut Vec<String>) -> Vec<SiteSpec> {
    let mut specs = Vec::new();
    for (name, value) in map {
        let Some(entry) = value.as_mapping() else {
            warnings.push(format!(
                "site `{name}` is not a mapping of settings, so it was skipped"
            ));
            continue;
        };
        for key in entry.keys() {
            if !SITE_KEYS.contains(&key.as_str()) {
                warnings.push(format!(
                    "site `{name}`: `{key}` is not a setting plates reads (it reads {})",
                    SITE_KEYS.join(", ")
                ));
            }
        }
        let Some(audience) = text(entry, "audience") else {
            warnings.push(format!(
                "site `{name}` declares no `audience`, so it was skipped — a site that does \
                 not say who it is for cannot be published"
            ));
            continue;
        };
        specs.push(SiteSpec {
            name: name.clone(),
            label: text(entry, "label"),
            audience,
            gate_field: text(entry, "gate_field"),
            view: text(entry, "view"),
            index: text(entry, "index"),
            shell: text(entry, "shell"),
            stylesheet: text(entry, "stylesheet"),
            lang: text(entry, "lang"),
            syntaxes: text_list(entry, "syntaxes"),
        });
    }
    specs
}

/// A site per `exports:` entry, for an archive that declares no `sites:` block.
fn specs_from_exports(exports: &[ExportSpec], warnings: &mut Vec<String>) -> Vec<SiteSpec> {
    let mut specs = Vec::new();
    for export in exports {
        if export.gate.field != AUDIENCE_FIELD {
            warnings.push(format!(
                "export `{}` gates on `{}` rather than `{AUDIENCE_FIELD}`, so no site was \
                 derived from it — declare one under `{SITES_KEY}` with `gate_field: {}` to \
                 publish it",
                export.name, export.gate.field, export.gate.field
            ));
            continue;
        }
        specs.push(SiteSpec {
            name: export.name.clone(),
            label: export.label.clone(),
            audience: export.gate.value.clone(),
            gate_field: None,
            view: export.view.clone(),
            index: None,
            shell: None,
            stylesheet: None,
            lang: None,
            syntaxes: Vec::new(),
        });
    }
    specs
}

/// A non-empty string setting, trimmed. An empty one is treated as absent,
/// because `shell:` with nothing after it is how a declaration says "not this
/// one" while leaving the key in place to remember it existed.
fn text(entry: &Mapping, key: &str) -> Option<String> {
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
/// what someone with one grammar writes, and making them punctuate it as a
/// list to be understood is a paper cut with no purpose. Entries that are
/// empty, blank or not strings are dropped on the same reasoning as [`text`]:
/// a key left in place with nothing under it is a declaration remembering
/// something used to be there.
fn text_list(entry: &Mapping, key: &str) -> Vec<String> {
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
    use plates::prov::exports::Gate;

    fn export(name: &str, field: &str, value: &str) -> ExportSpec {
        ExportSpec {
            name: name.to_string(),
            label: None,
            gate: Gate {
                field: field.to_string(),
                value: value.to_string(),
            },
            view: None,
        }
    }

    fn mapping(pairs: &[(&str, Value)]) -> Mapping {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    fn s(text: &str) -> Value {
        Value::String(text.to_string())
    }

    /// The whole vocabulary in one entry, so a key that stops being read fails
    /// here rather than in someone's site.
    #[test]
    fn every_declared_setting_reaches_the_spec() {
        let mut warnings = Vec::new();
        let block = mapping(&[(
            "blog",
            Value::Mapping(mapping(&[
                ("label", s("Field notes")),
                ("audience", s("public")),
                ("gate_field", s("clearance")),
                ("view", s("daily")),
                ("index", s("[Home](id:7f3a91c)")),
                ("shell", s(".config/sites/blog/shell.html")),
                ("stylesheet", s(".config/sites/blog/style.css")),
                ("lang", s("fr")),
                (
                    "syntaxes",
                    Value::Sequence(vec![s(".config/sites/blog/wat.sublime-syntax")]),
                ),
            ])),
        )]);

        let specs = specs_from(&block, &mut warnings);

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            specs,
            vec![SiteSpec {
                name: "blog".into(),
                label: Some("Field notes".into()),
                audience: "public".into(),
                gate_field: Some("clearance".into()),
                view: Some("daily".into()),
                index: Some("[Home](id:7f3a91c)".into()),
                shell: Some(".config/sites/blog/shell.html".into()),
                stylesheet: Some(".config/sites/blog/style.css".into()),
                lang: Some("fr".into()),
                syntaxes: vec![".config/sites/blog/wat.sublime-syntax".into()],
            }]
        );
    }

    /// One grammar needs no list punctuation around it.
    #[test]
    fn a_lone_syntax_may_be_written_as_a_scalar() {
        let entry = mapping(&[("syntaxes", s(" a.sublime-syntax "))]);
        assert_eq!(text_list(&entry, "syntaxes"), vec!["a.sublime-syntax"]);
    }

    /// The same rule `text` applies, applied per entry: a key left in place
    /// with nothing under it declares nothing.
    #[test]
    fn blank_syntax_entries_are_dropped() {
        let entry = mapping(&[(
            "syntaxes",
            Value::Sequence(vec![s("  "), s("real.sublime-syntax"), Value::Null]),
        )]);
        assert_eq!(text_list(&entry, "syntaxes"), vec!["real.sublime-syntax"]);
        assert!(text_list(&mapping(&[]), "syntaxes").is_empty());
    }

    /// A gate is not a setting with a default. A site missing one is dropped
    /// and named, never published to whoever the first audience happens to be.
    #[test]
    fn a_site_with_no_audience_is_refused_and_named() {
        let mut warnings = Vec::new();
        let block = mapping(&[
            ("open", Value::Mapping(mapping(&[("label", s("Open"))]))),
            (
                "blog",
                Value::Mapping(mapping(&[("audience", s("public"))])),
            ),
        ]);

        let specs = specs_from(&block, &mut warnings);

        assert_eq!(specs.len(), 1, "the one that declared a gate");
        assert_eq!(specs[0].name, "blog");
        assert!(
            warnings.iter().any(|w| w.contains("open")),
            "and the other is named: {warnings:?}"
        );
    }

    /// A key plates does not read is invisible everywhere else, so the listing
    /// is where the silence gets broken — without costing the site its build.
    #[test]
    fn an_unread_key_is_reported_but_never_fatal() {
        let mut warnings = Vec::new();
        let block = mapping(&[(
            "blog",
            Value::Mapping(mapping(&[
                ("audience", s("public")),
                ("stylesheat", s("style.css")),
            ])),
        )]);

        let specs = specs_from(&block, &mut warnings);

        assert_eq!(specs.len(), 1);
        assert!(
            warnings.iter().any(|w| w.contains("stylesheat")),
            "{warnings:?}"
        );
    }

    /// An export is already a named, gated set that may leave the archive.
    /// Deriving a site from one adds no disclosure — only a shell it does not
    /// have.
    #[test]
    fn exports_become_sites_when_nothing_declares_one() {
        let mut warnings = Vec::new();
        let specs = specs_from_exports(
            &[
                export("blog", AUDIENCE_FIELD, "public"),
                export("audit", "clearance", "internal"),
            ],
            &mut warnings,
        );

        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].audience, "public");
        assert!(
            warnings.iter().any(|w| w.contains("audit")),
            "an export gated on another field is skipped and said out loud: {warnings:?}"
        );
    }

    /// `shell:` with nothing after it is a declaration remembering that the key
    /// exists, not a request for a template named "".
    #[test]
    fn an_empty_setting_reads_as_absent() {
        let entry = mapping(&[("shell", s("   "))]);
        assert_eq!(text(&entry, "shell"), None);
    }
}
