//! Which sites an archive declares — read from prov's own `exports:`, plus the
//! deprecated `sites:` block that used to be the spelling.
//!
//! **A site is an export.** prov's `exports.<name>` is already a named, gated
//! set of documents that may leave the archive, which is the whole of what a
//! site needs to exist; what it lacks is only the render-facing half — a front
//! page, a shell, a stylesheet — and that half is written on the **term node**
//! of the gate field's vocabulary, where [`plates::read_term_config`] reads it.
//! Neither surface is plates' invention, so an archive says how its sites are
//! spelled and this binary declares no config vocabulary of its own.
//!
//! ```yaml
//! # The config document, or the root's own `prov:` block.
//! exports:
//!   blog:
//!     label: Field notes
//!     gate: { field: audience, value: public }
//!     view: daily
//! fields:
//!   audience:
//!     values: closed
//!     vocabulary: '[Audiences](/vocab/audiences.md)'
//!     reify: true
//! ```
//!
//! A gate on some field other than [`plates::AUDIENCE_FIELD`] is not a special
//! case here and never was one: reading exports directly, `gate.field:
//! clearance` *is* the gate, and the site carries it in
//! [`plates::SiteSpec::gate_field`].
//!
//! # The deprecated dialect
//!
//! ```yaml
//! sites:
//!   blog:
//!     label: Field notes
//!     audience: public
//!     view: daily
//!     index: '[Home](id:7f3a91c)'
//!     shell: .config/sites/blog/shell.html
//! ```
//!
//! This was the spelling when the argument for it was that a site's declaration
//! is a dialect a different host could replace wholesale. It reads from the same
//! two surfaces prov reads its own config from, in the same order — the root
//! document's frontmatter first, then the config document the root links to,
//! which wins — and when it is present it still wins outright over `exports:`,
//! block for block, so an archive that has not migrated builds exactly what it
//! built before. What it also does now is say so: every site it declares is
//! named alongside the export and term node that replace it. plates 0.3 removes
//! it, along with [`SITES_KEY`], [`SITE_KEYS`] and [`Source`] itself.

use std::path::{Path, PathBuf};

use plates::prov::meta::{Mapping, Value};
use plates::prov::{Document, ExportSpec};
use plates::term::{text, text_list};
use plates::{AUDIENCE_FIELD, SiteSpec};

/// The top-level key the **deprecated** site declaration lives under.
///
/// Deliberately far enough from every prov config axis that prov's own
/// near-miss lint reads it as a key belonging to someone else rather than as a
/// typo of one of its own. Still stripped from every collected document
/// ([`crate::build`]) for as long as an archive may carry one.
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
    /// A `sites:` block — the deprecated dialect, still authoritative where one
    /// exists.
    Declared,
    /// prov's own `exports:` — the ordinary path.
    Exports,
    /// Neither — the archive declares nothing to publish.
    #[default]
    None,
}

