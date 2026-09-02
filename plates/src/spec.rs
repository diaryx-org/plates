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

use prov::CensusEntry;
use prov::exports::ExportPlan;

use crate::error::{Error, Result};
use crate::links::{LinkDiagnostic, link_diagnostics};

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
    /// The document field the gate reads [`audience`](Self::audience) out of.
    /// `None` is [`AUDIENCE_FIELD`](crate::AUDIENCE_FIELD).
    ///
    /// A vault whose disclosure control is spelled something else — `clearance`,
    /// `visibility`, a term in another language — says so here rather than being
    /// unpublishable. It stays a per-site setting because prov's gate is
    /// per-export: two sites over one vault may legitimately judge different
    /// fields, and a workspace-wide setting could not express that.
    ///
    /// Changing it does not loosen anything. The gate is still exact after
    /// trimming and still closed by default, so a document that declares nothing
    /// under the named field is visible to nobody, exactly as before.
    pub gate_field: Option<String>,
    /// The document field this site reads a *not yet* out of — prov's
    /// [`hold`](prov::exports::ExportSpec::hold). A document the gate admits
    /// that declares `true` under this field is held: off the site, and named
    /// in [`SitePlan::held`] rather than lost among the documents the gate
    /// refused.
    ///
    /// `None` is a site with no hold, which is every site declared before the
    /// key existed: the gate alone decides, exactly as before.
    ///
    /// Per-site for the reason [`gate_field`](Self::gate_field) is: a hold is
    /// per-export in prov, and two sites over one vault may legitimately hold
    /// on different words — a vault whose drafts are `draft:` to its readers
    /// and `embargoed:` to its editors cannot say that once.
    ///
    /// It only ever narrows. A held document is one the gate already admitted,
    /// so naming a field here can take a page off the site and can never put
    /// one on it.
    pub hold: Option<String>,
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
    /// Extra grammars for highlighting code, as **vault-relative paths** to
    /// `.sublime-syntax` files.
    ///
    /// The render layer already carries 213 of them, so this is for a language
    /// no public grammar covers: an in-house DSL, a config dialect, a notation
    /// the vault invented. The grammar's own `file_extensions:` decides which
    /// fence tags it answers to — a definition listing `wat` is what makes
    /// ```` ```wat ```` colour.
    ///
    /// Paths rather than the texts, for the reason [`shell`](Self::shell) is a
    /// path: a grammar is a file someone edits. In declaration order, so that
    /// where two grammars claim one extension the later wins.
    /// [`SITE_ASSETS_DIR`] is the recommended home, not a rule.
    pub syntaxes: Vec<String>,
}

impl SiteSpec {
    /// What a person calls this site: its label, else its name humanized.
    pub fn display_label(&self) -> String {
        match &self.label {
            Some(label) => label.clone(),
            None => humanize(&self.name),
        }
    }

    /// The field this site's gate is judged on, defaulted.
    ///
    /// Every reader of [`gate_field`](Self::gate_field) goes through here, so
    /// the default lives in one place and a site that says nothing is judged on
    /// [`AUDIENCE_FIELD`](crate::AUDIENCE_FIELD) wherever the question is asked.
    pub fn gate_field(&self) -> &str {
        self.gate_field
            .as_deref()
            .unwrap_or(crate::plan::AUDIENCE_FIELD)
    }
}

/// The recommended home for a site's shell and stylesheet:
/// `.config/sites/<name>/`.
///
/// A *convention*, not a rule — [`SiteSpec::shell`], [`SiteSpec::stylesheet`]
/// and [`SiteSpec::syntaxes`] take any vault-relative path, and nothing here
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
    /// Documents the gate admits that this site's own hold kept back — the
    /// drafts. Empty for a site that names no [`hold`](SiteSpec::hold).
    ///
    /// Carried as [`VisibleDoc`]s, declared values and all, because these are
    /// the documents that *would* publish: the site's pending set, not
    /// strangers to it. A caller previewing a publish is expected to show them
    /// as such — they are the one group here whose absence has a date on it.
    ///
    /// Disjoint from [`outside_view`](Self::outside_view): prov applies the
    /// hold before the view, so a draft the view would also have scoped out is
    /// reported once, here.
    pub held: Vec<VisibleDoc>,
    /// Documents the gate admits that this site's view scoped out. Not an
    /// error — it is the difference between a site and its audience, and a
    /// publish preview owes the user a count of it, since "I tagged it `family`
    /// and it isn't on the site" is otherwise unexplainable from the file alone.
    pub outside_view: Vec<PathBuf>,
    /// Documents held back whose declared value matches the gate **except for
    /// case or surrounding space** — `audience: Public` against a `public`
    /// gate, in whichever field [`SiteSpec::gate_field`] named.
    ///
    /// Empty for every vault that never drifted. Non-empty means the site is
    /// publishing less than its author believes, which is the quiet failure the
    /// move to prov's exact-match gate could otherwise introduce; a caller is
    /// expected to say so rather than let the count speak for itself.
    pub case_drift: Vec<PathBuf>,
    /// Links this site's own pages write that lead nowhere — a renamed file, a
    /// retired id, a name two documents answer to. See [`crate::links`] for why
    /// a link to a document the *gate* held back is not one of these.
    ///
    /// Empty when the caller planned without a census, which is what a caller
    /// that does not want the report passes.
    pub link_diagnostics: Vec<LinkDiagnostic>,
}

