use clap::Command;
use inquire::Text;
use std::fs::File;
use std::io::Error;
use std::path::MAIN_SEPARATOR_STR;
use std::{fs::create_dir_all, path::Path};

use crate::db::{SILEX_INIT, get_current_branch};
use crate::utils::ok;

pub mod db;
pub mod utils;
pub mod vcs;

fn cli() -> Command {
    Command::new("silex")
        .about("An new vcs")
        .author("Saigo Ekitae <saigoekitae@gmail.com>")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand(Command::new("new").about("create a new silex project"))
        .subcommand(Command::new("status").about("show changes in working directory"))
}

fn conn(p: &str) -> Result<sqlite::Connection, sqlite::Error> {
    sqlite::open(format!(
        "{p}{MAIN_SEPARATOR_STR}.silex{MAIN_SEPARATOR_STR}db{MAIN_SEPARATOR_STR}silex.db"
    ))
}

fn check_status() -> Result<(), Error> {
    let current_dir = std::env::current_dir()?;
    let current_dir_str = current_dir.to_str().unwrap();
    if !Path::new(&format!("{MAIN_SEPARATOR_STR}.silex")).exists() && !Path::new(".silex").exists()
    {
        return Err(Error::other("Not a silex repository."));
    }

    let connection = conn(current_dir_str).map_err(|e| Error::other(e.to_string()))?;
    vcs::status(
        &connection,
        current_dir_str,
        get_current_branch(&connection)
            .expect("failed to get current branch")
            .as_str(),
    )
    .map_err(|e| Error::other(e.to_string()))?;
    Ok(())
}

fn new_project() -> Result<(), Error> {
    let mut project = String::new();
    while project.is_empty() {
        project.clear();
        project = Text::new("Name:")
            .prompt()
            .expect("failed to get name")
            .to_string();
        if (Path::new(project.as_str())).is_dir() {
            project.clear();
        }
    }
    create_dir_all(format!("{project}{MAIN_SEPARATOR_STR}.silex{MAIN_SEPARATOR_STR}db").as_str())
        .expect("failed to create the .silex directory");

    if conn(project.as_str())
        .expect("failed to get the connexion")
        .execute(SILEX_INIT)
        .is_ok()
    {
        File::create_new(format!("{project}{MAIN_SEPARATOR_STR}silexium").as_str())
            .expect("failed to create file");
        ok("silexium file created successfully");
        ok("project created successsfully");
        Ok(())
    } else {
        Err(Error::other("failed to create the sqlite database"))
    }
}

fn main() -> Result<(), Error> {
    let args = cli();
    let app = args.clone().get_matches();
    match app.subcommand() {
        Some(("new", _)) => {
            return new_project();
        }
        Some(("status", _)) => {
            return check_status();
        }
        _ => {
            args.clone().print_help().expect("failed to print the help");
            Ok(())
        }
    }
}
