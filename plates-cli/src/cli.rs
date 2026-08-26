//! The argument grammar — the CLI *spelling* of what the library does.
//!
//! Four verbs, and they are four views of one build. `build` writes it to a
//! directory, `watch` keeps writing it, `serve` hands it to a browser, and
//! `clean` takes back exactly what `build` put down. There is one collector and
//! one renderer underneath all four, which is the property that stops a preview
//! and a deploy drifting apart (see [`crate::build`]).

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Where a build lands when nobody says otherwise.
///
/// Underscore-prefixed by the static-site convention, which is not decoration:
/// it sorts away from the archive's own directories, and the hosts that
/// auto-publish a repository skip it, so a build committed by accident does not
/// become a second copy of the site.
pub const DEFAULT_OUT: &str = "_site";

#[derive(Parser, Debug)]
#[command(
    name = "plates",
    version,
    about = "A static site generator over a prov archive",
    long_about = None,
)]
pub struct Cli {
    /// Run as if plates had started in DIR.
    ///
    /// The `git -C` model: the directory is entered once, before anything
    /// resolves, so workspace discovery and every relative path argument agree
    /// about where they are.
    #[arg(
        short = 'C',
        long = "root",
        value_name = "DIR",
        global = true,
        env = "PLATES_ROOT"
    )]
    pub root: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Render the archive's sites into a directory.
    Build {
        #[command(flatten)]
        site: SiteArgs,
        #[command(flatten)]
        out: OutArgs,

        /// Write into a directory holding files no build of ours wrote.
        #[arg(long)]
        force: bool,
    },

    /// Render, then re-render whenever the archive changes.
    Watch {
        #[command(flatten)]
        site: SiteArgs,
        #[command(flatten)]
        out: OutArgs,

        /// Write into a directory holding files no build of ours wrote.
        #[arg(long)]
        force: bool,
    },

    /// Serve the archive's sites on a local address, reloading on change.
    Serve {
        #[command(flatten)]
        site: SiteArgs,

        /// Address to listen on.
        #[arg(long, default_value = "127.0.0.1", value_name = "HOST")]
        host: String,

        /// Port to listen on. Without one, the first free port from 4321 is
        /// taken, so a second archive served alongside the first just works.
        #[arg(short, long, value_name = "PORT")]
        port: Option<u16>,

        /// Open the site in a browser once it is up.
        #[arg(long)]
        open: bool,
    },

    /// Remove the files a previous build wrote.
    Clean {
        #[command(flatten)]
        out: OutArgs,

        /// Remove the whole destination directory, manifest or no manifest.
        ///
        /// Without it `clean` removes only what a build recorded having
        /// written, and refuses a directory it has no record of — a `--out`
        /// typed one character wrong should not be a way to delete somebody's
        /// work.
        #[arg(long)]
        force: bool,
    },
}

/// Which site, and against which address — shared by every verb that renders.
#[derive(clap::Args, Debug, Clone)]
pub struct SiteArgs {
    /// Render only the site with this name.
    ///
    /// For `build` and `watch` that site is written *at* the destination root
    /// rather than under its own name, and for `serve` it answers at `/`,
    /// because someone who named one site asked for one site.
    #[arg(long, value_name = "NAME")]
    pub site: Option<String>,

    /// The absolute URL the finished site will live at.
    ///
    /// Canonical links, the sitemap, `robots.txt` and the feeds are written
    /// against it; without one they are skipped, which is the right default for
    /// a preview whose address is `localhost`.
    #[arg(long, value_name = "URL")]
    pub base_url: Option<String>,
}

/// Where a build lands — shared by `build`, `watch` and `clean`.
#[derive(clap::Args, Debug, Clone)]
pub struct OutArgs {
    /// Directory to write the site into.
    #[arg(short, long, value_name = "DIR", default_value = DEFAULT_OUT, env = "PLATES_OUT")]
    pub out: PathBuf,
}
