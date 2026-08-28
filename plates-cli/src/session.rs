//! Finding the archive and opening it — the part of a CLI that is plumbing.
//!
//! Everything here is prov's own discovery, phrased as diagnostics. The walk up
//! from the current directory, the root-candidate rule and the tie-breaking all
//! live in [`plates::prov::discover`]; what is added is the site declarations
//! ([`crate::config`]) and the id registry, both of which a render needs and
//! neither of which discovery hands back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use plates::SiteSpec;
use plates::prov::{
    Discovery, FileIndex, IdStorage, IndexStore, NoIdentity, Settings, StdFs, Workspace,
    WorkspaceConfig, block_on,
};

use crate::config::{self, Source};

/// The workspace a render reads, with its id index already loaded.
///
/// Concretely typed rather than generic: identity is [`NoIdentity`] because
/// nothing in this binary writes, and the index is a [`FileIndex`] because a
/// document's registered id travels into the site's permalinks and a render
/// without one would publish pages that cannot be pointed at.
pub type Archive = Workspace<StdFs, NoIdentity, FileIndex>;

/// One invocation's view of the archive: where it is, what it declares, and
/// what could not be read on the way.
pub struct Session {
    /// Absolute path of the workspace root directory.
    pub root_dir: PathBuf,
    /// The root document, relative to [`root_dir`](Self::root_dir).
    pub root_doc: PathBuf,
    /// The effective prov config — the views a site's arrangement is resolved
    /// against, and the settings the workspace is opened with.
    pub config: WorkspaceConfig,
    /// The sites this archive publishes, in declaration order.
    pub sites: Vec<SiteSpec>,
    /// Where those came from, for a message that has to say what to edit.
    pub source: Source,
    /// What went wrong reading the declaration, in the words of whoever has to
    /// fix it. Never fatal — see [`crate::config::Sites`].
    pub warnings: Vec<String>,
    /// The registry document the root declares, relative to the root.
    registry: Option<PathBuf>,
}

impl Session {
    /// Discover the archive containing the current directory and read what it
    /// declares.
    pub fn open() -> Result<Self, String> {
        let cwd =
            std::env::current_dir().map_err(|e| format!("cannot read this directory: {e}"))?;
        Self::open_at(&cwd)
    }

    /// [`open`](Self::open), but discovering from `dir` — for a re-read after
    /// the config on disk has changed under a watcher, where the root is
    /// already known.
    pub fn open_at(dir: &Path) -> Result<Self, String> {
        let found = match block_on(plates::prov::discover(&StdFs, dir))
            .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        {
            Discovery::Found(found) => found,
            Discovery::Ambiguous { dir, candidates } => {
                return Err(format!(
                    "ambiguous archive root in {}: {} (rename one, or add part_of)",
                    dir.display(),
                    candidates.join(", ")
                ));
            }
            Discovery::NotFound => {
                return Err(
                    "no prov archive found: no ancestor directory has a document with \
                     metadata and no part_of\n  (run `prov init` to start one)"
                        .to_string(),
                );
            }
        };

        // The config document is asked for through a probe workspace rather
        // than the real one, because building the real one needs the registry,
        // and where the registry lives is one of the things a config document
        // is allowed to say.
        let probe: Workspace<StdFs> = Workspace::builder(StdFs).root(&found.root_dir).build();
        let config_doc = block_on(probe.config_path(&found.root_doc)).ok().flatten();

        let sites = config::read_sites(
            &found.root_dir,
            &found.root_doc,
            config_doc.as_deref(),
            &found.config.exports,
        );

        Ok(Self {
            root_dir: found.root_dir,
            root_doc: found.root_doc,
            config: found.config,
            sites: sites.specs,
            source: sites.source,
            warnings: sites.warnings,
            registry: found.registry,
        })
    }

    /// Open the workspace this session's renders read.
    ///
    /// Re-opened per build rather than held: a rebuild has to see files that
    /// did not exist when the process started, including a changed config
    /// document, which is where views and sites are declared.
    pub fn workspace(&self) -> Result<Archive, String> {
        Ok(Workspace::builder(StdFs)
            .root(&self.root_dir)
            // Every policy knob at once, from the config: the relation
            // vocabulary, the reference style, the embedding pair, and what
            // this archive calls itself. A knob added to prov's config reaches
            // the render without touching this function.
            .settings(Settings::from(&self.config))
            .index(self.index()?)
            .build())
    }

    /// The id→path map a collection stamps into each document's frontmatter.
    ///
    /// Built once per build and shared across every site in it, which is the
    /// shape [`plates::CollectOptions::id_by_path`] asks for.
    pub fn id_by_path(&self, ws: &Archive) -> HashMap<PathBuf, String> {
        ws.index()
            .iter()
            .map(|(id, path)| (path.clone(), id.as_str().to_string()))
            .collect()
    }

    /// Load the id index the same way prov's own CLI does: from the registry
    /// document when there is one, and by scanning each document's `id` field
    /// when identity is stored in frontmatter alone.
    fn index(&self) -> Result<FileIndex, String> {
        if self.config.id_storage == IdStorage::FrontmatterOnly {
            let probe: Workspace<StdFs> = Workspace::builder(StdFs).root(&self.root_dir).build();
            let mut index = FileIndex::new(self.config.default_embed_format);
            let ids = block_on(probe.scan_ids())
                .map_err(|e| format!("cannot scan this archive's ids: {e}"))?;
            for (id, path) in ids {
                index.register(&id, &path);
            }
            index.mark_clean();
            return Ok(index);
        }

        let Some(rel) = &self.registry else {
            // No registry declared: an empty in-memory one. Documents fall back
            // to their own frontmatter `id`, which is where the authoritative
            // one lives under `id_storage: both`.
            return Ok(FileIndex::new(self.config.default_embed_format));
        };
        let text = match std::fs::read_to_string(self.root_dir.join(rel)) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("cannot read {}: {e}", rel.display())),
        };
        FileIndex::parse(rel, &text).map_err(|e| format!("cannot read {}: {e}", rel.display()))
    }

    /// What to say when the archive has nothing to publish — the same sentence
    /// from every verb, since the fix is the same one.
    pub fn nothing_to_publish(&self) -> String {
        match self.source {
            Source::None => format!(
                "this archive declares no exports, so there is nothing to render\n  \
                 (declare one under `exports:` in {}: a name, a `label`, and a `gate` \
                 naming the field and value that admit a document)",
                self.root_doc.display()
            ),
            _ => "this archive declares no sites to render".to_string(),
        }
    }
}
