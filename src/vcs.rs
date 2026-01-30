use crate::utils::ok;
use crate::utils::ok_status;
use ignore::DirEntry;
use similar::{ChangeTag, TextDiff};
use sqlite::Connection;
use sqlite::Error;
use sqlite::State;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Error as IoError, ErrorKind}; // On renomme pour clarifier
use std::io::{Read, Result as IoResult};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug)]
pub enum FileStatus {
    New(PathBuf),           // N'existe pas en base -> Nouvel Asset
    Modified(PathBuf, i64), // Existe mais hash différent -> Même Asset
    Deleted(PathBuf, i64),  // Existe en base mais plus sur disque
    Unchanged,
}
pub struct ManifestEntry {
    path: String,
    blob_id: i64,
    asset_id: i64,
    perm: i64,
}

pub fn tag_create(conn: &Connection, name: &str, message: Option<&str>) -> Result<(), IoError> {
    // 1. On récupère le commit actuel (HEAD)
    let current_branch = crate::db::get_current_branch(conn)
        .map_err(|e| IoError::new(ErrorKind::Other, e.to_string()))?;

    let (head_id, head_hash) = get_branch_head_info(conn, &current_branch)
        .map_err(|e| IoError::new(ErrorKind::Other, e.to_string()))?;

    if head_id.is_none() {
        return Err(IoError::other(
            "Cannot tag an empty branch. Commit something first.",
        ));
    }

    // 2. On insère le tag
    let query = "INSERT INTO tags (name, commit_id, description) VALUES (?, ?, ?)";
    let mut stmt = conn
        .prepare(query)
        .map_err(|e| IoError::new(ErrorKind::Other, e.to_string()))?;

    stmt.bind((1, name)).unwrap();
    stmt.bind((2, head_id.unwrap())).unwrap();
    stmt.bind((3, message)).unwrap();

    match stmt.next() {
        Ok(_) => ok(&format!(
            "Tag '{}' created on commit {}",
            name,
            &head_hash[0..7]
        )),
        Err(_) => return Err(IoError::other(format!("Tag '{}' already exists.", name))),
    }
    Ok(())
}

pub fn tag_list(conn: &Connection) -> Result<(), IoError> {
    // On joint avec la table commits pour afficher le hash correspondant
    let query = "
        SELECT t.name, t.description, c.hash
        FROM tags t
        JOIN commits c ON t.commit_id = c.id
        ORDER BY t.name
    ";
    let mut stmt = conn
        .prepare(query)
        .map_err(|e| IoError::new(ErrorKind::Other, e.to_string()))?;

    println!("Existing tags:");
    let mut count = 0;
    while let Ok(State::Row) = stmt.next() {
        let name: String = stmt.read("name").unwrap();
        let desc: Option<String> = stmt.read("description").unwrap_or(None);
        let hash: String = stmt.read("hash").unwrap();

        let desc_str = desc.unwrap_or_else(|| String::new());
        // Affichage : Nom (Jaune) -> Hash (Gris) Description
        println!(
            "  \x1b[1;33m{}\x1b[0m -> {} \x1b[90m{}\x1b[0m",
            name,
            &hash[0..7],
            desc_str
        );
        count += 1;
    }

    if count == 0 {
        println!("  (No tags yet)");
    }
    Ok(())
}

// --- GESTION GIT FLOW (OPTIMISÉE) ---

pub fn hotfix_start(conn: &Connection, name: &str) -> Result<(), Error> {
    let branch_name = format!("hotfix/{}", name);
    let source_branch = "main"; // CONTRAINTE : Un hotfix part toujours de la prod

    // 1. On vérifie qu'on part bien de 'main' pour avoir la base saine
    let (main_id, _) = get_branch_head_info(conn, source_branch)?;
    if main_id.is_none() {
        return Err(Error {
            code: Some(1),
            message: Some(String::from("No main branches has been founded")),
        });
    }

    // 2. On crée la branche manuellement (sans utiliser create_branch qui utilise HEAD)
    let query = "INSERT INTO branches (name, head_commit_id) VALUES (?, ?)";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch_name.as_str()))?;
    stmt.bind((2, main_id.unwrap()))?;

    match stmt.next() {
        Ok(_) => {} // Création OK
        Err(_) => {
            return Err(Error {
                code: Some(1),
                message: Some(String::from("hotfix already exist")),
            });
        }
    }

    // 3. On bascule dessus
    checkout(conn, &branch_name)?;

    ok(&format!(
        "Hotfix started: Switched to '{branch_name}' from 'main'"
    ));
    Ok(())
}

