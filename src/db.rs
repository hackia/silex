use sqlite::{Connection, State};
use std::io::{Error, ErrorKind};

pub const SILEX_INIT : &str = "CREATE TABLE blobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT UNIQUE NOT NULL,      -- Hash SHA-256 du contenu pour déduplication
    content BLOB,                   -- Le contenu réel (peut être zlib compressé)
    size INTEGER NOT NULL,
    mime_type TEXT                  -- Utile pour une UI web rapide sans analyser le binaire
);

CREATE TABLE assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid TEXT UNIQUE NOT NULL,      -- Identifiant unique universel de l'asset
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    creator_id INTEGER              -- Qui a introduit ce fichier pour la première fois
);

CREATE TABLE commits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT UNIQUE NOT NULL,       -- Hash calculé sur les métadonnées + parent
    parent_hash TEXT,                -- NULL si c'est le commit initial
    author TEXT NOT NULL,
    message TEXT NOT NULL,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(parent_hash) REFERENCES commits(hash)
);

CREATE TABLE manifest (
    commit_id INTEGER NOT NULL,
    asset_id INTEGER NOT NULL,
    blob_id INTEGER NOT NULL,
    
    file_path TEXT NOT NULL,         -- Le chemin PEUT changer d'un commit à l'autre pour le même asset_id
    permissions INTEGER DEFAULT 644, -- 755 pour exécutable, etc.
    
    PRIMARY KEY (commit_id, asset_id),
    FOREIGN KEY (commit_id) REFERENCES commits(id),
    FOREIGN KEY (asset_id) REFERENCES assets(id),
    FOREIGN KEY (blob_id) REFERENCES blobs(id)
);

CREATE TABLE branches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    head_commit_id INTEGER NOT NULL,
    FOREIGN KEY (head_commit_id) REFERENCES commits(id)
);

CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT UNIQUE NOT NULL,
    commit_id INTEGER NOT NULL,
    description TEXT,
    FOREIGN KEY (commit_id) REFERENCES commits(id)
);
CREATE TABLE config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO config (key, value) VALUES ('current_branch', 'main');
";

pub fn get_current_branch(conn: &Connection) -> Result<String, Error> {
    let query = "SELECT value FROM config WHERE key = 'current_branch'";
    let mut statement = conn
        .prepare(query)
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;

    if let Ok(State::Row) = statement.next() {
        let branch_name: String = statement
            .read("value")
            .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
        Ok(branch_name)
    } else {
        // Fallback si la config est cassée, mais ça ne devrait pas arriver
        Err(Error::new(
            ErrorKind::Other,
            "FATAL: Could not determine current branch.",
        ))
    }
}
