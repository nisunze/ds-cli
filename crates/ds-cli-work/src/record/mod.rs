//! `ds work record` — the project's record of what happened.
//!
//! A record is the correspondence layer of Project Work: instructions,
//! requests for information, submissions, reviews, decisions, field records.
//! They are read here and nothing more — a record is authored on the Records
//! surface, where the person writing it can see what it will be attached to,
//! and creating one from a terminal would be a governed communication act
//! that this CLI deliberately has no door for.

pub mod list;
pub mod read;
