use clap::{Arg, ArgAction, ArgMatches, Command};
use inquire::Text;
use std::fs::File;
use std::io::Error;
use std::path::MAIN_SEPARATOR_STR;
use std::path::Path;

use crate::db::{SILEX_INIT, connect_silex, get_current_branch};
use crate::utils::ok;

pub mod db;
pub mod utils;
pub mod vcs;
pub mod web;

fn cli() -> Command {
    Command::new("silex")
        .about("An new vcs")
        .author("Saigo Ekitae <saigoekitae@gmail.com>")
        .version(env!("CARGO_PKG_VERSION"))
        .subcommand(Command::new("new").about("create a new silex project"))
        .subcommand(Command::new("status").about("show changes in working directory"))
        .subcommand(Command::new("log").about("Show commit logs"))
        .subcommand(Command::new("diff").about("Show changes between working tree and last commit"))
        .subcommand(
            Command::new("commit")
                .about("Record changes to the repository")
                .arg(
                    Arg::new("message")
                        .short('m')
                        .long("message")
                        .help("Description of the changes")
                        .required(true)
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("restore")
                .about("Discard changes in working directory")
                .arg(
                    Arg::new("path")
                        .help("The file to restore")
                        .required(true)
                        .action(ArgAction::Set),
                ),
        )
        .subcommand(
            Command::new("sync")
                .about("Backup repository to a destination (USB, Drive...)")
                .arg(
                    Arg::new("path")
                        .required(true)
                        .action(ArgAction::Set)
                        .help("Destination path"),
                ),
        )
        .subcommand(
            Command::new("branch")
                .about("Create a new branch")
                .arg(Arg::new("name").required(true).action(ArgAction::Set)),
        )
        .subcommand(
            Command::new("checkout")
                .about("Switch branches or restore working tree files")
                .arg(Arg::new("name").required(true).action(ArgAction::Set)),
        )
        .subcommand(
            Command::new("feat")
                .about("Manage feature branches")
                .subcommand(
                    Command::new("start")
                        .about("Start a new feature")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set)),
                )
                .subcommand(
                    Command::new("finish")
                        .about("Merge and close a feature")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set)),
                ),
        )
        .subcommand(
            Command::new("hotfix")
                .about("Manage hotfix branches")
                .subcommand(
                    Command::new("start")
                        .about("Start a critical fix from main")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set)),
                )
                .subcommand(
                    Command::new("finish")
                        .about("Apply fix to main and close")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set)),
                ),
        )
        .subcommand(
            Command::new("tag")
                .about("Manage version tags")
                .subcommand(
                    Command::new("create")
                        .about("Create a new tag at HEAD")
                        .arg(Arg::new("name").required(true).action(ArgAction::Set))
                        .arg(
                            Arg::new("message")
                                .short('m')
                                .help("Description")
                                .action(ArgAction::Set),
                        ),
                )
                .subcommand(Command::new("list").about("List all tags")),
        )
        .subcommand(
            Command::new("web").about("Start the web interface").arg(
                Arg::new("port")
                    .short('p')
                    .default_value("3000")
                    .action(ArgAction::Set),
            ),
        )
}

