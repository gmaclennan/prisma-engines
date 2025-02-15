#![deny(rust_2018_idioms, unsafe_code)]

mod commands;
mod logger;

use migration_core::rpc_api;
use structopt::StructOpt;
use user_facing_errors::{common::SchemaParserError, UserFacingError};

/// When no subcommand is specified, the migration engine will default to starting as a JSON-RPC
/// server over stdio.
#[derive(Debug, StructOpt)]
#[structopt(version = env!("GIT_HASH"))]
struct MigrationEngineCli {
    /// Path to the datamodel
    #[structopt(short = "d", long, name = "FILE")]
    datamodel: Option<String>,
    #[structopt(subcommand)]
    cli_subcommand: Option<SubCommand>,
}

#[derive(Debug, StructOpt)]
enum SubCommand {
    /// Doesn't start a server, but allows running specific commands against Prisma.
    #[structopt(name = "cli")]
    Cli(commands::Cli),
}

impl SubCommand {
    #[cfg(test)]
    fn unwrap_cli(self) -> commands::Cli {
        match self {
            SubCommand::Cli(cli) => cli,
        }
    }
}

#[tokio::main]
async fn main() {
    user_facing_errors::set_panic_hook();
    logger::init_logger();

    let input = MigrationEngineCli::from_args();

    match input.cli_subcommand {
        None => {
            if let Some(datamodel_location) = input.datamodel.as_ref() {
                start_engine(datamodel_location).await
            } else {
                panic!("Missing --datamodel");
            }
        }
        Some(SubCommand::Cli(cli_command)) => {
            tracing::info!(git_hash = env!("GIT_HASH"), "Starting migration engine CLI");
            cli_command.run().await;
        }
    }
}

async fn start_engine(datamodel_location: &str) -> ! {
    use std::io::Read as _;

    tracing::info!(git_hash = env!("GIT_HASH"), "Starting migration engine RPC server",);
    let mut file = std::fs::File::open(datamodel_location).expect("error opening datamodel file");

    let mut datamodel = String::new();
    file.read_to_string(&mut datamodel).unwrap();

    match rpc_api(&datamodel).await {
        // Block the thread and handle IO in async until EOF.
        Ok(api) => json_rpc_stdio::run(&api).await.unwrap(),
        Err(err) => {
            let user_facing_error = err.to_user_facing();
            let exit_code =
                if user_facing_error.as_known().map(|err| err.error_code) == Some(SchemaParserError::ERROR_CODE) {
                    1
                } else {
                    250
                };

            serde_json::to_writer(std::io::stdout().lock(), &user_facing_error).expect("failed to write to stdout");
            std::process::exit(exit_code)
        }
    }

    std::process::exit(0);
}
