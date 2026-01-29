use crate::vcs::FileStatus;

pub fn ok(message: &str) {
    println!(
        "{}",
        format!("\x1b[1;32m *\x1b[1;37m {message}\x1b[0m").as_str()
    );
}
pub fn ok_status(verb: &FileStatus) {
    match verb {
        FileStatus::New(p) => {
            println!(
                "{}",
                format!("\x1b[1;32m+\x1b[1;37m {}\x1b[0m", p.display())
            );
        }
        FileStatus::Modified(p, _) => {
            println!(
                "{}",
                format!("\x1b[1;33m~\x1b[1;37m {}\x1b[0m", p.display())
            );
        }
        FileStatus::Deleted(p, _) => {
            println!(
                "{}",
                format!("\x1b[1;31m-\x1b[1;37m {}\x1b[0m", p.display())
            );
        }
        _ => {}
    }
}
