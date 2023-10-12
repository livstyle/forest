// Copyright 2019-2023 ChainSafe Systems
// SPDX-License-Identifier: Apache-2.0, MIT
//! # Custom lints
//!
//! A simple, syntactical custom linting framework for forest, to unconditionally
//! forbid certain constructs.
//!
//! Excessive custom lints can be a codebase hazard, so careful consideration is
//! required for what to lint.
//!
//! Out of scope for the current design:
//! - Any conditionality.
//!   We intentionally don't support any `#[allow(..)]`-type gating.
//! - Resolved types, modules.
//! - Cross-file scope.
//!
//! ## Alternative designs.
//!
//! [`clippy`](https://github.com/rust-lang/rust-clippy/) can handle all of the
//! "out of scope" points above.
//! But is a lot more heavyweight, as is the similar project [`dylint`](https://github.com/trailofbits/dylint).
//!
//! If we need more functionality, we should consider porting.
//!
//! ## Technical overview
//!
//! - We parse `rustc`'s Makefile-style dependency files to know which source files
//!   to lint.
//!   This means that new, e.g. `examples/...` artifacts don't need special handling.
//! - We use [`syn`] to parse source files into an Abstract Syntax Tree.
//!   These are inputs to the custom linters, which are run on each file.
//!   Linters return [`proc_macro2::Span`]s to point to lint violations.
//! - We use [`ariadne`] to format violations into pretty `rustc`-style error
//!   messages.
//!   This involves converting [`proc_macro2::Span`]s to utf-8 character offsets
//!   into the file.

mod lints;

use std::{
    io,
    ops::Range,
    process::{self, Command},
};

use ariadne::{ReportKind, Source};
use cargo_metadata::{
    camino::{Utf8Path, Utf8PathBuf},
    Message, MetadataCommand,
};
use itertools::Itertools as _;
use lints::{Lint, Violation};
use proc_macro2::{LineColumn, Span};
use syn::visit::Visit;
use tracing::{debug, info, trace};

#[test]
fn lint() {
    use tracing_subscriber::{filter::LevelFilter, util::SubscriberInitExt as _};
    let _guard = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(LevelFilter::INFO)
        .with_writer(io::stderr)
        .set_default();
    LintRunner::new()
        .run::<lints::NoTestsWithReturn>()
        .run::<lints::SpecializedAssertions>()
        .finish();
}

#[must_use = "you must drive the runner to completion"]
struct LintRunner {
    files: Cache,
    num_violations: usize,
}

impl LintRunner {
    /// Performs source file discovery and parsing.
    ///
    /// # Panics
    /// - freely
    pub fn new() -> Self {
        info!("collecting source files...");

        // 1. get the package ids (there is only one in this case)
        let metadata = MetadataCommand::new().no_deps().exec().unwrap();
        // note: we need
        //           `forest-filecoin 0.13.0 (path+file:///home/aatif/chainsafe/forest)`
        //       as returned by `cargo metadata`, not
        //           `file:///home/aatif/chainsafe/forest#forest-filecoin@0.13.0`
        //       as returned by `cargo pkgid`
        let all_pkg_ids = metadata
            .packages
            .iter()
            .map(|it| &it.id)
            .collect::<Vec<_>>();
        debug!(collected_package_ids = all_pkg_ids.iter().join(", "));

        // 2. get all the final artifacts
        let output = Command::new("cargo")
            .args([
                "check",
                "--workspace", // fwd-compatibility
                "--message-format=json",
                "--quiet",
                "--all-targets",
                "--all-features",
            ])
            .output()
            .unwrap();

        assert!(output.status.success());

        let artifacts = Message::parse_stream(output.stdout.as_slice())
            .map(Result::unwrap)
            .filter_map(|msg| match msg {
                Message::CompilerArtifact(artifact)
                    if all_pkg_ids.contains(&&artifact.package_id) =>
                {
                    debug!(source_file = %artifact.target.src_path);
                    Some(artifact)
                }
                _ => None,
            });

        // 3. get depfiles
        let depfiles = artifacts
            .flat_map(|artifact| {
                match artifact
                    .target
                    .kind // there could be bugs here - see documentation on field
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    // target/debug/build/forest-filecoin-63ff492e456e0923/build-script-build
                    // -> target/debug/build/forest-filecoin-63ff492e456e0923/build_script_build-63ff492e456e0923.d
                    ["custom-build"] => {
                        assert_eq!(artifact.filenames.len(), 1);
                        let filename = &artifact.filenames[0];
                        let file_stem = filename.file_stem().unwrap();
                        assert_eq!(file_stem, "build-script-build");
                        let (_, hash) = filename
                            .parent()
                            .and_then(|it| {
                                it.components().last().and_then(|it| {
                                    it.as_str().rsplit_once(|c| !char::is_alphanumeric(c))
                                })
                            })
                            .unwrap();
                        vec![filename.with_file_name(format!("build_script_build-{}.d", hash))]
                    }
                    // target/debug/deps/libforest_wallet-fa26ebcb4b76d710.rmeta
                    // -> target/debug/deps/forest_wallet-fa26ebcb4b76d710.d
                    ["bin"] | ["example"] | ["test"] | ["bench"] | ["lib"] => artifact
                        .filenames
                        .iter()
                        .map(|it| {
                            assert_eq!(it.extension().unwrap(), "rmeta");
                            let new_file_name = it
                                .with_extension("d")
                                .file_name()
                                .unwrap()
                                .replacen("lib", "", 1);
                            it.with_file_name(new_file_name)
                        })
                        .collect::<Vec<_>>(),
                    other => panic!("unexpected artifact.target.kind: {}", other.join(", ")),
                }
            })
            .inspect(|it| debug!(depfile = %it))
            .map(std::fs::read_to_string)
            .map(Result::unwrap);

