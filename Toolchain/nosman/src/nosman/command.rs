pub mod init;
pub mod remote;
mod install;
mod info;
mod remove;
mod rescan;
mod deinit;

use std::io;

use clap::ArgMatches;
use err_derive::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error(display = "I/O error")]
    IOError(#[error(source)] io::Error),
    #[error(display = "Invalid argument: {}", message)]
    InvalidArgumentError { message: String },
    #[error(display = "Zip error: {}", message)]
    ZipError { message: String },
}

pub(crate) type CommandResult = Result<bool, CommandError>;

pub trait Command {
    fn matched_args<'a>(&self, args : &'a ArgMatches) -> Option<&'a ArgMatches>;
    fn run(&self, args: &ArgMatches) -> CommandResult;

    fn needs_workspace(&self) -> bool {
        true
    }
}

pub fn commands() -> Vec<Box<dyn Command>> {
    vec![
        Box::new(init::InitCommand {}),
        Box::new(remote::RemoteAddCommand {}),
        Box::new(remote::RemoteListCommand {}),
        Box::new(install::InstallCommand {}),
        Box::new(info::InfoCommand {}),
        Box::new(remove::RemoveCommand {}),
        Box::new(rescan::RescanCommand {}),
        Box::new(deinit::DeinitCommand {})
    ]
}