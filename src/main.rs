use clap::Command;
use inquire::Text;
use std::io::Error;
use std::path::MAIN_SEPARATOR_STR;
use std::{env::current_dir, fs::create_dir_all, path::Path};

fn cli() -> Command {
    Command::new("silex")
        .about("An new vcs")
        .author("Saigo Ekitae <saigoekitae@gmail.com>")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand(Command::new("new").about("create a new silex project"))
}

fn conn(p: &str) -> Result<sqlite::Connection, sqlite::Error> {
    sqlite::open(format!(
        "{p}{MAIN_SEPARATOR_STR}.silex{MAIN_SEPARATOR_STR}db{MAIN_SEPARATOR_STR}silex.db"
    ))
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

    if conn(project.as_str()).is_ok() {
        Ok(())
    } else {
        Err(Error::other("failed to create the db"))
    }
}

fn main() -> Result<(), Error> {
    let args = cli();
    let app = args.clone().get_matches();
    match app.subcommand() {
        Some(("new", _)) => {
            return new_project();
        }
        _ => {
            args.clone().print_help().expect("failed to print the help");
            Ok(())
        }
    }
}