pub fn hotfix_finish(conn: &Connection, name: &str) -> Result<(), Error> {
    // C'est la même logique que feature_finish, mais sémantiquement distinct
    let hotfix_branch = format!("hotfix/{}", name);
    let target_branch = "main";

    let (hf_head_id, _) = get_branch_head_info(conn, &hotfix_branch)?;
    if hf_head_id.is_none() {
        return Err(Error {
            code: Some(1),
            message: Some(String::from("hotfix not exist")),
        });
    }

    ok(format!("Switching to '{target_branch}' to apply hotfix...").as_str());
    checkout(conn, target_branch)?;

    // Fast-Forward Merge
    let query = "UPDATE branches SET head_commit_id = ? WHERE name = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, hf_head_id.unwrap()))?;
    stmt.bind((2, target_branch))?;
    stmt.next()?;

    ok("Hotfix applied to main");

    // Nettoyage
    let delete_query = "DELETE FROM branches WHERE name = ?";
    let mut del_stmt = conn.prepare(delete_query)?;
    del_stmt.bind((1, hotfix_branch.as_str()))?;
    del_stmt.next()?;
    ok(&format!("Hotfix '{}' finished and branch deleted.", name));
    Ok(())
}

pub fn feature_start(conn: &Connection, name: &str) -> Result<(), Error> {
    // 1. Standardisation du nom : feature/nom
    let branch_name = format!("feature/{}", name);

    // 2. On crée la branche (basée sur le HEAD actuel)
    // create_branch gère déjà l'erreur si elle existe
    create_branch(conn, &branch_name)?;

    // 3. On bascule dessus immédiatement (Optimisation UX)
    checkout(conn, &branch_name)?;

    ok(&format!("Flow started: You are now on '{branch_name}'"));
    Ok(())
}

pub fn feature_finish(conn: &Connection, name: &str) -> Result<(), Error> {
    let feat_branch = format!("feature/{}", name);
    let target_branch = "main"; // Ou "dev" selon ta philosophie

    // 1. Sécurité : On vérifie que la branche feature existe
    let (feat_head_id, _) = get_branch_head_info(conn, &feat_branch)?;
    if feat_head_id.is_none() {
        return Err(Error {
            code: Some(1),
            message: Some(String::from("main branch not exist")),
        });
    }

    // 2. On bascule sur 'main' pour préparer la fusion
    ok(format!("Switching to '{target_branch}' to merge changes...").as_str());
    checkout(conn, target_branch)?;

    // 3. LE FAST-FORWARD (L'optimisation ultime)
    // Au lieu de calculer un diff, on déplace juste le pointeur de main sur la tête de la feature
    let query = "UPDATE branches SET head_commit_id = ? WHERE name = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, feat_head_id.unwrap()))?;
    stmt.bind((2, target_branch))?;
    stmt.next()?;

    ok("Fast-forward merge complete");

    // 4. Nettoyage : On supprime la branche temporaire
    let delete_query = "DELETE FROM branches WHERE name = ?";
    let mut del_stmt = conn.prepare(delete_query)?;
    del_stmt.bind((1, feat_branch.as_str()))?;
    del_stmt.next()?;

    ok(&format!("Feature '{name}' finished and branch deleted."));
    Ok(())
}

