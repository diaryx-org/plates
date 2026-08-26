//! What a site *is*: the declaration, the plan it composes to, and the front
//! page that fronts it.
//!
//! These describe a site rather than a vault: a caller holding a
//! [`prov::Workspace`] and a [`SiteSpec`] can plan and collect a whole website
//! with no workspace layer above it at all, which is the property this crate is
//! for.
//!
//! What is deliberately *not* here is the half that reads a vault's **config
//! vocabulary** — which block declares a site, where its front page is named,
//! what the compatibility fallbacks are. That is a vault's dialect, not a site's
//! shape, and an application that planned sites for a different vault format
//! would read a different one while composing the same [`SiteSpec`].
//!
//! # Why a site is neither an audience nor a view
//!
//! An audience answers *may this document leave the vault*. A view answers *how
//! what stays is arranged*. Neither derives the other, and a published site
//! needs both.
//!
//! The temptation is to collapse the two — to make an audience "just a filter on
//! a view", since both select documents by the value of a declared field. Three
//! things stop it. A wrong view is a wrong grouping you fix in the picker; a
//! wrong gate is a file in a stranger's hands, which is why a gate field is
//! [`OpenClosed::Closed`](prov::config::OpenClosed) where a user-declared field
//! is open. A view with no `under:` covers the whole vault, while a document
//! with no gate value is visible to no one — open-by-default against
//! closed-by-default, and one primitive cannot hold both. And the gate value is
//! written *in the document*, so it travels with the file into another vault and
//! still means what it meant, where view membership is a property of the vault
//! and cannot be.
//!
//! So a gate is not a *kind* of filter. It is a *position*: the domain every
//! view runs over once the corpus leaves the vault.
//!
//! # The one-way valve
//!
//! > **A site's document set is a subset of its gate's admitted set.** A view
//! > may narrow that set and order it. A view may never admit a document the
//! > gate held out.
//!
//! [`prov::exports::compose`] enforces this structurally rather than by
//! convention: it seeds from what the gate admits and the only operation it
//! applies is `retain`. Nothing in this crate can widen it.
//!
//! # Why a site is named separately from its gate
//!
//! A site's name is its path segment in every published URL — public surface
//! that outlives any one page. A gate value is private vocabulary, chosen to be
//! precise about *who*, and the honest name for a readership is routinely one
//! its members should never read off a URL. Separating them also lets two
//! audiences share an arrangement, and one audience carry two sites.

use std::path::{Path, PathBuf};

use prov::exports::ExportPlan;

use crate::error::{Error, Result};

/// One site a vault declares for itself: an audience to publish to, optionally
/// arranged by a view and fronted by a chosen index.
///
/// The site-facing shape of a [`prov::exports::ExportSpec`] — kept separate
/// because `audience` is the vocabulary the product speaks, and because the
/// index and the render-facing keys have nowhere to live in prov's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteSpec {
    /// The key under `exports` — and the site's path segment in every published
    /// URL. Deliberately not the audience's name; see the module docs.
    pub name: String,
    /// What a person calls it. Absent falls back to the name, humanized.
    pub label: Option<String>,
    /// The audience whose declared set bounds this site — the gate's value.
    /// Required: a site that does not say who it is for is not a site.
    pub audience: String,
    /// The [`ViewSpec`](prov::ViewSpec) naming this site's arrangement, by its
    /// key under `views`. `None` publishes the gate's whole set, arranged by
    /// containment.
    pub view: Option<String>,
    /// The site's front page, as a link (`'[Home](id:abc1234)'`) resolved
    /// through the spanning relation like a view's `under:`, so it survives a
    /// rename, a move and a retitle.
    ///
    /// `None` means the render layer synthesizes an index from the site's
    /// entries — the case that makes per-file audiences work at all, since a
    /// vault whose root is private (the default) has no natural front page to
    /// promote, and promoting whichever entry happens to sort first is how the
    /// site's identity ends up depending on traversal order.
    pub index: Option<String>,
    /// The site's shell template, as a **vault-relative path** to an HTML file.
    ///
    /// The template is the whole outer document, `<!DOCTYPE html>` to
    /// `</html>`, with named slots for the parts a render computes — see
    /// `plates_render::shell`. `None` uses the built-in shell, which is what
    /// every site published before templates existed gets, byte for byte.
    ///
    /// A path rather than the text, because a shell is a file someone edits.
    /// Any vault-relative path is honoured; [`SITE_ASSETS_DIR`] is only the
    /// recommended home.
    pub shell: Option<String>,
    /// The site's stylesheet, as a **vault-relative path** to a CSS file.
    ///
    /// It *replaces* the built-in sheet rather than layering over it
    /// (`plates_render::SiteStyle::custom_css`), so a site that names one owns
    /// its whole appearance. `None` keeps the built-in sheet.
    pub stylesheet: Option<String>,
    /// BCP 47 language tag for every page's `<html lang="…">`. `None` is
    /// `"en"`, which is what the render layer assumes when nobody says.
    pub lang: Option<String>,
}

