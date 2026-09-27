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
use std::path::PathBuf;
#[derive(Parser, Debug)]
#[command(
    name = "contextunity-forge-mcp",
    version,
    about = "Native code graph, documentation engine, MCP server and CLI"
)]
pub struct Cli {
    #[arg(long, global = true, default_value = ".", env = "FORGE_WORKSPACE_ROOT")]
    pub root: PathBuf,
    #[arg(long, global = true, env = "FORGE_DB")]
    pub db: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Option<Command>,
}
#[derive(Subcommand, Debug)]
pub enum Command {
    Serve,
    Build {
        #[arg(default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        adapter: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
    },
    Scan {
        #[arg(default_value = ".")]
        workspace_root: PathBuf,
        #[arg(long)]
        adapter: Option<PathBuf>,
        #[arg(long)]
        pretty: bool,
    },
    Delta {
        workspace_root: PathBuf,
        #[arg(required = true)]
        modified_files: Vec<PathBuf>,
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
impl Cli {
    pub async fn run(self) -> Result<()> {
        let root = self.root.canonicalize()?;
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
                let workspace_root = workspace_root.canonicalize()?;
                let output = output
                    .or(self.db)
                    .unwrap_or_else(|| default_db(&workspace_root));
                build::build(&workspace_root, &output, adapter.as_deref())?
            }
            Command::Scan {
                workspace_root,
                adapter,
                pretty: _,
            } => json!(crate::engine::scanner::scan(
                &workspace_root,
                adapter.as_deref()
            )?),
            Command::Delta {
                workspace_root,
                modified_files,
            } => {
                let workspace_root = workspace_root.canonicalize()?;
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
            } => ast::search(&root, &pattern, &language, path.as_deref(), limit)?,
            Command::Query { command } => {
                let conn = crate::db::reader::open(&db, &root)?;
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
                let conn = crate::db::reader::open(&db, &root)?;
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
        if let Err(e) = writeln!(io::stdout(), "{formatted}") {
            if e.kind() == io::ErrorKind::BrokenPipe {
                return Ok(());
            }
            return Err(e.into());
        }
        Ok(())
    }
}
