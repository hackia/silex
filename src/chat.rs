use chrono::{Duration, Local, Timelike};
use sqlite::{Connection, Error};

pub fn send_ephemeral_message(conn: &Connection, sender: &str, content: &str) -> Result<(), Error> {
    let now = Local::now();
    // Logique : Si il est avant 20h, c'est aujourd'hui 20h. Sinon c'est demain 20h.
    let mut target = now
        .with_hour(20)
        .unwrap()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap();
    if now.hour() >= 20 {
        target += Duration::days(1);
    }

    let query = "INSERT INTO ephemeral_messages (sender, content, expires_at) VALUES (?, ?, ?)";
    let mut stmt = conn.prepare(query)?;
    stmt.bind((1, sender))?;
    stmt.bind((2, content))?;
    stmt.bind((3, target.to_rfc3339().as_str()))?;
    stmt.next()?;
    Ok(())
}

// À appeler au démarrage de `silex web` ou via une commande `silex gc`
pub fn cleanup_messages(conn: &Connection) -> Result<(), Error> {
    // Supprime tout ce qui a expiré
    conn.execute("DELETE FROM ephemeral_messages WHERE expires_at <= CURRENT_TIMESTAMP")?;
    Ok(())
}
