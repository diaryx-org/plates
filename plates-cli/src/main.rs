//! `plates` — a static site generator over a prov archive.
//!
//! A thin adapter: parse arguments, call into the libraries, render the result.
//! What a site *is* lives in [`plates`], what a page *looks like* lives in
//! [`plates_render`], and what leaves the archive at all is `prov`'s — reached
//! through [`plates::prov`], so the whole tree names one of it. What lives here
//! is the three things a library must not decide:
//!
//! - **The vocabulary.** [`config`] reads the `sites:` block. `plates` takes a
//!   [`SiteSpec`](plates::SiteSpec) already built, deliberately, so that two
//!   applications over one archive format can spell a declaration differently
//!   and still produce the same website.
//! - **Where a build lands**, and what taking it back means — [`manifest`] and
//!   [`mod@write`].
//! - **When to build.** Once ([`commands::build`]), on every change
//!   ([`commands::watch`]), or on demand behind a socket ([`serve`]).
//!
//! Underneath all four verbs is one collector and one renderer ([`build`]),
//! which is what keeps a preview and a deploy from drifting apart.

use std::process::ExitCode;

use clap::Parser;

mod build;
mod cli;
mod commands;
mod config;
mod manifest;
mod scan;
mod serve;
mod session;
mod write;

fn main() -> ExitCode {
    let args = cli::Cli::parse();

    // `-C <dir>` (or `PLATES_ROOT`) runs plates as if it had started in that
    // directory: entered once, up front, so archive discovery and every
    // relative path argument resolve there — the `git -C` model, in one place.
    if let Some(dir) = &args.root
        && let Err(e) = std::env::set_current_dir(dir)
    {
        return fail(&format!("could not use {}: {e}", dir.display()));
    }

    let result = match &args.command {
        cli::Command::Build { site, out, force } => commands::build(site, out, *force),
        cli::Command::Watch { site, out, force } => commands::watch(site, out, *force),
        cli::Command::Serve {
            site,
            host,
            port,
            open,
        } => serve::serve(site, host, *port, *open),
        cli::Command::Clean { out, force } => commands::clean(out, *force),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}

/// One failure shape for every verb: the reason on stderr, and a non-zero exit
/// so a script that chained something after this stops.
fn fail(message: &str) -> ExitCode {
    eprintln!("✗ {message}");
    ExitCode::FAILURE
}