pub fn create_branch(conn: &Connection, new_branch_name: &str) -> Result<(), Error> {
    // 1. On récupère la branche actuelle et son commit ID
    let current_branch = crate::db::get_current_branch(conn).map_err(|e| e)?;
    let (head_id, _) = get_branch_head_info(conn, &current_branch)?;

    if let Some(id) = head_id {
        // 2. On insère la nouvelle étiquette pointant vers le MEME commit
        let query = "INSERT INTO branches (name, head_commit_id) VALUES (?, ?)";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, new_branch_name))?;
        stmt.bind((2, id))?;

        match stmt.next() {
            Ok(_) => ok(&format!("Branch '{}' created.", new_branch_name)),
            Err(_) => println!(
                "\x1b[1;31mError:\x1b[0m Branch '{}' already exists.",
                new_branch_name
            ),
        }
    } else {
        ok("Cannot branch from an empty repository. Commit something first.");
    }
    Ok(())
}

// --- LE COEUR DU SYSTEME : CHECKOUT ---

pub fn checkout(conn: &Connection, target_branch: &str) -> Result<(), Error> {
    // 1. VÉRIFICATION DE SÉCURITÉ
    // On ne veut pas écraser le travail en cours de l'utilisateur
    let current_dir = std::env::current_dir().unwrap();
    let current_branch = crate::db::get_current_branch(conn).map_err(|e| e)?;

    // Si on est déjà dessus, on ne fait rien
    if current_branch == target_branch {
        ok(&format!("Already on '{}'", target_branch));
        return Ok(());
    }

    let status_list = status(conn, current_dir.to_str().unwrap(), &current_branch)?;
    if !status_list.is_empty() {
        ok("Your changes would be overwritten by checkout.");
        ok("Please commit your changes or stash them first.");
        return Ok(());
    }

    // 2. PRÉPARATION DES DONNÉES
    // On récupère les IDs des commits (Source et Cible)
    let (current_head_id, _) = get_branch_head_info(conn, &current_branch)?;
    let (target_head_id, _) = get_branch_head_info(conn, target_branch)?;

    // Si la branche cible n'existe pas
    if target_head_id.is_none() && target_branch != "main" {
        // Hack si repo vide
        ok(format!("Branch '{target_branch}' not found.").as_str());
        return Ok(());
    }

    // On charge les deux manifestes en mémoire pour comparer
    // Map : Chemin -> (Hash, AssetID)
    let current_files = get_manifest_map(conn, current_head_id)?;
    let target_files = get_manifest_map(conn, target_head_id)?;

    ok(format!("Switched to branch '{target_branch}'").as_str());

    // 3. MISE À JOUR DU DISQUE (Différentiel)

    // A. Gérer les AJOUTS et MODIFICATIONS (Target vs Current)
    for (path, (target_hash, _)) in &target_files {
        let should_write = match current_files.get(path) {
            Some((current_hash, _)) => current_hash != target_hash, // Modifié
            None => true,                                           // Nouveau fichier
        };

        if should_write {
            // On récupère le contenu binaire depuis le store
            if let Some(content) = get_blob_bytes_by_hash(conn, target_hash)? {
                // On crée les dossiers parents si nécessaire
                if let Some(parent) = Path::new(path).parent() {
                    std::fs::create_dir_all(parent).expect("failed to create directory");
                }
                std::fs::write(path, content).expect("failed to write content");
            }
        }
    }

    // B. Gérer les SUPPRESSIONS (Ce qui est dans Current mais plus dans Target)
    for (path, _) in &current_files {
        if !target_files.contains_key(path) {
            if Path::new(path).exists() {
                std::fs::remove_file(path).expect("failed to remove the file");
                // Optionnel : Supprimer les dossiers vides parents
            }
        }
    }

    // 4. METTRE À JOUR LA CONFIGURATION
    let query = "UPDATE config SET value = ? WHERE key = 'current_branch'";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, target_branch))?;
    stmt.next()?;

    Ok(())
}

// --- NOUVEAUX HELPERS ---

