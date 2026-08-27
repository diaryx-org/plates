//! The links a site publishes that lead nowhere — told apart from the links
//! that lead somewhere a reader may not go.
//!
//! A rendered page demotes any link whose target this site does not publish to a
//! [`span.unpublished-link`](plates_render::transform_links): the text stays, the
//! anchor goes, and nothing 404s. That is the right *output* for both of the
//! cases it covers and the wrong *report* for either of them, because they are
//! not one thing. A link to a page held back by the gate is the gate doing its
//! job, and the author has nothing to fix. A link to a page that was renamed,
//! moved or never existed is a mistake, and it looks identical from inside the
//! render — which is how a typo survives a hundred builds.
//!
//! prov's [census](prov::Workspace::census) is what separates them. It resolves
//! every link in the archive against the archive itself rather than against one
//! site's set, so `Broken` means *nothing is there* and
//! [`Resolution::Path`](prov::Resolution) means *something is there, whatever
//! this site chose to publish*. Only the first kind reaches this module.
//!
//! Everything here is a **report**. Nothing is an error, nothing changes what a
//! page renders as, and a diagnostic never puts a document into or out of a
//! site's set — the same rule [`case_drift`](crate::case_drift) is written
//! under, for the same reason: the answer to a broken link is to fix the link.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use prov::{CensusEntry, Id, LinkSite, Resolution};

/// One link a site's own page writes that a reader could not follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkDiagnostic {
    /// The document the link is written in, workspace-relative. Always one of
    /// the site's published pages — a broken link in a document nobody can read
    /// is not this site's problem to report.
    pub source: PathBuf,
    /// Where in [`source`](Self::source) it is written: a frontmatter relation
    /// by name, or the body at a byte span.
    pub site: LinkSite,
    /// The target exactly as the author wrote it, so the message names the text
    /// to search for rather than a path resolution invented.
    pub target: String,
    /// The link's display label, when it carried one — `[label](target)`.
    pub label: Option<String>,
    /// Why it leads nowhere.
    pub problem: LinkProblem,
}

/// Why a link leads nowhere.
///
/// The five resolutions a reader cannot follow, and no others: prov's
/// [`Resolution`] also describes links that resolve perfectly well
/// ([`Path`](Resolution::Path), [`Id`](Resolution::Id)), links off the archive
/// entirely ([`External`](Resolution::External),
/// [`Foreign`](Resolution::Foreign)) and a locator into the citing document
/// ([`SameDocument`](Resolution::SameDocument)). A closed enum of the five is
/// what keeps a caller from having to decide which of prov's cases are faults —
/// deciding that once, here, is the whole point of the module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkProblem {
    /// A path with nothing on disk at it: a rename, a move, or a typo.
    Broken,
    /// A path that matches a file only case-insensitively.
    ///
    /// Not broken *here* — this build resolved it — and broken on the next
    /// case-sensitive filesystem the archive is read on, which is every server
    /// the site will ever be served from.
    CaseMismatch {
        /// The path as the link resolved to it.
        got: PathBuf,
        /// The file's exact name on disk.
        actual: String,
    },
    /// A well-formed id the registry does not hand to a live document.
    DanglingId {
        /// The id as written.
        id: Id,
        /// Whether the registry retired it (the document was deleted) rather
        /// than never having issued it (a link from another archive).
        tombstoned: bool,
    },
    /// An id that fails its check character — a typo prov can prove is one.
    MalformedId,
    /// A `[[name]]` several documents answer to, so it resolves to none of
    /// them. The fix is in the link, not in the documents: name one.
    AmbiguousAlias {
        /// The documents sharing the name, sorted.
        candidates: Vec<PathBuf>,
    },
}

