

![test workflow](https://github.com/jasonmc/fixepub/actions/workflows/test.yml/badge.svg)

# Fix ePub

This is a simple tool written in Rust to correct ePub file problems for use with Amazon Kindle devices. This was inspired heavily by https://kindle-epub-fix.netlify.app/ whose JS source is available: https://github.com/innocenat/kindle-epub-fix


## Usage

```sh
cargo build --release
./target/release/fixepub book.epub another-book.epub
./target/release/fixepub --language fr-CA book.epub
./target/release/fixepub --force book.epub
```

Each input produces a `-fixed` file alongside the original. Existing output files
are protected unless `--force` is supplied. Output is committed only after the
whole repair succeeds; failed repairs leave existing files intact.

The tool removes images without a nonempty `src`, removes link fragments that
point to a chapter's body ID, ensures XHTML has an XML declaration, and repairs
missing or invalid package language metadata. The fallback language defaults to
`en`; use `--language` to choose another valid BCP 47 tag. Valid language metadata
is preserved.

Documents are parsed as XML, preserving namespaces, comments, and CDATA. Supported
input encodings are decoded without character replacement and modified documents
are emitted as UTF-8. Malformed XML and unsupported encodings cause an error that
identifies the document, instead of producing potentially damaged output.

Unchanged entries are copied in their compressed form. Binary assets are never
loaded into memory, and document processing holds only one document at a time.
The `mimetype` entry is written first and uncompressed.

## Development

Linux releases are static musl binaries for x86-64, built with Nix. They do not
require Nix or distribution-specific shared libraries. Build one on Linux with
`nix build .#static`; the binary is at `result/bin/fixepub`. CI runs its tests and
checks that it starts in Alpine and Debian containers without Nix installed.

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

`nix flake check` also runs the test suite through nextest.
