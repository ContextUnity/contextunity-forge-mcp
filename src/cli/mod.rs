/// Implements ast support.
pub mod ast;
/// Implements guide support.
pub mod guide;
/// Implements migrate support.
pub mod migrate;
/// Implements milestone commands.
pub mod milestone;
/// Sends a reload signal to running MCP servers.
pub mod reload;
/// Implements task support.
pub mod task;
use crate::{
    core::response::{CoverageOptions, QueryOptions, ResponsePolicy, SourceOptions},
    db::{reader, symbols, traversal, writer},
};
use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
#[derive(Parser, Debug)]
#[command(
    name = "contextunity-forge-mcp",
    version,
    about = "Native code graph, documentation engine, MCP server and CLI"
)]
/// Represents cli data.
pub struct Cli {
    #[arg(long, global = true, env = "FORGE_WORKSPACE_ROOT")]
    /// Optional root value.
    pub root: Option<PathBuf>,
    #[arg(long, global = true, env = "FORGE_DB")]
    /// Optional db value.
    pub db: Option<PathBuf>,
    #[command(subcommand)]
    /// Optional command value.
    pub command: Option<Command>,
}
#[derive(Subcommand, Debug)]
/// Enumerates the supported command values.
pub enum Command {
    /// Represents the migrate case.
    Migrate {
        #[command(subcommand)]
        /// The command value.
        command: migrate::MigrateCommand,
    },
    /// Represents the task case.
    Task {
        #[command(subcommand)]
        /// The command value.
        command: task::TaskCommand,
    },
    /// Runs milestone lifecycle commands.
    Milestone {
        #[command(subcommand)]
        /// The selected milestone operation.
        command: milestone::MilestoneCommand,
    },
    /// Represents the serve case.
    Serve,
    /// Reloads running MCP servers owned by this user.
    Reload,
    /// Represents the build case.
    Build {
        /// Optional workspace root value.
        workspace_root: Option<PathBuf>,
        #[arg(long)]
        /// Optional output value.
        output: Option<PathBuf>,
        #[arg(long)]
        /// Optional adapter value.
        adapter: Option<PathBuf>,
        #[arg(long)]
        /// Whether verbose applies.
        verbose: bool,
    },
    /// Represents the scan case.
    Scan {
        /// Optional workspace root value.
        workspace_root: Option<PathBuf>,
        #[arg(long)]
        /// Optional adapter value.
        adapter: Option<PathBuf>,
        #[arg(long)]
        /// Whether pretty applies.
        pretty: bool,
    },
    /// Represents the delta case.
    Delta {
        /// Optional workspace root value.
        workspace_root: Option<PathBuf>,
        /// The modified files value.
        modified_files: Vec<PathBuf>,
        #[arg(long = "modified", value_name = "FILE")]
        /// The modified value.
        modified: Vec<PathBuf>,
    },
    /// Represents the query case.
    Query {
        #[command(subcommand)]
        /// The command value.
        command: QueryCommand,
    },
    /// Represents the docs case.
    Docs {
        #[command(subcommand)]
        /// The command value.
        command: DocsCommand,
    },
    /// Represents the ast case.
    Ast {
        #[command(subcommand)]
        /// The command value.
        command: AstCommand,
    },
    /// Represents the guide case.
    Guide {
        /// The topic value.
        topic: String,
        #[arg(long)]
        /// Whether force applies.
        force: bool,
    },
    /// Represents the checkpoint case.
    Checkpoint {
        /// The action value.
        action: String,
        #[arg(long)]
        /// Optional name value.
        name: Option<String>,
        #[arg(long)]
        /// Optional content value.
        content: Option<String>,
    },
}
#[derive(Subcommand, Debug)]
/// Enumerates the supported query command values.
pub enum QueryCommand {
    /// Represents the overview case.
    Overview,
    /// Represents the inspect case.
    Inspect {
        /// The selector value.
        selector: String,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
        #[arg(long,default_value_t=true,action=clap::ArgAction::Set,num_args=0..=1,default_missing_value="true")]
        /// Whether show doc applies.
        show_doc: bool,
        #[arg(long)]
        /// Whether show source applies.
        show_source: bool,
    },
    /// Represents the search case.
    Search {
        /// The pattern value.
        pattern: String,
        #[arg(long)]
        /// Optional kind value.
        kind: Option<String>,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
    /// Represents the tests case.
    Tests {
        /// The selector value.
        selector: String,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value = "inbound")]
        /// The direction value.
        direction: String,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
    /// Represents the impact case.
    Impact {
        /// The selector value.
        selector: String,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = 1)]
        /// Maximum impact depth; defaults to one edge.
        depth: u32,
    },
    /// Represents the explain case.
    Explain {
        /// The selector value.
        selector: String,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
    },
    /// Represents the remove case.
    Remove {
        /// The selector value.
        selector: String,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
    },
    /// Represents the analyze case.
    Analyze {
        #[arg(default_value = "")]
        /// The target value.
        target: String,
    },
    /// Represents the run case.
    Run {
        /// The operation value.
        operation: String,
        /// Optional selector value.
        selector: Option<String>,
        /// Restrict selector resolution to a workspace-relative file or directory.
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = 2)]
        /// Maximum graph depth for generic query operations; impact defaults to two.
        depth: u32,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
}
#[derive(Subcommand, Debug)]
/// Enumerates the supported docs command values.
pub enum DocsCommand {
    /// Represents the search case.
    Search {
        /// The query value.
        query: String,
        #[arg(long)]
        /// Optional doc type value.
        doc_type: Option<String>,
        #[arg(long)]
        /// Optional component value.
        component: Option<String>,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
    /// Represents the get case.
    Get {
        /// The path or id value.
        path_or_id: String,
        #[arg(long)]
        /// Optional section value.
        section: Option<String>,
    },
}
#[derive(Subcommand, Debug)]
/// Enumerates the supported ast command values.
pub enum AstCommand {
    /// Represents the grep case.
    Grep {
        /// The pattern value.
        pattern: String,
        #[arg(long = "lang", alias = "language")]
        /// The language value.
        language: String,
        #[arg(long)]
        /// Optional path value.
        path: Option<String>,
        #[arg(long, default_value_t = 100)]
        /// The limit value.
        limit: usize,
    },
}
/// Performs default db.
pub fn default_db(root: &std::path::Path) -> PathBuf {
    root.join(".forge/code-map.sqlite")
}
fn cli_page(limit: usize) -> Result<QueryOptions> {
    QueryOptions::resolve(&ResponsePolicy::default(), Some(limit), 0, None, None)
}

fn cli_source(enabled: bool) -> Result<SourceOptions> {
    SourceOptions::resolve(&ResponsePolicy::default(), Some(enabled), None, None, 0)
}
fn command_root(global: Option<&Path>, positional: Option<&Path>) -> Result<PathBuf> {
    let global = global.map(Path::canonicalize).transpose()?;
    let positional = positional.map(Path::canonicalize).transpose()?;
    if let (Some(global), Some(positional)) = (&global, &positional) {
        if global != positional {
            bail!(
                "conflicting workspace roots: --root {} and positional {}",
                global.display(),
                positional.display()
            );
        }
    }
    Ok(positional
        .or(global)
        .unwrap_or(std::env::current_dir()?.canonicalize()?))
}

fn open_or_rebuild(db: &Path, root: &Path) -> Result<crate::db::reader::LockedConnection> {
    match crate::db::reader::open_locked(db, root) {
        Ok(conn) => Ok(conn),
        Err(err) => {
            let msg = err.to_string();
            let requires_rebuild = err
                .downcast_ref::<crate::db::reader::IndexRebuildRequired>()
                .is_some();
            if requires_rebuild || !db.exists() {
                eprintln!(
                    "Forge index incompatible or missing ({msg}); auto-rebuilding for {}...",
                    root.display()
                );
                crate::db::writer::build(root, db, None)?;
                crate::db::reader::open_locked(db, root)
            } else {
                Err(err)
            }
        }
    }
}
impl Cli {
    /// Performs run.
    pub async fn run(self) -> Result<()> {
        let root = command_root(self.root.as_deref(), None)?;
        let db = self.db.clone().unwrap_or_else(|| default_db(&root));
        let command = self.command.unwrap_or(Command::Serve);
        let result: Value = match command {
            Command::Migrate { command } => migrate::run(&root, command)?,
            Command::Task { command } => task::run(&root, command)?,
            Command::Milestone { command } => milestone::run(&root, command)?,
            Command::Serve => {
                crate::mcp::server::serve(root, db).await?;
                return Ok(());
            }
            Command::Reload => reload::run()?,
            Command::Build {
                workspace_root,
                output,
                adapter,
                verbose: _,
            } => {
                let workspace_root = command_root(self.root.as_deref(), workspace_root.as_deref())?;
                let output = output
                    .or(self.db)
                    .unwrap_or_else(|| default_db(&workspace_root));
                writer::build(&workspace_root, &output, adapter.as_deref())?
            }
            Command::Scan {
                workspace_root,
                adapter,
                pretty: _,
            } => {
                let workspace_root = command_root(self.root.as_deref(), workspace_root.as_deref())?;
                json!(crate::engine::scanner::scan(
                    &workspace_root,
                    adapter.as_deref()
                )?)
            }
            Command::Delta {
                workspace_root,
                mut modified_files,
                modified,
            } => {
                let workspace_root = command_root(self.root.as_deref(), workspace_root.as_deref())?;
                modified_files.extend(modified);
                if modified_files.is_empty() {
                    bail!("delta requires at least one modified file");
                }
                let db = self.db.unwrap_or_else(|| default_db(&workspace_root));
                writer::delta(&workspace_root, &db, &modified_files)?
            }
            Command::Guide { topic, force } => guide::run(&root, &topic, force)?,
            Command::Checkpoint {
                action,
                name,
                content,
            } => guide::checkpoint(
                &root,
                &action,
                name.as_deref(),
                content.map(|s| serde_json::from_str(&s)).transpose()?,
            )?,
            Command::Ast {
                command:
                    AstCommand::Grep {
                        pattern,
                        language,
                        path,
                        limit,
                    },
            } => {
                let conn = open_or_rebuild(&db, &root)?;
                ast::search(&conn, &root, &pattern, &language, path.as_deref(), limit)?
            }
            Command::Query { command } => {
                let conn = open_or_rebuild(&db, &root)?;
                match command {
                    QueryCommand::Overview => reader::overview_with_options(
                        &conn,
                        &reader::OverviewOptions {
                            aspects: None,
                            page: &cli_page(100)?,
                        },
                    )?,
                    QueryCommand::Inspect {
                        selector,
                        path,
                        show_doc,
                        show_source,
                    } => symbols::inspect_with_path_options(
                        &conn,
                        &root,
                        &selector,
                        path.as_deref(),
                        &symbols::InspectOptions {
                            show_doc,
                            source: &cli_source(show_source)?,
                            coverage: CoverageOptions {
                                include_coverage: true,
                            },
                            page: &cli_page(100)?,
                        },
                    )?,
                    QueryCommand::Search {
                        pattern,
                        kind,
                        limit,
                    } => symbols::search_with_options(
                        &conn,
                        &pattern,
                        &symbols::SearchOptions {
                            kind: kind.as_deref(),
                            path: None,
                            include_docs: false,
                            exact: false,
                            page: &cli_page(limit)?,
                        },
                    )?,
                    QueryCommand::Tests {
                        selector,
                        path,
                        direction,
                        limit,
                    } => symbols::tests_paged_with_path(
                        &conn,
                        &selector,
                        path.as_deref(),
                        &direction,
                        &cli_page(limit)?,
                    )?,
                    QueryCommand::Impact {
                        selector,
                        path,
                        depth,
                    } => traversal::traverse_with_path_options(
                        &conn,
                        &selector,
                        path.as_deref(),
                        &traversal::TraversalOptions {
                            depth,
                            inbound: true,
                            mode: None,
                            edge_types: None,
                            page: &cli_page(100)?,
                        },
                    )?,
                    QueryCommand::Explain { selector, path } => symbols::explain_with_path_options(
                        &conn,
                        &root,
                        &selector,
                        path.as_deref(),
                        &symbols::ExplainOptions {
                            direction: None,
                            show_doc: true,
                            source: &cli_source(false)?,
                            coverage: CoverageOptions {
                                include_coverage: true,
                            },
                            page: &cli_page(100)?,
                        },
                    )?,
                    QueryCommand::Remove { selector, path } => traversal::removal_paged_with_path(
                        &conn,
                        &selector,
                        path.as_deref(),
                        &cli_page(100)?,
                    )?,
                    QueryCommand::Analyze { target } => {
                        reader::analyze_paged(&conn, &target, None, &cli_page(100)?)?
                    }
                    QueryCommand::Run {
                        operation,
                        selector,
                        path,
                        depth,
                        limit,
                    } => traversal::query_with_path_options(
                        &conn,
                        &operation,
                        selector.as_deref(),
                        path.as_deref(),
                        &traversal::GraphQueryOptions {
                            depth,
                            direction: None,
                            coverage: CoverageOptions::default(),
                            page: &cli_page(limit)?,
                        },
                    )?,
                }
            }
            Command::Docs { command } => {
                let conn = open_or_rebuild(&db, &root)?;
                match command {
                    DocsCommand::Search {
                        query,
                        doc_type,
                        component,
                        limit,
                    } => reader::search_docs_with_options(
                        &conn,
                        &query,
                        &reader::DocSearchOptions {
                            doc_type: doc_type.as_deref(),
                            component: component.as_deref(),
                            include_excerpt: false,
                            page: &cli_page(limit)?,
                        },
                    )?,
                    DocsCommand::Get {
                        path_or_id,
                        section,
                    } => reader::get_doc_paged(
                        &conn,
                        &path_or_id,
                        section.as_deref(),
                        &cli_page(100)?,
                    )?,
                }
            }
        };
        let formatted = serde_json::to_string_pretty(&result)?;
        if let Ok(adapter) = crate::engine::scanner::load_adapter(&root, None) {
            if adapter.debug {
                let cli_cmd = {
                    let args: Vec<String> = std::env::args().skip(1).collect();
                    if args.is_empty() {
                        "serve".to_string()
                    } else {
                        args.join(" ")
                    }
                };
                crate::core::debug_log::log_cli(&root, &cli_cmd, None, &result);
            }
        }
        if let Err(e) = writeln!(io::stdout(), "{formatted}") {
            if e.kind() == io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(e.into());
        }
        Ok(())
    }
}