// Récupère tout le manifeste d'un commit sous forme de HashMap facile à comparer
fn get_manifest_map(
    conn: &Connection,
    commit_id: Option<i64>,
) -> Result<HashMap<String, (String, i64)>, Error> {
    let mut map = HashMap::new();
    if let Some(id) = commit_id {
        let query = "
            SELECT m.file_path, b.hash, m.asset_id 
            FROM manifest m
            JOIN store.blobs b ON m.blob_id = b.id
            WHERE m.commit_id = ?
        ";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, id))?;
        while let Ok(State::Row) = stmt.next() {
            let path: String = stmt.read("file_path")?;
            let hash: String = stmt.read("hash")?;
            let asset_id: i64 = stmt.read("asset_id")?;
            map.insert(path, (hash, asset_id));
        }
    }
    Ok(map)
}

// Récupère les octets via le hash (plus rapide que via le path)
fn get_blob_bytes_by_hash(conn: &Connection, hash: &str) -> Result<Option<Vec<u8>>, Error> {
    let query = "SELECT content FROM store.blobs WHERE hash = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, hash))?;
    if let Ok(State::Row) = stmt.next() {
        Ok(Some(stmt.read("content")?))
    } else {
        Ok(None)
    }
}

// Helper pour récupérer le contenu BRUT (bytes) d'un fichier dans le HEAD
// C'est vital pour restaurer des images ou des exécutables sans corruption UTF-8
fn get_blob_bytes(conn: &Connection, branch: &str, path: &Path) -> Result<Option<Vec<u8>>, Error> {
    let relative_path = path.strip_prefix(".").unwrap_or(path).to_string_lossy();

    let query = "
        SELECT b.content 
        FROM branches br
        JOIN manifest m ON m.commit_id = br.head_commit_id
        JOIN store.blobs b ON m.blob_id = b.id
        WHERE br.name = ? AND m.file_path = ?
    ";

    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch))?;
    stmt.bind((2, relative_path.as_ref()))?;

    if let Ok(State::Row) = stmt.next() {
        let content: Vec<u8> = stmt.read("content")?;
        Ok(Some(content))
    } else {
        Ok(None) // Le fichier n'existe pas dans le HEAD
    }
}

pub fn restore(conn: &Connection, path_str: &str) -> Result<(), Error> {
    let path = Path::new(path_str);
    let branch = crate::db::get_current_branch(conn).map_err(|e| e)?;

    // 1. On cherche le contenu original dans la BDD
    match get_blob_bytes(conn, &branch, path)? {
        Some(content) => {
            // 2. Le fichier existe dans le HEAD, on l'écrase sur le disque
            std::fs::write(path, content).expect("failed to restore");
            ok(&format!("Restored '{}' from HEAD.", path.display()));
        }
        None => {
            // 3. Cas particulier : Le fichier n'est pas dans le HEAD (c'est un fichier purement nouveau)
            // Dans ce cas, 'restore' ne peut rien faire (ou devrait proposer de le supprimer)
            println!(
                "\x1b[1;31mError:\x1b[0m File '{}' does not exist in the last commit.",
                path.display()
            );
        }
    }
    Ok(())
}

