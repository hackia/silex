use chrono::{DateTime, Local};
use ignore::{DirEntry, WalkBuilder};
use std::collections::HashMap;
use std::fs::Metadata;
use std::os::unix::fs::PermissionsExt; // Nécessaire pour .mode()
use std::path::{Path, PathBuf};

struct TreeNode {
    path: PathBuf,
    entry: Option<DirEntry>,
    children: HashMap<String, TreeNode>,
}

impl TreeNode {
    fn new(path: PathBuf, entry: Option<DirEntry>) -> Self {
        Self {
            path,
            entry,
            children: HashMap::new(),
        }
    }

    fn add_child(&mut self, components: &[&std::ffi::OsStr], entry: DirEntry) {
        if let Some((first, rest)) = components.split_first() {
            let key = first.to_string_lossy().to_string();
            let node = self
                .children
                .entry(key)
                .or_insert_with(|| TreeNode::new(PathBuf::from(first), None));

            if rest.is_empty() {
                node.entry = Some(entry);
            } else {
                node.add_child(rest, entry);
            }
        }
    }
}

pub fn scan_and_print_tree(root_path: &Path) {
    // CORRECTION : "Permissions" remplit mieux la colonne que "Perms"
    println!(
        "\n{:<4} {:<12} {:<12} {:<20} Tree",
        "Type", "Permissions", "Size", "Modified"
    );
    println!();
    let walker = WalkBuilder::new(root_path)
        .hidden(false)
        .add_custom_ignore_filename("silexium")
        .standard_filters(true)
        .threads(4)
        .build();

    let mut root = TreeNode::new(root_path.to_path_buf(), None);

    for result in walker {
        match result {
            Ok(entry) => {
                let path = entry.path();
                if let Ok(relative) = path.strip_prefix(root_path) {
                    let components: Vec<_> = relative.iter().collect();
                    if !components.is_empty() {
                        root.add_child(&components, entry.clone());
                    }
                }
            }
            Err(err) => eprintln!("Erreur scan: {err}"),
        }
    }

    print_node(&root, "", true);
    println!();
}

fn print_node(node: &TreeNode, prefix: &str, is_last: bool) {
    if let Some(entry) = &node.entry {
        let metadata = entry.metadata().ok();
        let (mode, size, date) = extract_metadata(metadata.as_ref());

        let type_char = if let Some(ft) = entry.file_type() {
            if ft.is_dir() {
                "D"
            } else if ft.is_symlink() {
                "L"
            } else {
                "F"
            }
        } else {
            "?"
        };

        let connector = if is_last { "└──" } else { "├──" };
        let display_name = entry.file_name().to_string_lossy();

        println!(
            "{:<4} {:<12} {:<12} {:<20} {}{} {}",
            type_char, mode, size, date, prefix, connector, display_name
        );
    }

    let mut children: Vec<_> = node.children.values().collect();
    children.sort_by(|a, b| {
        let a_is_dir = a
            .entry
            .as_ref()
            .map(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .unwrap_or(false);
        let b_is_dir = b
            .entry
            .as_ref()
            .map(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .unwrap_or(false);

        if a_is_dir == b_is_dir {
            a.path.cmp(&b.path)
        } else {
            b_is_dir.cmp(&a_is_dir)
        }
    });

    for (i, child) in children.iter().enumerate() {
        let is_last_child = i == children.len() - 1;
        let child_prefix = if node.entry.is_none() {
            "".to_string()
        } else if is_last {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };

        print_node(child, &child_prefix, is_last_child);
    }
}

fn extract_metadata(meta: Option<&Metadata>) -> (String, String, String) {
    match meta {
        Some(m) => {
            let mode_val = m.permissions().mode();
            let mode_str = format_permissions(mode_val);

            let size = if m.is_dir() {
                "-".to_string()
            } else {
                human_bytes(m.len())
            };

            let date: DateTime<Local> = m.modified().unwrap_or(std::time::SystemTime::now()).into();
            let date_str = date.format("%Y-%m-%d %H:%M").to_string();

            (mode_str, size, date_str)
        }
        None => ("????".to_string(), "?".to_string(), "?".to_string()),
    }
}

fn format_permissions(mode: u32) -> String {
    let user = (mode >> 6) & 0o7;
    let group = (mode >> 3) & 0o7;
    let other = mode & 0o7;
    format!(
        "{}{}{}",
        fmt_triplet(user),
        fmt_triplet(group),
        fmt_triplet(other)
    )
}

fn fmt_triplet(val: u32) -> String {
    let r = if val & 4 != 0 { "r" } else { "-" };
    let w = if val & 2 != 0 { "w" } else { "-" };
    let x = if val & 1 != 0 { "x" } else { "-" };
    format!("{}{}{}", r, w, x)
}

fn human_bytes(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB"];
    let mut s = size as f64;
    let mut unit_idx = 0;
    while s >= 1024.0 && unit_idx < UNITS.len() - 1 {
        s /= 1024.0;
        unit_idx += 1;
    }
    format!("{:.1} {}", s, UNITS[unit_idx])
}
