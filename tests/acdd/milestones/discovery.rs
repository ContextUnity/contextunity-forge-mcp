use super::*;

fn minimal_milestone(id: &str) -> String {
    include_str!("../fixtures/milestones/minimal.md").replace("m-minimal", id)
}

mod bulk_sync;
mod cli;
mod directories;
mod initialization;
mod listing;
mod manifest;
mod selectors;