impl SiteSpec {
    /// What a person calls this site: its label, else its name humanized.
    pub fn display_label(&self) -> String {
        match &self.label {
            Some(label) => label.clone(),
            None => humanize(&self.name),
        }
    }
}

/// The recommended home for a site's shell and stylesheet:
/// `.config/sites/<name>/`.
///
/// A *convention*, not a rule — [`SiteSpec::shell`] and
/// [`SiteSpec::stylesheet`] take any vault-relative path, and nothing here
/// hardcodes this one. It is dot-prefixed so no publish path picks the files up
/// as documents or attachments of their own: a shell is the frame a site is
/// rendered in, not a page in it.
pub const SITE_ASSETS_DIR: &str = ".config/sites";

/// `daily_entries` → `Daily entries`: a key is written for a file, a label for a
/// person.
///
/// prov-views has this too, and `ViewSpec::display_label` uses it — but prov
/// does not re-export the function, and a site that declares no label needs the
/// same fallback.
pub fn humanize(key: &str) -> String {
    let mut words = key.split(['_', '-']).filter(|w| !w.is_empty());
    let Some(first) = words.next() else {
        return key.to_string();
    };
    let mut out = first.to_string();
    if let Some(c) = out.get_mut(0..1) {
        c.make_ascii_uppercase();
    }
    for word in words {
        out.push(' ');
        out.push_str(&word.to_lowercase());
    }
    out
}

/// A document a site publishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisibleDoc {
    /// Workspace-relative path.
    pub path: PathBuf,
    /// The document's `title`, when present.
    pub title: Option<String>,
    /// The audiences the document declares (empty when visible only because the
    /// request was a wildcard and the document declared nothing).
    pub declared: Vec<String>,
}

/// A site whose front page is a directory: what
/// [`SitePlan::index_directory`] carries when the declared index turned out to
/// be a [manifest](prov::manifest) node.
///
/// A manifest node is not a page — it has no body, only a claim over a
/// directory of files — so a site fronted by one is not asking for that node to
/// be rendered. It is saying *this directory is the site's front matter*: an
/// authored `index.html` and the assets it references, published as they were
/// written rather than as the renderer would have written them.
///
/// Everything here is expressed relative to [`root`](Self::root), which becomes
/// the site's own root, because that is the frame the authored page's links
/// were written against: `www/index.html` asking for `logo.png` must find it at
/// the site root, not at `www/logo.png`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDirectory {
    /// The covered directory, workspace-relative (`www`).
    pub root: PathBuf,
    /// The front page, relative to [`root`](Self::root) — always
    /// [`FRONT_PAGE`], confirmed to be on disk before this is constructed.
    ///
    /// Not one of [`files`](Self::files): prov reads HTML, so a manifest never
    /// claims it (`manifests.md` §3). It is the one file in a covered directory
    /// this layer names by convention, which is why it is named here rather
    /// than left for each consumer to go looking for.
    pub front_page: PathBuf,
    /// Every file the manifest covers, relative to [`root`](Self::root), in the
    /// manifest's own order.
    ///
    /// The assets, in other words. A covered file is opaque bytes by
    /// definition, so this set never contains a page and never needs rendering
    /// — it is copied.
    pub files: Vec<PathBuf>,
}

