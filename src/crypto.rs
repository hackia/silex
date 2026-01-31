use crate::utils::ok;
use ed25519_dalek::{Signature, Signer, SigningKey};
use rand::rngs::OsRng;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
pub fn generate_keypair(root_path: &Path) -> Result<(), String> {
    let identity_dir = root_path.join(".silex/identity");
    fs::create_dir_all(&identity_dir).expect("failed to create identity");

    let secret_path = identity_dir.join("secret.key");
    let public_path = identity_dir.join("public.key");

    if secret_path.exists() {
        return Err("Une identité existe déjà pour ce dépôt.".to_string());
    }

    // Génération cryptographique
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    // Sauvegarde
    let mut file = File::create(secret_path).expect("failed to create secret key");
    file.write_all(&signing_key.to_bytes())
        .map_err(|e| e.to_string())?;

    let mut file_pub = File::create(public_path).expect("failed to create public key");
    file_pub
        .write_all(verifying_key.as_bytes())
        .map_err(|e| e.to_string())?;

    ok("Clés cryptographiques générées dans .silex/identity/");
    Ok(())
}

// Signe un message (le hash du commit)
pub fn sign_message(root_path: &Path, message: &str) -> Result<String, String> {
    let secret_path = root_path.join(".silex/identity/secret.key");

    if !secret_path.exists() {
        return Err("Pas de clé privée trouvée. Lance 'sx keygen' d'abord.".to_string());
    }

    // 1. Lecture de la clé
    let mut file = File::open(secret_path).expect("failed to get secret key");
    let mut bytes = [0u8; 32];
    file.read_exact(&mut bytes).expect("failed to read key");

    let signing_key = SigningKey::from_bytes(&bytes);

    // 2. Signature
    let signature: Signature = signing_key.sign(message.as_bytes());

    // 3. Retourne la signature en Hexadécimal
    Ok(hex::encode(signature.to_bytes()))
}