/// Finish a prov export plan into a site plan: attach the front page, carry
/// through the documents the site's hold kept back, name the documents the
/// exact-match gate held back that a case-insensitive rule would have let
/// through, and name the links its pages write that lead nowhere.
///
/// `index` is `spec.index` already resolved to a path by the caller, and
/// `index_directory` is what that path turned out to cover when it was a
/// manifest node; this function's job is to confirm the index is a member. The
/// set arithmetic that matters — the one-way valve — already happened in
/// [`prov::exports::compose`].
///
/// `census` is the archive's, taken once for a whole run rather than per site —
/// the resolutions in it belong to the archive and do not change with who is
/// reading them. `&[]` is a caller that does not want the link report.
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
    census: &[CensusEntry],
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
    let held: Vec<VisibleDoc> = export
        .held
        .into_iter()
        .map(|doc| VisibleDoc {
            path: doc.path,
            title: doc.title,
            declared: doc.declared,
        })
        .collect();

    // A front page the site's own *hold* keeps back is its own error, told
    // apart from the gate's below because the cause and the fix are different:
    // nothing is mistagged and no audience is wrong — the author wrote
    // `draft: true` on the page the site opens with. Checked first because a
    // held document is in neither of the two sets the gate check consults, so
    // falling through would report a draft as a disclosure problem.
    if let (Some(field), Some(path)) = (spec.hold.as_deref(), index)
        && held.iter().any(|doc| doc.path.as_path() == path)
    {
        return Err(Error::SiteIndexHeld {
            site: spec.name.clone(),
            field: field.to_string(),
            path: path.to_path_buf(),
        });
    }

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

    // What the site publishes *as pages*, which is what bounds the link report:
    // the entries, plus the front page when it is a document. A front page that
    // is a covered directory is left out for the reason collection leaves it
    // out — a manifest node is never rendered, so nothing a reader can click on
    // comes from it.
    let pages = entries
        .iter()
        .map(|doc| doc.path.as_path())
        .chain(index.as_deref().filter(|_| index_directory.is_none()));
    let link_diagnostics = link_diagnostics(census, pages);

    Ok(SitePlan {
        site: spec.name.clone(),
        audience: spec.audience.clone(),
        entries,
        index,
        index_directory,
        held,
        outside_view: export.outside_view,
        case_drift: case_drift(&spec.audience, &export.withheld),
        link_diagnostics,
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
///
/// No field name is passed because none is needed: prov fills
/// [`Withheld::declared`](prov::exports::Withheld::declared) from the gate's own
/// field, so this follows whatever [`SiteSpec::gate_field`] named without being
/// told.
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
            hold: None,
            view: None,
        }
    }

    fn site(name: &str, audience: &str) -> SiteSpec {
        let export = export_spec(name, audience);
        SiteSpec {
            name: export.name,
            label: export.label,
            audience: export.gate.value,
            gate_field: None,
            hold: export.hold,
            view: export.view,
            index: None,
            shell: None,
            stylesheet: None,
            lang: None,
            syntaxes: Vec::new(),
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
            held: Vec::new(),
            withheld,
        }
    }

    fn plan_scoping_out(entries: Vec<ExportDoc>, outside_view: Vec<&str>) -> ExportPlan {
        ExportPlan {
            export: "letters".into(),
            entries,
            outside_view: outside_view.into_iter().map(PathBuf::from).collect(),
            held: Vec::new(),
            withheld: Vec::new(),
        }
    }

    fn plan_holding(entries: Vec<ExportDoc>, held: Vec<&str>) -> ExportPlan {
        ExportPlan {
            export: "letters".into(),
            entries,
            outside_view: Vec::new(),
            held: held.into_iter().map(entry).collect(),
            withheld: Vec::new(),
        }
    }

    fn holding(name: &str, audience: &str, field: &str) -> SiteSpec {
        SiteSpec {
            hold: Some(field.to_string()),
            ..site(name, audience)
        }
    }

    /// A site that names no field is judged on `audience`; one that names a
    /// field is judged on that, and the value it compares is untouched. The two
    /// halves of a gate are independent, which is what lets an archive rename
    /// its disclosure control without renaming its readerships.
    #[test]
    fn a_site_gates_on_the_field_it_names_and_audience_otherwise() {
        let mut spec = site("letters", " family ");
        assert_eq!(spec.gate_field(), crate::plan::AUDIENCE_FIELD);
        assert_eq!(crate::plan::to_export(&spec).gate.field, "audience");

        spec.gate_field = Some("clearance".into());
        let export = crate::plan::to_export(&spec);
        assert_eq!(export.gate.field, "clearance");
        assert_eq!(export.gate.value, "family", "still trimmed, still exact");
    }

    #[test]
    fn a_declared_index_may_be_one_of_the_entries() {
        let plan = finish(
            &site("letters", "family"),
            plan_of(vec![entry("index.md"), entry("trip.md")], Vec::new()),
            Some(Path::new("index.md")),
            None,
            &[],
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
            &[],
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
            &[],
        )
        .unwrap_err();
        assert!(
            matches!(err, Error::SiteIndexNotVisible { ref path, .. } if path == Path::new("index.md")),
            "got {err:?}"
        );
    }

    /// A site's hold is prov's, spelled the site's way: the field reaches the
    /// export untouched, and a site that names none exports none — which is
    /// what keeps every site declared before the key existed planning exactly
    /// as it did.
    #[test]
    fn a_sites_hold_is_the_exports_hold() {
        assert_eq!(
            crate::plan::to_export(&site("letters", "family")).hold,
            None
        );
        assert_eq!(
            crate::plan::to_export(&holding("letters", "family", "draft")).hold,
            Some("draft".into())
        );
    }

    /// A held document is the site's pending set, not a stranger to it: it is
    /// off the entries and named with its title and its declared audience, so a
    /// preview can say *this one is coming* rather than leaving the author to
    /// work out why a page they tagged is missing.
    #[test]
    fn held_documents_are_carried_through_and_kept_off_the_entries() {
        let plan = finish(
            &holding("letters", "family", "draft"),
            plan_holding(vec![entry("trip.md")], vec!["half-written.md"]),
            None,
            None,
            &[],
        )
        .expect("a plan");
        assert_eq!(
            plan.entries.iter().map(|d| &d.path).collect::<Vec<_>>(),
            vec![Path::new("trip.md")]
        );
        assert_eq!(
            plan.held,
            vec![VisibleDoc {
                path: PathBuf::from("half-written.md"),
                title: None,
                declared: vec!["family".into()],
            }]
        );
    }

    /// A front page its author called a draft is its own error, not the gate's.
    /// The two are one `SiteIndexNotVisible` away from being told to check the
    /// audience of a page whose audience is perfectly correct, and the fix —
    /// finish the page — is nowhere near the file that message would send them
    /// to.
    #[test]
    fn a_front_page_the_hold_keeps_back_is_its_own_error() {
        let err = finish(
            &holding("letters", "family", "draft"),
            plan_holding(vec![entry("trip.md")], vec!["index.md"]),
            Some(Path::new("index.md")),
            None,
            &[],
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                Error::SiteIndexHeld { ref field, ref path, .. }
                    if field == "draft" && path == Path::new("index.md")
            ),
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
            &[],
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

    /// The link report is bounded by the site, not by the archive: the census
    /// covers every document prov can reach, and a site answers for the pages it
    /// publishes. A broken link in a document the gate held back is a real
    /// mistake in somebody's vault and not this site's to print.
    #[test]
    fn the_link_report_covers_this_sites_pages_and_no_others() {
        let broken = |source: &str| prov::CensusEntry {
            source: PathBuf::from(source),
            site: prov::LinkSite::Body(0..7),
            target_text: "gone.md".to_string(),
            label: None,
            resolution: prov::Resolution::Broken,
        };
        let plan = finish(
            &site("letters", "family"),
            plan_scoping_out(vec![entry("daily/monday.md")], vec!["daily.md"]),
            Some(Path::new("daily.md")),
            None,
            &[
                broken("daily/monday.md"),
                broken("daily.md"),
                broken("private.md"),
            ],
        )
        .expect("a plan");

        let sources: Vec<&Path> = plan
            .link_diagnostics
            .iter()
            .map(|d| d.source.as_path())
            .collect();
        assert_eq!(
            sources,
            [Path::new("daily/monday.md"), Path::new("daily.md")],
            "the entries and the front page — which is a published page even \
             though the view scoped it out of the entries"
        );
    }
}