/// The file a covered directory is fronted by, when a site declares its
/// manifest node as an index.
///
/// A convention, and deliberately the web's own rather than one of ours: the
/// directory these nodes exist to describe is a directory someone wrote to be
/// served, and its front page is already called `index.html` in every tool that
/// will ever look at it. A covered directory without one is
/// [`Error::SiteIndexDirectoryHasNoFrontPage`] rather than a synthesized
/// listing, because a site whose front page silently became a file index is a
/// site whose author's landing page vanished with nothing said.
pub const FRONT_PAGE: &str = "index.html";

/// What one site publishes: the documents, the front page, and what the gate and
/// the view each held back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitePlan {
    /// The site's name — its public path segment.
    pub site: String,
    /// The audience this site's set was bounded by.
    pub audience: String,
    /// The documents this site publishes. Guaranteed a subset of what the gate
    /// admits.
    pub entries: Vec<VisibleDoc>,
    /// The declared front page, confirmed to be admitted by the gate. `None`
    /// when the site declares none and the render layer should synthesize one.
    ///
    /// **Not necessarily one of [`entries`](Self::entries).** A site's index is
    /// usually its view's anchor, and prov's view scope is the subtree *below*
    /// the anchor — the entries under `Daily`, not `Daily` itself. So the front
    /// page is the site's frame rather than an item in its own list, and the
    /// collection layer ships it alongside the entries instead of finding it
    /// among them.
    ///
    /// What is still guaranteed is the part that matters: the gate admitted it.
    /// A front page the gate holds back is [`Error::SiteIndexNotVisible`], not a
    /// page quietly promoted past the disclosure rule.
    pub index: Option<PathBuf>,
    /// Set when [`index`](Self::index) is a manifest node: the directory it
    /// covers, which *is* the site's front matter. See [`IndexDirectory`].
    ///
    /// `None` for the ordinary case — a front page that is a document, rendered
    /// like every other. A consumer that ignores this field publishes a site
    /// whose front page is an empty rendering of a body-less node, which is why
    /// the render and collection layers both branch on it.
    pub index_directory: Option<IndexDirectory>,
    /// Documents the gate admits that this site's view scoped out. Not an
    /// error — it is the difference between a site and its audience, and a
    /// publish preview owes the user a count of it, since "I tagged it `family`
    /// and it isn't on the site" is otherwise unexplainable from the file alone.
    pub outside_view: Vec<PathBuf>,
    /// Documents held back whose declared audience matches the gate **except
    /// for case or surrounding space** — `audience: Public` against a `public`
    /// gate.
    ///
    /// Empty for every vault that never drifted. Non-empty means the site is
    /// publishing less than its author believes, which is the quiet failure the
    /// move to prov's exact-match gate could otherwise introduce; a caller is
    /// expected to say so rather than let the count speak for itself.
    pub case_drift: Vec<PathBuf>,
}

