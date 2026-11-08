use super::*;
use crate::acdd::snapshot::{assert_safe_git_environment, FixtureSnapshot};
use crate::acdd::tasks::harness::TaskHarness;
use std::collections::HashMap;

mod harness;

mod signature;
mod commit_scope;