impl LinkProblem {
    /// The problem a resolution *is*, or `None` when a reader can follow it.
    ///
    /// A [`Resolution::Path`] that this site does not publish is deliberately
    /// `None`. The gate held that document back on purpose, the render already
    /// says so in the page, and calling it a fault would train an author to
    /// ignore the report that also carries the real ones.
    pub fn of(resolution: &Resolution) -> Option<Self> {
        match resolution {
            Resolution::Broken => Some(Self::Broken),
            Resolution::CaseMismatch { got, actual } => Some(Self::CaseMismatch {
                got: got.clone(),
                actual: actual.clone(),
            }),
            Resolution::DanglingId { id, tombstoned } => Some(Self::DanglingId {
                id: id.clone(),
                tombstoned: *tombstoned,
            }),
            Resolution::MalformedId => Some(Self::MalformedId),
            Resolution::AmbiguousAlias { candidates, .. } => Some(Self::AmbiguousAlias {
                candidates: candidates.clone(),
            }),
            Resolution::Path(_)
            | Resolution::Id { .. }
            | Resolution::External
            | Resolution::SameDocument
            | Resolution::Foreign { .. } => None,
        }
    }
}

/// The links a site's pages write that lead nowhere.
///
/// `census` is [`prov::Workspace::census`]'s, taken over the whole archive:
/// there is one census per build, not one per site, because the resolutions are
/// the archive's and do not change with who is reading them. `published` is the
/// set of documents this site serves, which is all that narrows the report — a
/// broken link in a document the site does not publish is somebody else's build
/// to fix.
pub fn link_diagnostics<'a>(
    census: &[CensusEntry],
    published: impl IntoIterator<Item = &'a Path>,
) -> Vec<LinkDiagnostic> {
    let published: HashSet<&Path> = published.into_iter().collect();
    census
        .iter()
        .filter(|entry| published.contains(entry.source.as_path()))
        .filter_map(|entry| {
            Some(LinkDiagnostic {
                source: entry.source.clone(),
                site: entry.site.clone(),
                target: entry.target_text.clone(),
                label: entry.label.clone(),
                problem: LinkProblem::of(&entry.resolution)?,
            })
        })
        .collect()
}

impl fmt::Display for LinkDiagnostic {
    /// One sentence naming the document, the link as written and what is wrong
    /// with it — everything needed to find it in an editor, since a byte span
    /// is not what a person searches by.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} links to {:?}", self.source.display(), self.target)?;
        if let Some(label) = &self.label {
            write!(f, " ({label:?})")?;
        }
        match &self.site {
            LinkSite::Relation(name) => write!(f, " in {name}:")?,
            LinkSite::Body(_) => f.write_str(" in its body")?,
        }
        write!(f, " — {}", self.problem)
    }
}

