//! The document-to-HTML half of `plates`: what a page *looks like*.
//!
//! This crate reads nothing and resolves nothing. It is handed source text and
//! a description of the site that text belongs to, and it gives back HTML —
//! which is what lets one rendering run in a command-line build, in a sync
//! server, and in an edge worker without three implementations quietly
//! disagreeing about what a site looks like.
//!
//! The `plates` crate is the layer above: it walks a [`prov::Workspace`],
//! decides which documents a site holds and where each one lands, and calls
//! this.
//!
//! A body's grammar is its own — Markdown, Djot or HTML, read off the source
//! document's extension via [`prov::ContentFormat`] — and all three go through
//! one parser, `twig`. See [`body`] for why that is the same parser the editor
//! uses and what changed in the output when it stopped being comrak.
//!
//! It must remain portable to `wasm32-unknown-unknown`, which is a constraint
//! rather than a preference: no host functions, no filesystem, no entropy and
//! no clock. A caller that has those reads the files and passes the bytes in.

pub mod appearance;
pub mod body;
pub mod dates;
pub mod frontmatter;
pub mod html;
mod links;
pub mod nav;
pub mod page;
pub mod shell;
#[cfg(feature = "templating")]
pub mod site;
#[cfg(feature = "syntax-highlighting")]
pub mod syntax;
#[cfg(feature = "templating")]
pub mod template;
pub mod types;
pub mod visibility;

pub use appearance::{
    ColorPalette, ContentWidth, FaviconAsset, FontFamily, ThemeAppearance, TypographySettings,
};
#[cfg(feature = "syntax-highlighting")]
pub use body::render_body_with;
pub use body::{preprocess_custom_syntax, render_body};
pub use html::{
    Generator, HtmlRenderer, ISLAND_CHILD_SCRIPT, ISLAND_CHILD_SCRIPT_FILENAME, PageContext,
    SiteStyle,
};
pub use links::{percent_decode, root_prefix, transform_links};
pub use nav::{build_site_nav_tree, nav_for_page};
pub use shell::{ShellError, ShellSlots, ShellTemplate};
#[cfg(feature = "syntax-highlighting")]
pub use syntax::{CLASS_PREFIX, HIGHLIGHTED_CLASS, Syntaxes, highlight_code_blocks};
pub use types::{Arrangement, Grain, Grouping, PageLayout, serve_at_dest};