pub fn diff(conn: &Connection) -> Result<(), Error> {
    let current_dir = std::env::current_dir().unwrap();
    let current_dir_str = current_dir.to_str().unwrap();
    let branch = crate::db::get_current_branch(conn).map_err(|e| e)?;

    // 1. On récupère les changements (on réutilise ta logique de status)
    let changes = status(conn, current_dir_str, &branch)?;

    if changes.is_empty() {
        return Ok(());
    }

    for change in changes {
        match change {
            FileStatus::Modified(path, _) => {
                println!("\n\x1b[1;33mDiff: {}\x1b[0m", path.display());
                println!("\x1b[90m==================================================\x1b[0m");

                // A. Lire le nouveau contenu sur le disque
                let new_content = match std::fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => {
                        println!("(Binary or unreadable file)");
                        continue;
                    }
                };

                // B. Récupérer l'ancien contenu depuis la BDD (via le Hash du HEAD)
                let old_content = get_file_content_from_head(conn, &branch, &path)?;

                // C. Calculer et afficher le Diff
                let diff = TextDiff::from_lines(&old_content, &new_content);

                for change in diff.iter_all_changes() {
                    let (sign, color) = match change.tag() {
                        ChangeTag::Delete => ("-", "\x1b[31m"), // Rouge
                        ChangeTag::Insert => ("+", "\x1b[32m"), // Vert
                        ChangeTag::Equal => (" ", "\x1b[0m"),   // Blanc
                    };
                    print!("{}{}{}\x1b[0m", color, sign, change);
                }
            }
            FileStatus::New(path) => {
                println!(
                    "\n\x1b[1;32mNew File: {}\x1b[0m (All content is new)",
                    path.display()
                );
            }
            FileStatus::Deleted(path, _) => {
                println!("\n\x1b[1;31mDeleted File: {}\x1b[0m", path.display());
            }
            _ => {}
        }
    }
    Ok(())
}

// Helper pour récupérer le contenu textuel d'un blob depuis le HEAD
fn get_file_content_from_head(
    conn: &Connection,
    branch: &str,
    path: &Path,
) -> Result<String, Error> {
    let relative_path = path.strip_prefix(".").unwrap_or(path).to_string_lossy();

    // 1. Trouver le hash du fichier dans le commit HEAD
    let query = "
        SELECT b.content 
        FROM branches br
        JOIN manifest m ON m.commit_id = br.head_commit_id
        JOIN store.blobs b ON m.blob_id = b.id
        WHERE br.name = ? AND m.file_path = ?
    ";

    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch))?;
    stmt.bind((2, relative_path.as_ref()))?;

    if let Ok(State::Row) = stmt.next() {
        // Attention : On suppose ici que c'est du texte (UTF-8)
        let content_blob: Vec<u8> = stmt.read("content")?;
        match String::from_utf8(content_blob) {
            Ok(s) => Ok(s),
            Err(_) => Ok(String::from("(Binary content)")),
        }
    } else {
        Ok(String::new()) // Fichier introuvable (ne devrait pas arriver si Modified)
    }
}

pub fn log(conn: &Connection) -> Result<(), Error> {
    // 1. Trouver où on est (HEAD de la branche actuelle
    let current_branch = crate::db::get_current_branch(conn).map_err(|e| e)?;
    // On réutilise ta fonction helper pour avoir le hash du dernier commit
    let (_, mut current_hash) = get_branch_head_info(conn, &current_branch)?;

    if current_hash.is_empty() {
        ok("No commits yet.");
        return Ok(());
    }

    println!(
        "Historique pour la branche \x1b[1;36m{}\x1b[0m :\n",
        current_branch
    );

    // 2. La boucle temporelle (On remonte les parents)
    while !current_hash.is_empty() {
        let query = "
            SELECT hash, author, message, timestamp, parent_hash 
            FROM commits 
            WHERE hash = ?
        ";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, current_hash.as_str()))?;

        if let Ok(State::Row) = stmt.next() {
            let hash: String = stmt.read("hash")?;
            let author: String = stmt.read("author")?;
            let message: String = stmt.read("message")?;
            let date: String = stmt.read("timestamp")?;
            let parent: Option<String> = stmt.read("parent_hash").ok();

            // Affichage stylé (façon Git)
            println!("\x1b[33mcommit {}\x1b[0m", hash);
            println!("Author: {}", author);
            println!("Date:   {}", date);
            println!("\n    {}\n", message);
            println!("\x1b[90m----------------------------------------\x1b[0m");

            // 3. On passe au père (Remonter le temps)
            current_hash = parent.unwrap_or_default();
        } else {
            break; // Plus de commit trouvé (ne devrait pas arriver si l'intégrité est bonne)
        }
    }

    Ok(())
}