        // 4. Collect all source files by parsing the depfiles
        let all_source_files = depfiles
            .flat_map(|depfile| {
                let dependencies = depfile
                    .lines()
                    .filter(|it| !(it.starts_with('#') || it.is_empty()))
                    .map(|it| {
                        let (target, _precursors) = it.split_once(':').unwrap();
                        Utf8PathBuf::from(target)
                    })
                    .collect::<Vec<_>>();
                trace!(dependencies = %dependencies.iter().join(", "));
                dependencies
            })
            .unique();

        // 5. Load all the source files, skipping non-existent or non-rust files
        let files = all_source_files
            .flat_map(|path| {
                let text = std::fs::read_to_string(&path).ok()?;
                let ast = syn::parse_file(&text).ok()?;
                Some((path, (Source::from(text), ast)))
            })
            .collect::<Cache>();

        info!(num_source_files = files.map.len());

        Self {
            files,
            num_violations: 0,
        }
    }

    /// Run the given linter.
    ///
    /// This prints out any messages, and updates the internal failure count.
    pub fn run<T: for<'a> Visit<'a> + Default + Lint>(mut self) -> Self {
        info!("running {}", std::any::type_name::<T>());
        let mut linter = T::default();
        let mut all_violations = vec![];
        for (path, (text, ast)) in self.files.map.iter() {
            linter.visit_file(ast);
            for Violation {
                span,
                message,
                color,
            } in linter.flush()
            {
                let mut label = ariadne::Label::new((path, span2span(text, span)));
                if let Some(message) = message {
                    label = label.with_message(message)
                }
                if let Some(color) = color {
                    label = label.with_color(color)
                }
                all_violations.push(label)
            }
        }
        let num_violations = all_violations.len();
        let auto = Utf8PathBuf::new(); // ariadne figures out the file label if it doesn't have one
        let mut builder = ariadne::Report::build(ReportKind::Error, &auto, 0)
            .with_labels(all_violations)
            .with_message(T::DESCRIPTION);
        if let Some(help) = T::HELP {
            builder.set_help(help)
        }
        if let Some(note) = T::NOTE {
            builder.set_note(note)
        }
        match num_violations {
            _none @ 0 => {}
            _mid @ 1..=20 => {
                builder.finish().print(&self.files).unwrap();
            }
            _many => {
                builder
                    .with_config(ariadne::Config::default().with_compact(true))
                    .finish()
                    .print(&self.files)
                    .unwrap();
            }
        }
        self.num_violations += num_violations;
        self
    }
    /// Exits the program with an appropriate error code.
    pub fn finish(self) -> ! {
        match self.num_violations {
            0 => {
                println!("no violations found in {} files", self.files.map.len());
                process::exit(0)
            }
            nonzero => {
                println!(
                    "found {} violations in {} files",
                    nonzero,
                    self.files.map.len()
                );
                process::exit(1)
            }
        }
    }
}

/// Stores all the files for repeated linting and formatting into pretty reports
struct Cache {
    map: ahash::HashMap<Utf8PathBuf, (Source, syn::File)>,
}

impl<Id> ariadne::Cache<Id> for &Cache
where
    Id: AsRef<str>,
{
    fn fetch(&mut self, id: &Id) -> Result<&Source, Box<dyn std::fmt::Debug + '_>> {
        self.map
            .get(Utf8Path::new(&id))
            .map(|(text, _ast)| text)
            .ok_or(Box::new(format!("{} not in cache", id.as_ref())))
    }

    fn display<'a>(&self, id: &'a Id) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(id.as_ref()))
    }
}

impl FromIterator<(Utf8PathBuf, (Source, syn::File))> for Cache {
    fn from_iter<T: IntoIterator<Item = (Utf8PathBuf, (Source, syn::File))>>(iter: T) -> Self {
        Self {
            map: iter.into_iter().collect(),
        }
    }
}

fn span2span(text: &Source, span: Span) -> Range<usize> {
    coord2offset(text, span.start())..coord2offset(text, span.end())
}

fn coord2offset(text: &Source, coord: LineColumn) -> usize {
    let line = text.line(coord.line - 1).expect("line is past end of file");
    assert!(coord.column <= line.len(), "column is past end of line");
    line.offset() + coord.column
}