impl fmt::Display for LinkProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Broken => f.write_str("nothing is on disk there"),
            Self::CaseMismatch { actual, .. } => {
                write!(
                    f,
                    "the file is on disk as {actual:?}, so the link works here and \
                     breaks wherever case matters"
                )
            }
            Self::DanglingId {
                id,
                tombstoned: true,
            } => write!(f, "id {id} belonged to a document that was deleted"),
            Self::DanglingId {
                id,
                tombstoned: false,
            } => write!(f, "id {id} was never issued in this archive"),
            Self::MalformedId => f.write_str("that id fails its check character, so it is a typo"),
            Self::AmbiguousAlias { candidates } => write!(
                f,
                "{} documents answer to that name ({}), so it names none of them",
                candidates.len(),
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(source: &str, target: &str, resolution: Resolution) -> CensusEntry {
        CensusEntry {
            source: PathBuf::from(source),
            site: LinkSite::Body(0..target.len()),
            target_text: target.to_string(),
            label: None,
            resolution,
        }
    }

    /// The distinction the module exists for. Both links render as the same
    /// unpublished span; only one of them is a mistake.
    #[test]
    fn a_broken_link_is_a_diagnostic_and_an_ungated_one_is_not() {
        let census = vec![
            entry("trip.md", "gone.md", Resolution::Broken),
            entry(
                "trip.md",
                "private.md",
                Resolution::Path(PathBuf::from("private.md")),
            ),
        ];
        let found = link_diagnostics(&census, [Path::new("trip.md")]);
        assert_eq!(found.len(), 1, "got {found:?}");
        assert_eq!(found[0].target, "gone.md");
        assert_eq!(found[0].problem, LinkProblem::Broken);
    }

    /// A link that resolves only case-insensitively resolves *here*, and on the
    /// case-sensitive filesystem the site is served from it does not — so it is
    /// reported with both names, the one written and the one on disk.
    #[test]
    fn a_case_mismatch_names_what_was_written_and_what_is_there() {
        let census = vec![entry(
            "trip.md",
            "About.md",
            Resolution::CaseMismatch {
                got: PathBuf::from("About.md"),
                actual: "about.md".to_string(),
            },
        )];
        let found = link_diagnostics(&census, [Path::new("trip.md")]);
        assert_eq!(
            found[0].problem,
            LinkProblem::CaseMismatch {
                got: PathBuf::from("About.md"),
                actual: "about.md".to_string(),
            }
        );
        let said = found[0].to_string();
        assert!(said.contains("About.md"), "{said}");
        assert!(said.contains("about.md"), "{said}");
    }

    /// The report is bounded by what the site publishes: a broken link in a
    /// document this site never serves belongs to whoever publishes that
    /// document, and repeating it here would make every site answer for the
    /// whole archive.
    #[test]
    fn a_broken_link_in_an_unpublished_document_is_not_this_sites_report() {
        let census = vec![entry("elsewhere.md", "gone.md", Resolution::Broken)];
        assert!(link_diagnostics(&census, [Path::new("trip.md")]).is_empty());
    }

    /// Everything prov can resolve, and the two it deliberately does not
    /// (external and foreign), leave the report empty. `SameDocument` is why
    /// the prov floor is 0.9.2: before it, `#3` resolved as a path to the
    /// citing document and every locator in the archive would have had to be
    /// recognized here by hand.
    #[test]
    fn a_link_prov_can_follow_or_will_not_judge_is_never_a_diagnostic() {
        let census = vec![
            entry(
                "trip.md",
                "about.md",
                Resolution::Path(PathBuf::from("about.md")),
            ),
            entry(
                "trip.md",
                "prov:abc1234",
                Resolution::Id {
                    id: Id("abc1234".to_string()),
                    to: PathBuf::from("about.md"),
                },
            ),
            entry("trip.md", "https://example.org", Resolution::External),
            entry("trip.md", "#3", Resolution::SameDocument),
            entry(
                "trip.md",
                "id:other/abc1234",
                Resolution::Foreign {
                    workspace: "other".to_string(),
                    id: Id("abc1234".to_string()),
                },
            ),
        ];
        assert!(link_diagnostics(&census, [Path::new("trip.md")]).is_empty());
    }

    /// An id that never resolved reads differently depending on why, and the
    /// difference is the fix: a tombstone means the document was deleted, and
    /// no record at all means the link came from somewhere else.
    #[test]
    fn a_dangling_id_says_whether_the_document_was_deleted() {
        let census = vec![
            entry(
                "trip.md",
                "prov:abc1234",
                Resolution::DanglingId {
                    id: Id("abc1234".to_string()),
                    tombstoned: true,
                },
            ),
            entry(
                "trip.md",
                "prov:xyz9876",
                Resolution::DanglingId {
                    id: Id("xyz9876".to_string()),
                    tombstoned: false,
                },
            ),
        ];
        let found = link_diagnostics(&census, [Path::new("trip.md")]);
        assert!(found[0].to_string().contains("deleted"), "{:?}", found[0]);
        assert!(
            found[1].to_string().contains("never issued"),
            "{:?}",
            found[1]
        );
    }

    /// A relation link names its field, because "in its body" would send the
    /// author looking through prose for something written in the frontmatter.
    #[test]
    fn a_relation_link_is_reported_by_its_field_name() {
        let census = vec![CensusEntry {
            source: PathBuf::from("trip.md"),
            site: LinkSite::Relation("contents".to_string()),
            target_text: "gone.md".to_string(),
            label: Some("the old page".to_string()),
            resolution: Resolution::Broken,
        }];
        let said = link_diagnostics(&census, [Path::new("trip.md")])[0].to_string();
        assert_eq!(
            said,
            "trip.md links to \"gone.md\" (\"the old page\") in contents: — nothing is on disk there"
        );
    }
}
