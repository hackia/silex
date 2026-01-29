pub fn ok(message: &str) {
    println!(
        "{}",
        format!("\x1b[1;32m *\x1b[1;37m {message}\x1b[0m").as_str()
    );
}