pub fn commit(conn: &Connection, message: &str, author: &str) -> Result<(), Error> {
    let current_branch = crate::db::get_current_branch(conn).map_err(|e| e)?;
    let (parent_id, parent_hash) = get_branch_head_info(conn, &current_branch)?;

    // 1. Charger l'état parent complet (ID + Hash)
    let parent_state = get_parent_asset_map(conn, parent_id)?;

    let root_path = ".";
    let walk = ignore::WalkBuilder::new(root_path)
        .add_custom_ignore_filename("silexium")
        .standard_filters(true)
        .build();

    let mut new_manifest: Vec<ManifestEntry> = Vec::new();
    let mut changes_count = 0; // Compteur de modifs

    // ON OUVRE LA TRANSACTION MAIS ON NE VALIDE PAS TOUT DE SUITE
    conn.execute("BEGIN TRANSACTION;")?;

    for result in walk {
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        if path.components().any(|c| c.as_os_str() == ".silex") || path.is_dir() {
            continue;
        }

        let content_hash = match calculate_hash(path) {
            Ok(h) => h,
            Err(_) => continue,
        };

        let metadata = std::fs::metadata(path).expect("failed to get metadata");
        let blob_id = ensure_blob_exists(conn, &content_hash, path, metadata.len())?;

        let relative_path = path
            .strip_prefix("./")
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        // --- DÉTECTION INTELLIGENTE ---
        let asset_id = match parent_state.get(&relative_path) {
            Some((id, old_hash)) => {
                // Le fichier existait déjà : a-t-il changé ?
                if old_hash != &content_hash {
                    changes_count += 1; // MODIFIED
                }
                *id
            }
            None => {
                changes_count += 1; // NEW FILE
                create_new_asset(conn, author)?
            }
        };

        new_manifest.push(ManifestEntry {
            path: relative_path,
            blob_id,
            asset_id,
            perm: if metadata.permissions().readonly() {
                444
            } else {
                644
            },
        });
    }

    // --- DÉTECTION DES SUPPRESSIONS ---
    // Si un fichier était dans le parent mais n'est pas dans le nouveau manifest -> DELETED
    for (old_path, _) in &parent_state {
        if !new_manifest.iter().any(|e| e.path == *old_path) {
            changes_count += 1;
        }
    }

    // --- LE VERDICT ---
    if changes_count == 0 {
        conn.execute("ROLLBACK;")?; // On annule tout, on ne touche pas à la DB
        ok("Nothing to commit, working tree clean."); // Message clair
        return Ok(());
    }

    // Si on arrive ici, c'est qu'il y a des changements -> ON ÉCRIT !

    // (Le reste du code reste identique : calcul hash, insert commits, insert manifest...)
    let timestamp = chrono::Utc::now().to_rfc3339();
    let commit_hash_input = format!("{}{}{}{}", parent_hash, author, message, timestamp);
    let commit_hash = blake3::hash(commit_hash_input.as_bytes())
        .to_hex()
        .to_string();

    let query_commit = "INSERT INTO commits (hash, parent_hash, author, message, timestamp) VALUES (?, ?, ?, ?, ?) RETURNING id;";
    let mut stmt = conn.prepare(query_commit)?;
    stmt.bind((1, commit_hash.as_str()))?;
    stmt.bind((
        2,
        if parent_hash.is_empty() {
            None
        } else {
            Some(parent_hash.as_str())
        },
    ))?;
    stmt.bind((3, author))?;
    stmt.bind((4, message))?;
    stmt.bind((5, timestamp.as_str()))?;
    stmt.next()?;
    let new_commit_id: i64 = stmt.read("id")?;

    let query_manifest = "INSERT INTO manifest (commit_id, asset_id, blob_id, file_path, permissions) VALUES (?, ?, ?, ?, ?)";
    let mut stmt_m = conn.prepare(query_manifest)?;

    for entry in new_manifest {
        stmt_m.reset()?;
        stmt_m.bind((1, new_commit_id))?;
        stmt_m.bind((2, entry.asset_id))?;
        stmt_m.bind((3, entry.blob_id))?;
        stmt_m.bind((4, entry.path.as_str()))?;
        stmt_m.bind((5, entry.perm))?;
        stmt_m.next()?;
    }

    let query_upsert = "INSERT INTO branches (name, head_commit_id) VALUES (?, ?) ON CONFLICT(name) DO UPDATE SET head_commit_id = excluded.head_commit_id";
    let mut stmt_b = conn.prepare(query_upsert)?;
    stmt_b.bind((1, current_branch.as_str()))?;
    stmt_b.bind((2, new_commit_id))?;
    stmt_b.next()?;

    drop(stmt);
    drop(stmt_m);
    drop(stmt_b);

    conn.execute("COMMIT;")?; // Validation finale

    ok(&format!(
        "Commit {} created successfully!",
        &commit_hash[0..7]
    ));
    Ok(())
}
// --- FONCTIONS HELPER (A mettre aussi dans vcs.rs) ---
fn get_branch_head_info(conn: &Connection, branch: &str) -> Result<(Option<i64>, String), Error> {
    let query = "
        SELECT c.id, c.hash 
        FROM branches b 
        JOIN commits c ON b.head_commit_id = c.id 
        WHERE b.name = ?";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, branch))?;

    if let Ok(State::Row) = stmt.next() {
        Ok((Some(stmt.read("id")?), stmt.read("hash")?))
    } else {
        Ok((None, String::new())) // Pas de parent (Premier commit)
    }
}

