mod era_command;
mod genesis_command;
mod serve_command;
mod tools_command;

use anyhow::anyhow;
use clap::{CommandFactory, Parser, Subcommand};
use config::{builder::DefaultState, ConfigBuilder, Map, Source, Value, ValueKind};
use mithril_common::StdResult;
use mithril_doc::{DocExtractor, DocExtractorDefault, StructDoc};
use slog::Level;
use slog_scope::debug;
use std::path::PathBuf;

use crate::{Configuration, DefaultConfiguration};
use mithril_doc::GenerateDocCommands;

/// Main command selector
#[derive(Debug, Clone, Subcommand)]
pub enum MainCommand {
    Genesis(genesis_command::GenesisCommand),
    Era(era_command::EraCommand),
    Serve(serve_command::ServeCommand),
    Tools(tools_command::ToolsCommand),
    #[clap(alias("doc"))]
    GenerateDoc(GenerateDocCommands),
}

impl MainCommand {
    pub async fn execute(&self, config_builder: ConfigBuilder<DefaultState>) -> StdResult<()> {
        match self {
            Self::Genesis(cmd) => cmd.execute(config_builder).await,
            Self::Era(cmd) => cmd.execute(config_builder).await,
            Self::Serve(cmd) => cmd.execute(config_builder).await,
            Self::Tools(cmd) => cmd.execute(config_builder).await,
            Self::GenerateDoc(cmd) => {
                let config_infos = vec![Configuration::extract(), DefaultConfiguration::extract()];
                match cmd.execute_with_configurations(&mut MainOpts::command(), &config_infos) {
                    Ok(()) => StdResult::Ok(()),
                    Err(message) => StdResult::Err(anyhow!(message)),
                }
                // Ok(cmd.execute_with_configurations(&mut MainOpts::command(), &config_infos)?)
            }
        }
    }
}

/// Mithril Aggregator Node
#[derive(DocExtractor, Parser, Debug, Clone)]
#[command(version)]
pub struct MainOpts {
    /// application main command
    #[clap(subcommand)]
    pub command: MainCommand,

    /// Run Mode
    #[clap(short, long, default_value = "dev")]
    pub run_mode: String,

    /// Verbosity level
    #[clap(short, long, action = clap::ArgAction::Count)]
    #[example = "Parsed from the number of occurrences: `-v` for `Warning`, `-vv` for `Info`, `-vvv` for `Debug` and `-vvvv` for `Trace`"]
    pub verbose: u8,

    /// Directory of the Cardano node files
    #[clap(long)]
    pub db_directory: Option<PathBuf>,

    /// Directory where configuration file is located
    #[clap(long, default_value = "./config")]
    pub config_directory: PathBuf,
}

impl Source for MainOpts {
    fn clone_into_box(&self) -> Box<dyn Source + Send + Sync> {
        Box::new(self.clone())
    }

    fn collect(&self) -> Result<Map<String, Value>, config::ConfigError> {
        let mut result = Map::new();
        let namespace = "clap arguments".to_string();

        if let Some(db_directory) = self.db_directory.clone() {
            result.insert(
                "db_directory".to_string(),
                Value::new(
                    Some(&namespace),
                    ValueKind::from(format!("{}", db_directory.to_string_lossy())),
                ),
            );
        }

        Ok(result)
    }
}

impl MainOpts {
    /// execute command
    pub async fn execute(&self) -> StdResult<()> {
        let config_file_path = self
            .config_directory
            .join(format!("{}.json", self.run_mode));
        let config_builder = config::Config::builder()
            .add_source(DefaultConfiguration::default())
            .add_source(
                config::File::with_name(&config_file_path.to_string_lossy()).required(false),
            )
            .add_source(config::Environment::default().separator("__"))
            .add_source(self.clone());
        debug!("Started"; "run_mode" => &self.run_mode, "node_version" => env!("CARGO_PKG_VERSION"));

        self.command.execute(config_builder).await
    }

    /// get log level from parameters
    pub fn log_level(&self) -> Level {
        match self.verbose {
            0 => Level::Error,
            1 => Level::Warning,
            2 => Level::Info,
            3 => Level::Debug,
            _ => Level::Trace,
        }
    }
}
