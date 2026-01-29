use crate::utils::ok;
use crate::utils::ok_status;
use ignore::DirEntry;
use sqlite::Connection;
use sqlite::State;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::Error;
use std::io::{Read, Result as IoResult};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum FileStatus {
    New(PathBuf),           // N'existe pas en base -> Nouvel Asset
    Modified(PathBuf, i64), // Existe mais hash différent -> Même Asset
    Deleted(PathBuf, i64),  // Existe en base mais plus sur disque
    Unchanged,
}
pub fn get_head_state(
    conn: &Connection,
    branch: &str,
) -> Result<HashMap<PathBuf, (String, i64)>, sqlite::Error> {
    let mut state_map = HashMap::new();
    let query_head = "SELECT head_commit_id FROM branches WHERE name = ?";
    let mut statement = conn.prepare(query_head)?;
    statement.bind((1, branch))?;

    let head_commit_id = if let Ok(State::Row) = statement.next() {
        statement.read::<i64, _>("head_commit_id")?
    } else {
        return Ok(state_map); // Pas de commit, repo vide
    };

    // 2. Récupérer le manifest de ce commit
    let query_manifest = "
        SELECT m.file_path, b.hash, m.asset_id 
        FROM manifest m
        JOIN store.blobs b ON m.blob_id = b.id
        WHERE m.commit_id = ?
    ";
    let mut statement = conn.prepare(query_manifest)?;
    statement.bind((1, head_commit_id))?;

    while let Ok(State::Row) = statement.next() {
        let path_str: String = statement.read("file_path")?;
        let hash: String = statement.read("hash")?;
        let asset_id: i64 = statement.read("asset_id")?;

        state_map.insert(PathBuf::from(path_str), (hash, asset_id));
    }

    Ok(state_map)
}

pub fn status(conn: &Connection, root_path: &str, branch: &str) -> Result<Vec<FileStatus>, Error> {
    let db_state = get_head_state(conn, branch).expect("failed to get db state");
    let mut changes = Vec::new();
    let mut files_on_disk: HashSet<PathBuf> = HashSet::new();
    let walk = ignore::WalkBuilder::new(root_path)
        .add_custom_ignore_filename("silexium")
        .threads(4)
        .standard_filters(true)
        .build()
        .flatten()
        .collect::<Vec<DirEntry>>();

    for path in &walk {
        if path.path().components().any(|c| c.as_os_str() == ".silex") || path.path().is_dir() {
            continue;
        }

        let relative_path = path
            .path()
            .strip_prefix(root_path)
            .expect("failed to get relative path")
            .to_path_buf();
        files_on_disk.insert(relative_path.clone());

        let current_hash = match calculate_hash(path.path()) {
            Ok(h) => h,
            Err(_) => continue, // On ignore les fichiers illisibles (ou on log un warning)
        };
        // Comparaison
        match db_state.get(&relative_path) {
            Some((db_hash, asset_id)) => {
                if *db_hash != current_hash {
                    changes.push(FileStatus::Modified(relative_path, *asset_id));
                }
            }
            None => {
                // Le fichier n'est pas dans le manifest -> New
                changes.push(FileStatus::New(relative_path));
            }
        }
    }
    for (path, (_, asset_id)) in db_state {
        if !files_on_disk.contains(&path) {
            changes.push(FileStatus::Deleted(path, asset_id));
        }
    }
    if changes.is_empty() {
        ok("No changes detected. Working tree is clean.");
    } else {
        for change in &changes {
            ok_status(&change);
        }
    }
    Ok(changes)
}

pub fn calculate_hash(path: &Path) -> IoResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 1024]; // Buffer de lecture

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex::encode(hasher.finalize().as_bytes()))
}
