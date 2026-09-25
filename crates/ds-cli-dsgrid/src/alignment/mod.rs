//! `ds dsgrid alignment …` — typed facts of a model's alignments, over the
//! family plumbing in [`crate::mutation`].
//!
//! `gap show|set` reads and authors the multiple-alignment gap: the gap the
//! model's global stationing leaves before each alignment's start, which the
//! PLS-CADD export writes on the NUM break rows and into every later DON
//! global station.

pub mod gap;
