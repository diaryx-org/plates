---
title: cargo doc writes the CLI's pages over the library's
description: The `plates` binary in plates-cli and the `plates` library both document to `target/doc/plates/`, so the Docs job's `cargo doc --workspace` leaves the library's index replaced by the CLI's, and passes
part_of: '[Tasks](/docs/tasks/tasks.md)'
created: 2026-10-27
status: open
updated: 2026-10-27
audience: public
author: adammharris
---

# cargo doc writes the CLI's pages over the library's

## Repro

```sh
cargo doc --workspace --all-features --no-deps    # the Docs job, at 0ce2fc1
```

Cargo warns, and the job still passes, because `RUSTDOCFLAGS='-D warnings'`
covers rustdoc's warnings and not Cargo's:

```text
warning: output filename collision at target/doc/plates/index.html
  = note: the bin target `plates` in package `plates-cli` has the same output
    filename as the lib target `plates` in package `plates`
  = note: this is a known bug where multiple crates with the same name use the
    same path; see <https://github.com/rust-lang/cargo/issues/6313>
```

`plates-cli/Cargo.toml` names its binary `plates`, so that the installed
command is `plates`, and rustdoc writes both crates into
`target/doc/plates/`. `plates-cli` is documented last, so
`target/doc/plates/index.html` is the binary's: it links `build`, `cli`,
`commands`, `config`, `manifest`, `scan`, `serve`, `session` and `write`, and
none of the library's `collect`, `digest`, `error`, `links`, `mount`, `plan`,
`source`, `spec`, `term` or `theme`. The library's own module pages are not
written either. Someone reading the workspace's docs locally sees no library.

## Candidates

- **`doc = false` on the `[[bin]]`.** Tried: the collision goes, and
  `target/doc/plates/` is the library's again. The binary's modules are then
  not documented at all, which loses nothing published (the workspace is
  `publish = false`, and the binary's modules are private) but does lose
  them locally.
- **Document the binary under another name**, with its modules moved into a
  `plates_cli` library target the `plates` binary calls. The binary's docs
  then have a directory of their own.
- **Either way, make the Docs job fail on the warning**, so a second
  collision cannot come back unseen; for example by refusing any
  `output filename collision` in `cargo doc`'s output.

## Done when

`cargo doc --workspace --all-features --no-deps` prints no collision
warning, `target/doc/plates/index.html` is the library's, and the Docs job
fails if a collision comes back.
