use chrono::Datelike;
use sqlite::{Connection, Error, State};
use std::fs::create_dir_all;
use std::path::Path;

pub const SILEX_INIT: &str = "
    -- ====================================================================
    -- PARTIE STOCKAGE (store.db) - Données lourdes et immuables
    -- ====================================================================

    CREATE TABLE IF NOT EXISTS store.blobs (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        hash TEXT UNIQUE NOT NULL,      -- Hash Blake3 du contenu (Déduplication)
        content BLOB,                   -- Le contenu réel
        size INTEGER NOT NULL,
        mime_type TEXT                  -- Pour future UI / Stats
    );

    CREATE TABLE IF NOT EXISTS store.assets (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid TEXT UNIQUE NOT NULL,      -- Identité stable du fichier (UUID)
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        creator_id INTEGER              -- ID de l'auteur original (optionnel)
    );

    -- ====================================================================
    -- PARTIE HISTORIQUE (history_YYYY.db) - Métadonnées légères
    -- ====================================================================

    CREATE TABLE IF NOT EXISTS commits (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        hash TEXT UNIQUE NOT NULL,       -- Hash (Merkle) : métadonnées + parent
        parent_hash TEXT,                -- NULL si commit initial
        author TEXT NOT NULL,
        message TEXT NOT NULL,
        timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY(parent_hash) REFERENCES commits(hash)
    );

    CREATE TABLE IF NOT EXISTS manifest (
        commit_id INTEGER NOT NULL,
        asset_id INTEGER NOT NULL,       -- Réfère à store.assets(id) (Géré par Rust)
        blob_id INTEGER NOT NULL,        -- Réfère à store.blobs(id) (Géré par Rust)

        file_path TEXT NOT NULL,         -- Le chemin à cet instant T
        permissions INTEGER DEFAULT 644,

        PRIMARY KEY (commit_id, asset_id),
        FOREIGN KEY (commit_id) REFERENCES commits(id)
        -- NOTE: Pas de FK vers store.* car SQLite ne supporte pas les FK inter-bases
    );

    CREATE TABLE IF NOT EXISTS branches (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT UNIQUE NOT NULL,
        head_commit_id INTEGER NOT NULL,
        FOREIGN KEY (head_commit_id) REFERENCES commits(id)
    );

    CREATE TABLE IF NOT EXISTS tags (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT UNIQUE NOT NULL,
        commit_id INTEGER NOT NULL,
        description TEXT,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
        FOREIGN KEY (commit_id) REFERENCES commits(id)
    );

    CREATE TABLE IF NOT EXISTS config (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
-- ====================================================================
-- PARTIE 1 : MESSAGERIE ÉPHÉMÈRE (Autodestruction à 20h)
-- ====================================================================
CREATE TABLE IF NOT EXISTS ephemeral_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    sender TEXT NOT NULL,
    content TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    -- La date d'expiration est calculée à l'insertion par l'app (prochain 20h)
    expires_at DATETIME NOT NULL, 
    is_read BOOLEAN DEFAULT 0
);

-- Index pour accélérer le nettoyage automatique
CREATE INDEX IF NOT EXISTS idx_messages_expire ON ephemeral_messages(expires_at);

-- ====================================================================
-- PARTIE 2 : TODO LIST INTÉGRÉE
-- ====================================================================
CREATE TABLE IF NOT EXISTS todos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    description TEXT,
    status TEXT DEFAULT 'TODO', -- TODO, IN_PROGRESS, DONE
    assigned_to TEXT,           -- Peut être lié à un auteur de commit
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Trigger pour mettre à jour updated_at automatiquement
CREATE TRIGGER IF NOT EXISTS update_todos_timestamp 
AFTER UPDATE ON todos
BEGIN
    UPDATE todos SET updated_at = CURRENT_TIMESTAMP WHERE id = OLD.id;
END;

-- ====================================================================
-- PARTIE 3 : STATS DE CO-OCCURRENCE (Fichiers modifiés ensemble)
-- ====================================================================
-- Cette table sert de cache pour éviter de scanner tous les commits à chaque fois
CREATE TABLE IF NOT EXISTS file_correlations (
    file_a TEXT NOT NULL,
    file_b TEXT NOT NULL,
    frequency INTEGER DEFAULT 1,
    last_seen_commit_id INTEGER,
    -- On force l'ordre alphabétique (file_a < file_b) pour éviter les doublons (A,B) et (B,A)
    PRIMARY KEY (file_a, file_b)
);

-- ====================================================================
-- PARTIE 4 : LOGS (Style Logstash)
-- ====================================================================
CREATE TABLE IF NOT EXISTS system_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    level TEXT NOT NULL,
    component TEXT NOT NULL,
    message TEXT NOT NULL,
    metadata JSON,
    trace_id TEXT
);        
-- ====================================================================
-- PARTIE 5 : CLASSEMENT DES CONTRIBUTEURS
-- ====================================================================
CREATE TABLE IF NOT EXISTS contributor_stats (
    author_name TEXT PRIMARY KEY,
    total_commits INTEGER DEFAULT 0,
    first_commit_at DATETIME,
    last_commit_at DATETIME,
    files_touched_count INTEGER DEFAULT 0,
    rank_score REAL DEFAULT 0.0 
);
    -- Initialisation par défaut (ignore si existe déjà)
    INSERT OR IGNORE INTO config (key, value) VALUES ('current_branch', 'main');
";

pub fn get_current_branch(conn: &Connection) -> Result<String, Error> {
    let query = "SELECT value FROM config WHERE key = 'current_branch'";
    let mut statement = conn.prepare(query)?;

    if let Ok(State::Row) = statement.next() {
        let branch_name: String = statement.read("value")?;
        Ok(branch_name)
    } else {
        // Fallback si la config est cassée, mais ça ne devrait pas arriver
        Ok(String::from("main"))
    }
}

pub fn connect_silex(root_path: &Path) -> Result<Connection, sqlite::Error> {
    let db_dir = root_path.join(".silex/db");
    let store_path = db_dir.join("store.db");

    // 1. Calculer l'année en cours pour l'historique
    let current_year = chrono::Local::now().year();
    let history_path = db_dir.join(format!("history_{}.db", current_year));

    // Créer les dossiers si nécessaire
    create_dir_all(&db_dir).expect("failed to create the .silex/db directory");

    // 2. Ouvrir la connexion principale sur l'HISTORIQUE (ex: 2026)
    let conn = Connection::open(&history_path)?;

    // 3. Attacher le STOCKAGE (Blobs) sous l'alias 'store'
    // L'astuce est là : on exécute du SQL pour lier le 2ème fichier
    let attach_query = format!("ATTACH DATABASE '{}' AS store;", store_path.display());
    conn.execute(attach_query)?;

    // 4. Activer les performances (WAL + Foreign Keys)
    conn.execute("PRAGMA foreign_keys = ON;")?;
    conn.execute("PRAGMA journal_mode = WAL;")?;

    // Configurer le cache pour le store (très important pour les blobs)
    conn.execute("PRAGMA store.cache_size = -200000;")?; // ~200Mo cache

    Ok(conn)
}