/// Finish a prov export plan into a site plan: attach the front page, and name
/// the documents the exact-match gate held back that a case-insensitive rule
/// would have let through.
///
/// `index` is `spec.index` already resolved to a path by the caller, and
/// `index_directory` is what that path turned out to cover when it was a
/// manifest node; this function's job is to confirm the index is a member. The
/// set arithmetic that matters — the one-way valve — already happened in
/// [`prov::exports::compose`].
///
/// The gate check is made against the *node*, not the directory, and that is the
/// right place for it: the node is the document carrying the audience its
/// author declared, and the covered files have no metadata of their own to
/// judge. Covering a directory does not disclose it; linking its node into a
/// site does, which is the decision a gate is for.
pub fn finish(
    spec: &SiteSpec,
    export: ExportPlan,
    index: Option<&Path>,
    index_directory: Option<IndexDirectory>,
) -> Result<SitePlan> {
    let entries: Vec<VisibleDoc> = export
        .entries
        .into_iter()
        .map(|doc| VisibleDoc {
            path: doc.path,
            title: doc.title,
            declared: doc.declared,
        })
        .collect();

    // A front page the *gate* holds back is a configuration error, not a cue to
    // quietly synthesize one: the site would publish with its intended front
    // page missing and no sign that anything was wrong. The usual cause is an
    // index document nobody remembered to tag with the site's audience.
    //
    // Membership is checked against what the gate admitted — `entries` plus what
    // the view scoped out — rather than against `entries` alone. An index is
    // typically the view's anchor, and prov scopes a view to the subtree *below*
    // its anchor, so the ordinary healthy site has its front page in
    // `outside_view`. Checking `entries` would reject exactly the arrangement
    // this feature exists to serve, while checking the gate still refuses the
    // one that matters: a page nobody may see becoming the front door.
    let index = match index {
        Some(path) => {
            let admitted = entries.iter().any(|doc| doc.path.as_path() == path)
                || export.outside_view.iter().any(|p| p.as_path() == path);
            if !admitted {
                return Err(Error::SiteIndexNotVisible {
                    site: spec.name.clone(),
                    audience: spec.audience.clone(),
                    path: path.to_path_buf(),
                });
            }
            Some(path.to_path_buf())
        }
        None => None,
    };

    Ok(SitePlan {
        site: spec.name.clone(),
        audience: spec.audience.clone(),
        entries,
        index,
        index_directory,
        outside_view: export.outside_view,
        case_drift: case_drift(&spec.audience, &export.withheld),
    })
}

