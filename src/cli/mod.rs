pub mod ast;
pub mod build;
pub mod delta;
pub mod docs;
pub mod guide;
pub mod query;
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
pub struct Cli {
    #[arg(long, global = true, env = "FORGE_WORKSPACE_ROOT")]
    pub root: Option<PathBuf>,
    #[arg(long, global = true, env = "FORGE_DB")]
    pub db: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand, Debug)]
pub enum Command {
    Serve,
    Build {
        workspace_root: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        adapter: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
    },
    Scan {
        workspace_root: Option<PathBuf>,
        #[arg(long)]
        adapter: Option<PathBuf>,
        #[arg(long)]
        pretty: bool,
    },
    Delta {
        workspace_root: Option<PathBuf>,
        modified_files: Vec<PathBuf>,
        #[arg(long = "modified", value_name = "FILE")]
        modified: Vec<PathBuf>,
    },
    Query {
        #[command(subcommand)]
        command: QueryCommand,
    },
    Docs {
        #[command(subcommand)]
        command: DocsCommand,
    },
    Ast {
        #[command(subcommand)]
        command: AstCommand,
    },
    Guide {
        topic: String,
        #[arg(long)]
        force: bool,
    },
    Checkpoint {
        action: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
}
#[derive(Subcommand, Debug)]
pub enum QueryCommand {
    Overview,
    Inspect {
        selector: String,
        #[arg(long,default_value_t=true,action=clap::ArgAction::Set,num_args=0..=1,default_missing_value="true")]
        show_doc: bool,
        #[arg(long)]
        show_source: bool,
    },
    Search {
        pattern: String,
        #[arg(long)]
        kind: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Tests {
        selector: String,
        #[arg(long, default_value = "inbound")]
        direction: String,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Impact {
        selector: String,
        #[arg(long, default_value_t = 2)]
        depth: u32,
    },
    Explain {
        selector: String,
    },
    Remove {
        selector: String,
    },
    Analyze {
        #[arg(default_value = "")]
        target: String,
    },
    Run {
        operation: String,
        selector: Option<String>,
        #[arg(long, default_value_t = 2)]
        depth: u32,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}
#[derive(Subcommand, Debug)]
pub enum DocsCommand {
    Search {
        query: String,
        #[arg(long)]
        doc_type: Option<String>,
        #[arg(long)]
        component: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    Get {
        path_or_id: String,
        #[arg(long)]
        section: Option<String>,
    },
}
#[derive(Subcommand, Debug)]
pub enum AstCommand {
    Grep {
        pattern: String,
        #[arg(long = "lang", alias = "language")]
        language: String,
        #[arg(long)]
        path: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
}
pub fn default_db(root: &std::path::Path) -> PathBuf {
    root.join(".forge/code-map.sqlite")
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
    pub async fn run(self) -> Result<()> {
        let root = command_root(self.root.as_deref(), None)?;
        let db = self.db.clone().unwrap_or_else(|| default_db(&root));
        let command = self.command.unwrap_or(Command::Serve);
        let result: Value = match command {
            Command::Serve => {
                crate::mcp::server::serve(root, db).await?;
                return Ok(());
            }
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
                build::build(&workspace_root, &output, adapter.as_deref())?
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
                delta::delta(&workspace_root, &db, &modified_files)?
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
                    QueryCommand::Overview => query::overview(&conn)?,
                    QueryCommand::Inspect {
                        selector,
                        show_doc,
                        show_source,
                    } => {
                        crate::db::symbols::inspect(&conn, &root, &selector, show_doc, show_source)?
                    }
                    QueryCommand::Search {
                        pattern,
                        kind,
                        limit,
                    } => crate::db::symbols::search(&conn, &pattern, kind.as_deref(), limit)?,
                    QueryCommand::Tests {
                        selector,
                        direction,
                        limit,
                    } => crate::db::symbols::tests(&conn, &selector, &direction, limit)?,
                    QueryCommand::Impact { selector, depth } => {
                        query::traverse(&conn, &selector, depth, true, 1000)?
                    }
                    QueryCommand::Explain { selector } => query::explain(&conn, &selector)?,
                    QueryCommand::Remove { selector } => query::removal(&conn, &selector)?,
                    QueryCommand::Analyze { target } => query::analyze(&conn, &target)?,
                    QueryCommand::Run {
                        operation,
                        selector,
                        depth,
                        limit,
                    } => {
                        if limit == 0 || limit > 10000 {
                            bail!("limit must be1..10000");
                        }
                        query::query(&conn, &operation, selector.as_deref(), depth, limit)?
                    }
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
                    } => docs::search_docs(
                        &conn,
                        &query,
                        doc_type.as_deref(),
                        component.as_deref(),
                        limit,
                    )?,
                    DocsCommand::Get {
                        path_or_id,
                        section,
                    } => docs::get_doc(&conn, &path_or_id, section.as_deref())?,
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