/// Read the archive's site declarations.
///
/// `root_dir` is the workspace root, `root_doc` the root document relative to
/// it, and `config_doc` the config document the root links to (relative to the
/// root) when there is one. `exports` is prov's own, already layered by prov's
/// precedence.
///
/// A `sites:` block on either surface wins outright over `exports:` and is
/// deprecated where it wins; the precedence *between* the two surfaces — root
/// frontmatter first, config document over it — is prov's own, restated here so
/// a reader who knows where `views:` goes already knows where `sites:` went.
pub fn read_sites(
    root_dir: &Path,
    root_doc: &Path,
    config_doc: Option<&Path>,
    exports: &[ExportSpec],
) -> Sites {
    let mut warnings = Vec::new();
    let mut block: Option<(PathBuf, Mapping)> = None;

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
            Some(map) => block = Some((rel.to_path_buf(), map.clone())),
            None => warnings.push(format!(
                "{}: `{SITES_KEY}` is not a mapping of site names, so it was ignored",
                rel.display()
            )),
        }
    }

    match block {
        Some((rel, map)) => {
            let specs = specs_from(&map, &mut warnings);
            for spec in &specs {
                warnings.push(deprecation(&rel, spec));
            }
            Sites {
                specs,
                source: Source::Declared,
                warnings,
            }
        }
        None => {
            let specs = specs_from_exports(exports);
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

/// What to say about one site a `sites:` block declares: that it still builds,
/// what replaces it, and when it stops.
///
/// Per site rather than per block, because the replacement is per site — a
/// different export entry and a different term node each time — and a
/// deprecation notice nobody can act on without first working out the mapping
/// themselves is a notice that gets ignored until the version that removes the
/// thing.
fn deprecation(rel: &Path, spec: &SiteSpec) -> String {
    format!(
        "{}: `{SITES_KEY}` is deprecated and stops working in plates 0.3 — site `{name}` is \
         `exports.{name}` with `gate: {{field: {field}, value: {audience}}}`, and its front \
         page, shell, stylesheet, lang and syntaxes belong on the `{audience}` term node of \
         the `{field}` field's vocabulary (`front_page:` and a `site:` mapping)",
        rel.display(),
        name = spec.name,
        field = spec.gate_field(),
        audience = spec.audience,
    )
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
            // Not a key of this dialect, and not becoming one: `sites:` is
            // frozen at what it read the day it was deprecated, so a vault that
            // wants a hold gets it by migrating to `exports:` — which is the
            // move every deprecation warning here already asks for.
            hold: None,
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

/// A site per `exports:` entry — every entry, whatever it gates on.
///
/// The render-facing half is left empty here on purpose: it is written on the
/// gate value's term node, and reading one needs a workspace with an id index,
/// which this pass does not have and the build does ([`plates::read_term_config`],
/// folded in by [`crate::build::build_sites`]).
fn specs_from_exports(exports: &[ExportSpec]) -> Vec<SiteSpec> {
    exports
        .iter()
        .map(|export| SiteSpec {
            name: export.name.clone(),
            label: export.label.clone(),
            audience: export.gate.value.clone(),
            // `None` for the default field rather than its name spelled out, so
            // that a spec says what the archive said: `gate_field()` supplies
            // `audience` wherever the question is asked, and a spec carrying the
            // string would be indistinguishable from one whose archive named it.
            gate_field: (export.gate.field != AUDIENCE_FIELD).then(|| export.gate.field.clone()),
            hold: export.hold.clone(),
            view: export.view.clone(),
            index: None,
            shell: None,
            stylesheet: None,
            lang: None,
            syntaxes: Vec::new(),
        })
        .collect()
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
            hold: None,
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
                hold: None,
                view: Some("daily".into()),
                index: Some("[Home](id:7f3a91c)".into()),
                shell: Some(".config/sites/blog/shell.html".into()),
                stylesheet: Some(".config/sites/blog/style.css".into()),
                lang: Some("fr".into()),
                syntaxes: vec![".config/sites/blog/wat.sublime-syntax".into()],
            }]
        );
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

    /// An export is already a named, gated set that may leave the archive, so
    /// every export is a site — including one gated on another field. That is
    /// not a special case being tolerated: read from `exports:`, `gate.field:
    /// clearance` *is* the gate, and it travels with the spec.
    #[test]
    fn every_export_becomes_a_site_carrying_its_own_gate() {
        let specs = specs_from_exports(&[
            export("blog", AUDIENCE_FIELD, "public"),
            export("audit", "clearance", "internal"),
        ]);

        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].audience, "public");
        assert_eq!(
            specs[0].gate_field, None,
            "the default field is left unsaid, so the spec says what the archive said"
        );
        assert_eq!(specs[1].audience, "internal");
        assert_eq!(specs[1].gate_field.as_deref(), Some("clearance"));
        assert_eq!(specs[1].gate_field(), "clearance");
    }

    /// The other half of the export an archive declares. A hold is a bound the
    /// author wrote on what leaves, so dropping it on the way to a site would
    /// publish drafts a `prov exports` preview says are being held — the two
    /// tools disagreeing about one archive's own declaration.
    #[test]
    fn an_exports_hold_reaches_the_site() {
        let held = ExportSpec {
            hold: Some("draft".into()),
            ..export("blog", AUDIENCE_FIELD, "public")
        };
        let specs = specs_from_exports(&[export("audit", AUDIENCE_FIELD, "public"), held]);

        assert_eq!(specs[0].hold, None);
        assert_eq!(specs[1].hold.as_deref(), Some("draft"));
    }

    /// A deprecation nobody can act on is one that gets ignored until the
    /// release that removes the thing, so the notice names both halves of the
    /// replacement and the version.
    #[test]
    fn a_declared_site_is_told_what_replaces_it() {
        let mut warnings = Vec::new();
        let block = mapping(&[(
            "blog",
            Value::Mapping(mapping(&[
                ("audience", s("public")),
                ("gate_field", s("clearance")),
            ])),
        )]);

        let specs = specs_from(&block, &mut warnings);
        let notice = deprecation(Path::new("prov.yaml"), &specs[0]);

        assert!(notice.contains("prov.yaml"), "{notice}");
        assert!(notice.contains("exports.blog"), "{notice}");
        assert!(notice.contains("field: clearance"), "{notice}");
        assert!(notice.contains("value: public"), "{notice}");
        assert!(notice.contains("term node"), "{notice}");
        assert!(notice.contains("0.3"), "{notice}");
    }
}
