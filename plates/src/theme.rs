//! A site's *render-facing* declaration: the shell it is wrapped in, the
//! stylesheet it wears, the grammars it highlights code with, the language it
//! is written in, and how it is arranged — resolved from the vault, and carried
//! to whoever does the rendering.
//!
//! # Why this is a separate pass
//!
//! `plates_render` reads nothing. It is `wasm32-unknown-unknown`-portable, so a
//! shell reaches it as *text* rather than as a path, and a renderer running
//! somewhere with no vault at all — a server rebuilding from uploaded sources —
//! could not open one anyway. So the files a declaration names are read here,
//! once, and travel as their contents.
//!
//! # Reads are best-effort
//!
//! A missing shell is reported and ignored, never fatal. The alternative is a
//! vault that cannot publish because a theme file was renamed, which costs a
//! site its existence to save it some styling. Everything that went wrong is in
//! [`SiteTheme::warnings`], for a caller to say out loud.

use std::collections::{BTreeMap, HashSet};

use plates_render::Arrangement;
use prov::{IdIndex, Storage, ViewSpec, Workspace};

use crate::source::SourceFile;
use crate::spec::SiteSpec;

/// A site's resolved render inputs — everything `plates_render::SiteOptions`
/// needs that the sources themselves do not carry.
#[derive(Debug, Clone)]
pub struct SiteTheme {
    /// What a reader should see the site called: its label, or its name
    /// humanized.
    pub title: String,
    /// The shell template's **text**, read from [`SiteSpec::shell`]. `None`
    /// uses `plates_render`'s built-in shell.
    pub template: Option<String>,
    /// The stylesheet's **text**, read from [`SiteSpec::stylesheet`]. `None`
    /// keeps the built-in sheet.
    pub custom_css: Option<String>,
    /// The shell templates individual **pages** named in their `shell:`
    /// frontmatter, keyed by the vault-relative path they named — the texts,
    /// for the reason [`template`](Self::template) is a text.
    ///
    /// Read from the site's own documents rather than from its declaration
    /// (see [`read_page_shells`]), because that is where the claim is made. A
    /// `BTreeMap` so the manifest's bytes do not depend on the order the
    /// documents were walked in: a map that reordered itself between publishes
    /// would look to the diff like a theme that had changed.
    pub shells: BTreeMap<String, String>,
    /// BCP 47 language tag for `<html lang="…">`.
    pub lang: String,
    /// The **texts** of the `.sublime-syntax` files named by
    /// [`SiteSpec::syntaxes`], paired with the path each was named by — the
    /// texts, for the reason [`template`](Self::template) is a text.
    ///
    /// A `Vec` rather than a map, and in declaration order: where two grammars
    /// claim one file extension the later one wins, so the order a site wrote
    /// them in is a decision it made and not an artifact of the walk.
    /// Unreadable ones are reported into [`warnings`](Self::warnings) and
    /// dropped, exactly as a missing shell is.
    pub syntaxes: Vec<(String, String)>,
    /// How the site is arranged, from the view it declares.
    pub arrangement: Arrangement,
    /// Files the declaration named that could not be read, as messages for
    /// whoever wrote the declaration.
    ///
    /// A missing shell is *reported and ignored*, not fatal. The alternative is
    /// a vault that cannot publish because a theme file was renamed, which
    /// costs a site its existence to save it some styling.
    pub warnings: Vec<String>,
}

/// The language a site is assumed to be in when it does not say — the same
/// default `plates_render` applies, restated here so a manifest always carries
/// an answer.
pub const DEFAULT_LANG: &str = "en";

impl Default for SiteTheme {
    /// An untitled, unthemed site in the default language. Hand-written rather
    /// than derived for one field: a derived `lang` would be the empty string,
    /// which is not "unset" in HTML — it is `<html lang="">`, an explicit claim
    /// that the page is in no language at all.
    fn default() -> Self {
        Self {
            title: String::new(),
            template: None,
            custom_css: None,
            shells: BTreeMap::new(),
            syntaxes: Vec::new(),
            lang: DEFAULT_LANG.to_string(),
            arrangement: Arrangement::default(),
            warnings: Vec::new(),
        }
    }
}