fn perform_commit(args: &ArgMatches) -> Result<(), Error> {
    let current_dir = std::env::current_dir()?;
    let current_dir_str = current_dir.to_str().unwrap();

    if !Path::new(".silex").exists() {
        return Err(Error::other("Not a silex repository."));
    }

    let connection =
        connect_silex(Path::new(current_dir_str)).map_err(|e| Error::other(e.to_string()))?;

    // On récupère le message depuis les arguments CLI
    let message = args.get_one::<String>("message").unwrap();

    // Pour l'instant on hardcode l'auteur, plus tard on le lira dans `config`
    let author = "Saigo Ekitae";

    vcs::commit(&connection, message, author).map_err(|e| Error::other(e.to_string()))?;

    Ok(())
}
pub fn check_status() -> Result<(), Error> {
    let current_dir = std::env::current_dir()?;
    let current_dir_str = current_dir.to_str().unwrap();
    if !Path::new(&format!("{MAIN_SEPARATOR_STR}.silex")).exists() && !Path::new(".silex").exists()
    {
        return Err(Error::other("Not a silex repository."));
    }

    let connection =
        connect_silex(Path::new(current_dir_str)).map_err(|e| Error::other(e.to_string()))?;
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
    if connect_silex(Path::new(project.as_str()))
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
        Some(("commit", sub_matches)) => {
            return perform_commit(sub_matches);
        }
        Some(("log", _)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_silex(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            return vcs::log(&conn).map_err(|e| Error::other(e.to_string()));
        }
        Some(("diff", _)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_silex(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            return vcs::diff(&conn).map_err(|e| Error::other(e.to_string()));
        }
        Some(("restore", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_silex(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            let path = sub_matches.get_one::<String>("path").unwrap();
            return vcs::restore(&conn, path).map_err(|e| Error::other(e.to_string()));
        }
        Some(("branch", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_silex(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            let name = sub_matches.get_one::<String>("name").unwrap();
            return vcs::create_branch(&conn, name).map_err(|e| Error::other(e.to_string()));
        }
        Some(("checkout", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_silex(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            let name = sub_matches.get_one::<String>("name").unwrap();
            return vcs::checkout(&conn, name).map_err(|e| Error::other(e.to_string()));
        }
        Some(("feat", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_silex(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            // On regarde la SOUS-commande (start ou finish)
            match sub_matches.subcommand() {
                Some(("start", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    return vcs::feature_start(&conn, name)
                        .map_err(|e| Error::other(e.to_string()));
                }
                Some(("finish", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    return vcs::feature_finish(&conn, name)
                        .map_err(|e| Error::other(e.to_string()));
                }
                _ => {
                    println!("Please specify 'start' or 'finish'.");
                    Ok(())
                }
            }
        }
        Some(("hotfix", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_silex(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            match sub_matches.subcommand() {
                Some(("start", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    return vcs::hotfix_start(&conn, name).map_err(|e| Error::other(e.to_string()));
                }
                Some(("finish", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    return vcs::hotfix_finish(&conn, name)
                        .map_err(|e| Error::other(e.to_string()));
                }
                _ => Ok(()),
            }
        }
        Some(("tag", sub_matches)) => {
            let current_dir = std::env::current_dir()?;
            let conn =
                connect_silex(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;

            match sub_matches.subcommand() {
                Some(("create", args)) => {
                    let name = args.get_one::<String>("name").unwrap();
                    let msg = args.get_one::<String>("message").map(|s| s.as_str());
                    return vcs::tag_create(&conn, name, msg);
                }
                Some(("list", _)) => {
                    return vcs::tag_list(&conn);
                }
                _ => {
                    // Par défaut, si l'utilisateur tape juste 'silex tag', on peut lister
                    // Mais avec clap configuré ainsi, il affichera l'aide.
                    println!("Please use 'create' or 'list'.");
                    Ok(())
                }
            }
        }
        Some(("sync", args)) => {
            let current_dir = std::env::current_dir()?;
            let _conn =
                connect_silex(current_dir.as_path()).map_err(|e| Error::other(e.to_string()))?;
            let path = args.get_one::<String>("path").unwrap();
            return vcs::sync(path);
        }
        Some(("web", args)) => {
            let current_dir = std::env::current_dir()?;
            let current_dir_str = current_dir.to_str().unwrap();

            // On vérifie que c'est un dépôt Silex
            if !Path::new(".silex").exists() {
                return Err(Error::other("Not a silex repository."));
            }

            let port: u16 = args
                .get_one::<String>("port")
                .unwrap()
                .parse()
                .unwrap_or(3000);

            // On lance le moteur Asynchrone juste pour cette commande
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(crate::web::start_server(current_dir_str, port));

            Ok(())
        }
        _ => {
            args.clone().print_help().expect("failed to print the help");
            Ok(())
        }
    }
}