/// Documents a gate held back that differ from its value only in case or
/// surrounding space.
///
/// The whole migration risk in one function. prov's gate is exact after
/// trimming, and it is right to be: a gate that forgives casing is a gate that
/// forgives a typo, and the typo it forgives is the one that publishes a
/// document. But a vault written under an older case-insensitive rule may hold
/// `audience: Public` in perfectly good faith, and flipping the rule takes those
/// documents off the site with nothing said. This is what gets said.
///
/// Deliberately a *report*, never a fallback. Nothing here puts a document back
/// in the set — the answer to drift is to fix the document, which is also what
/// a closed gate vocabulary is for.
pub fn case_drift(gate_value: &str, withheld: &[prov::exports::Withheld]) -> Vec<PathBuf> {
    let wanted = gate_value.trim();
    withheld
        .iter()
        .filter(|doc| {
            doc.declared.as_ref().is_some_and(|declared| {
                declared.iter().any(|value| {
                    let value = value.trim();
                    value != wanted && value.eq_ignore_ascii_case(wanted)
                })
            })
        })
        .map(|doc| doc.path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prov::exports::{ExportDoc, ExportSpec, Gate, Withheld};

    fn export_spec(name: &str, audience: &str) -> ExportSpec {
        ExportSpec {
            name: name.to_string(),
            label: None,
            gate: Gate {
                field: crate::plan::AUDIENCE_FIELD.to_string(),
                value: audience.to_string(),
            },
            view: None,
        }
    }

    fn site(name: &str, audience: &str) -> SiteSpec {
        let export = export_spec(name, audience);
        SiteSpec {
            name: export.name,
            label: export.label,
            audience: export.gate.value,
            view: export.view,
            index: None,
            shell: None,
            stylesheet: None,
            lang: None,
        }
    }

    fn entry(path: &str) -> ExportDoc {
        ExportDoc {
            path: PathBuf::from(path),
            title: None,
            declared: vec!["family".into()],
        }
    }

    fn plan_of(entries: Vec<ExportDoc>, withheld: Vec<Withheld>) -> ExportPlan {
        ExportPlan {
            export: "letters".into(),
            entries,
            outside_view: Vec::new(),
            withheld,
        }
    }

    fn plan_scoping_out(entries: Vec<ExportDoc>, outside_view: Vec<&str>) -> ExportPlan {
        ExportPlan {
            export: "letters".into(),
            entries,
            outside_view: outside_view.into_iter().map(PathBuf::from).collect(),
            withheld: Vec::new(),
        }
    }

    #[test]
    fn a_declared_index_may_be_one_of_the_entries() {
        let plan = finish(
            &site("letters", "family"),
            plan_of(vec![entry("index.md"), entry("trip.md")], Vec::new()),
            Some(Path::new("index.md")),
            None,
        )
        .expect("a plan");
        assert_eq!(plan.index, Some(PathBuf::from("index.md")));
    }

    /// …and ordinarily is not. A site's front page is usually its view's
    /// anchor, and prov scopes a view to the subtree *below* the anchor, so the
    /// healthy arrangement has the index in `outside_view`. The front page is
    /// the site's frame, not an item in its own list.
    #[test]
    fn a_front_page_the_view_scoped_out_is_still_the_front_page() {
        let plan = finish(
            &site("letters", "family"),
            plan_scoping_out(vec![entry("daily/monday.md")], vec!["daily.md"]),
            Some(Path::new("daily.md")),
            None,
        )
        .expect("a plan");
        assert_eq!(plan.index, Some(PathBuf::from("daily.md")));
        assert!(
            !plan.entries.iter().any(|d| d.path == Path::new("daily.md")),
            "carried alongside the entries, not folded into them"
        );
    }

    /// An index the *gate* holds back is a configuration error — usually a front
    /// page nobody remembered to tag. Synthesizing one instead would publish a
    /// site missing the page its author chose, and look fine doing it; accepting
    /// it would promote a page nobody may see to the site's front door.
    #[test]
    fn an_index_the_gate_holds_back_is_an_error() {
        let err = finish(
            &site("letters", "family"),
            plan_of(vec![entry("trip.md")], Vec::new()),
            Some(Path::new("index.md")),
            None,
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::SiteIndexNotVisible { ref path, .. } if path == Path::new("index.md")),
            "got {err:?}"
        );
    }

    /// The migration's one sharp edge, reported rather than absorbed. Under the
    /// old case-insensitive rule `Family` published; under prov's gate it does
    /// not, and the author is told which file to fix.
    #[test]
    fn casing_drift_is_named_not_forgiven() {
        let plan = finish(
            &site("letters", "family"),
            plan_of(
                vec![entry("trip.md")],
                vec![
                    Withheld {
                        path: PathBuf::from("letter.md"),
                        title: None,
                        declared: Some(vec!["Family".into()]),
                    },
                    Withheld {
                        path: PathBuf::from("spaced.md"),
                        title: None,
                        declared: Some(vec![" family ".into()]),
                    },
                    Withheld {
                        path: PathBuf::from("other.md"),
                        title: None,
                        declared: Some(vec!["internal".into()]),
                    },
                    Withheld {
                        path: PathBuf::from("private.md"),
                        title: None,
                        declared: None,
                    },
                ],
            ),
            None,
            None,
        )
        .expect("a plan");

        assert_eq!(
            plan.case_drift,
            [PathBuf::from("letter.md")],
            "only the one that differs by case — a value prov already trims is not drift, \
             and a document declaring something else, or nothing, is ordinary withholding"
        );
        assert!(
            !plan
                .entries
                .iter()
                .any(|d| d.path == Path::new("letter.md")),
            "and naming it must never put it back in the set"
        );
    }
}
