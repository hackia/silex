# Silex

**An Asset-Centric Version Control System powered by SQLite.**

> ⚠️ **Status:** Proof of Concept / Experimental.

Silex is a novel Version Control System (VCS) written in Rust. Unlike Git, which tracks snapshots of file system trees, Silex tracks **File Assets**. It leverages the relational power of SQLite to maintain the history, identity, and metadata of your project files.

## The Philosophy: Asset vs. Snapshot

The dominant philosophy in modern VCS (like Git) is "Content Addressable Storage". If you rename a file, Git sees a deletion and a creation, then guesses it's a rename based on content similarity.

**Silex takes a different approach: Identity.**

1. **File Identity (Assets):** Every file introduced to the system gets a unique UUID (`asset_id`). If you rename `main.rs` to `application.rs`, the `asset_id` remains the same. Rename tracking is explicit and native, not a heuristic.
2. **Flat Manifests:** Instead of recursive Merkle Trees (Git Trees), a commit in Silex is defined by a flat "Manifest" table. Getting the state of a project at `commit X` is a simple SQL `SELECT`.
3. **Relational Metadata:** Since the history is stored in SQLite, you can perform complex queries on your repository (e.g., *"Find all files larger than 1MB modified by user X between 2023 and 2024"*).

## Architecture & Schema

The core of Silex relies on a relational model stored in `.silex/db/silex.db`.

### 1. Blobs (Content)

Stores the raw binary content. De-duplicated via SHA-256 hashing.

```sql
CREATE TABLE blobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT UNIQUE NOT NULL,      -- SHA-256 of content
    content BLOB,                   -- Raw data (optionally compressed)
    size INTEGER NOT NULL,
    mime_type TEXT                  -- Metadata for quick UI rendering
);

```

### 2. Assets (Identity)

The persistent identity of a file. This ID never changes, even if the file path does.

```sql
CREATE TABLE assets (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    uuid TEXT UNIQUE NOT NULL,      -- Universal unique identifier
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    creator_id INTEGER
);

```

### 3. Commits (Events)

The history graph.

```sql
CREATE TABLE commits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT UNIQUE NOT NULL,
    parent_hash TEXT,               -- NULL for root commit
    author TEXT NOT NULL,
    message TEXT NOT NULL,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(parent_hash) REFERENCES commits(hash)
);

```

### 4. Manifest (The Link)

The pivot table that reconstructs the filesystem state. It links a Commit, an Asset (Identity), and a Blob (Content) to a specific File Path.

```sql
CREATE TABLE manifest (
    commit_id INTEGER NOT NULL,
    asset_id INTEGER NOT NULL,
    blob_id INTEGER NOT NULL,
    file_path TEXT NOT NULL,        -- Path can change between commits for the same asset
    permissions INTEGER DEFAULT 644,
    PRIMARY KEY (commit_id, asset_id)
);

```

## Getting Started

### Prerequisites

* Rust (latest stable)
* `libsqlite3` (usually pre-installed on most unix systems)

### Installation

```bash
git clone https://github.com/hackia/silex
cd silex
cargo build --release

```

### Usage

Initialize a new Silex repository:

```bash
# This creates a directory with a .silex/db/silex.db database
cargo run -- new

```

*Follow the interactive prompt to name your project.*

## Roadmap

* [x] **Init:** Database structure creation (`silex new`).
* [ ] **Stage:** Scanning working directory and identifying changed assets.
* [ ] **Commit:** Writing blobs and manifest entries.
* [ ] **Log:** Querying the `commits` table.
* [ ] **Checkout:** Reconstructing files from `blobs` based on `manifest`.

## License

This project is licensed under the **GNU Affero General Public License v3.0**. See the [LICENSE](https://www.google.com/search?q=LICENSE) file for details.