/// Resolve one declared site's theme from the vault: read its shell, its
/// stylesheet and any grammars it declares off disk, and resolve its view into
/// an arrangement.
///
/// Reads are best-effort by design — see [`SiteTheme::warnings`].
pub async fn read_theme<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    spec: &SiteSpec,
    views: &[ViewSpec],
) -> SiteTheme {
    let mut warnings = Vec::new();
    let template = read_asset(ws, spec, "shell", spec.shell.as_deref(), &mut warnings).await;
    let custom_css = read_asset(
        ws,
        spec,
        "stylesheet",
        spec.stylesheet.as_deref(),
        &mut warnings,
    )
    .await;

    let mut syntaxes = Vec::new();
    for rel in &spec.syntaxes {
        if let Some(text) = read_asset(ws, spec, "syntax", Some(rel), &mut warnings).await {
            syntaxes.push((rel.clone(), text));
        }
    }

    SiteTheme {
        title: spec.display_label(),
        template,
        custom_css,
        // Named by the site's documents rather than by its declaration, so it
        // is [`read_page_shells`]'s to fill once they have been collected.
        shells: BTreeMap::new(),
        syntaxes,
        lang: spec
            .lang
            .as_deref()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .unwrap_or(DEFAULT_LANG)
            .to_string(),
        arrangement: arrangement_for(spec, views),
        warnings,
    }
}

/// A site's arrangement, from the view it declares.
///
/// A site naming no view — or one naming a view the vault no longer declares —
/// is arranged by containment, which is [`Arrangement`]'s default and what a
/// site published before views existed gets.
pub fn arrangement_for(spec: &SiteSpec, views: &[ViewSpec]) -> Arrangement {
    spec.view
        .as_deref()
        .and_then(|name| views.iter().find(|v| v.name == name))
        .map(|view| Arrangement::Grouped(view.group.clone()))
        .unwrap_or_default()
}

/// Read every shell template this site's own **pages** name, and fold them into
/// the theme.
///
/// The claim lives in a document rather than in the site's declaration, so the
/// set of files to read is only knowable once the site has been collected —
/// which is why this is a second pass over [`read_theme`]'s result rather than
/// part of it. Each distinct path is read once however many pages name it.
///
/// Unreadable files are *reported and skipped*, exactly as a missing site shell
/// is: the pages that named one are published in the site's shell, and the
/// render says so as well ([`plates_render::site::SiteRender::page_shell_errors`]).
pub async fn read_page_shells<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    sources: &[SourceFile],
    theme: &mut SiteTheme,
) {
    // Every path this pass has already answered for, readable or not — a file
    // that will not open is one warning, not one per page that named it.
    let mut seen: HashSet<String> = HashSet::new();
    for source in sources {
        let Ok(parsed) = plates_render::frontmatter::parse_or_empty(&source.source_markdown) else {
            continue;
        };
        let Some(rel) = plates_render::frontmatter::get_string(&parsed.frontmatter, "shell")
            .map(str::trim)
            .filter(|p| !p.is_empty())
        else {
            continue;
        };
        if !seen.insert(rel.to_string()) {
            continue;
        }
        match ws.fs().read_to_string(&ws.fs_path(rel)).await {
            Ok(text) => {
                theme.shells.insert(rel.to_string(), text);
            }
            Err(e) => theme.warnings.push(format!(
                "{} declares shell {rel:?}, which could not be read ({e}) — \
                 publishing it in the site's shell",
                source.source_rel_path
            )),
        }
    }
}

/// Read one vault-relative theme file, recording why it could not be read
/// rather than failing the publish over it.
async fn read_asset<FS: Storage + Clone, Id, Ix: IdIndex>(
    ws: &Workspace<FS, Id, Ix>,
    spec: &SiteSpec,
    what: &str,
    rel: Option<&str>,
    warnings: &mut Vec<String>,
) -> Option<String> {
    let rel = rel.map(str::trim).filter(|p| !p.is_empty())?;
    match ws.fs().read_to_string(&ws.fs_path(rel)).await {
        Ok(text) => Some(text),
        Err(e) => {
            warnings.push(format!(
                "site {:?} declares {what} {rel:?}, which could not be read ({e}) — \
                 publishing with the built-in one",
                spec.name
            ));
            None
        }
    }
}