// Remplace get_parent_asset_map par ceci :
fn get_parent_asset_map(
    conn: &Connection,
    parent_id: Option<i64>,
) -> Result<HashMap<String, (i64, String)>, Error> {
    let mut map = HashMap::new();
    if let Some(pid) = parent_id {
        // On récupère l'ID mais AUSSI le HASH pour comparer
        let query = "
            SELECT m.file_path, m.asset_id, b.hash 
            FROM manifest m 
            JOIN store.blobs b ON m.blob_id = b.id 
            WHERE m.commit_id = ?
        ";
        let mut stmt = conn.prepare(query)?;
        stmt.bind((1, pid))?;
        while let Ok(State::Row) = stmt.next() {
            let path: String = stmt.read("file_path")?;
            let asset_id: i64 = stmt.read("asset_id")?;
            let hash: String = stmt.read("hash")?;
            map.insert(path, (asset_id, hash));
        }
    }
    Ok(map)
}

fn ensure_blob_exists(conn: &Connection, hash: &str, path: &Path, size: u64) -> Result<i64, Error> {
    // 1. Vérifier si le blob existe déjà (DÉDUPLICATION)
    let check_query = "SELECT id FROM store.blobs WHERE hash = ?"; // Note le 'store.' !
    let mut stmt = conn.prepare(check_query)?;
    stmt.bind((1, hash))?;

    if let Ok(State::Row) = stmt.next() {
        return Ok(stmt.read("id")?);
    }

    // 2. Si non, on l'insère
    // Attention : lire tout le fichier en RAM pour l'insérer en BLOB peut être lourd.
    // Pour l'instant on fait simple :
    let content = std::fs::read(path).expect("failed to read file");

    let insert_query = "
        INSERT INTO store.blobs (hash, content, size) 
        VALUES (?, ?, ?) 
        RETURNING id";
    let mut stmt_ins = conn.prepare(insert_query)?;
    stmt_ins.bind((1, hash))?;
    stmt_ins.bind((2, &content[..]))?; // Bind byte array
    stmt_ins.bind((3, size as i64))?;

    stmt_ins.next()?;
    Ok(stmt_ins.read("id")?)
}

fn create_new_asset(conn: &Connection, _creator: &str) -> Result<i64, Error> {
    let new_uuid = Uuid::new_v4().to_string();
    let query = "INSERT INTO store.assets (uuid) VALUES (?) RETURNING id";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, new_uuid.as_str()))?;
    stmt.next()?;
    Ok(stmt.read("id")?)
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
