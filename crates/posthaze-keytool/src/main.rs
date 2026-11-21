//! Minimal Ed25519 keygen and signing CLI for Posthaze release installers.
//!
//! Not shipped to end users (`publish = false`). Used locally to generate the
//! release signing keypair and in GitHub Actions to produce `<setup>.sig` files.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use ed25519_dalek::{Signer as _, SigningKey};
use rand_core::OsRng;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("gen") => {
            let signing_key = SigningKey::generate(&mut OsRng);
            let verifying_key = signing_key.verifying_key();
            let secret_b64 = BASE64.encode(signing_key.to_bytes());
            let public_b64 = BASE64.encode(verifying_key.to_bytes());
            println!("UPDATER_SIGNING_KEY={secret_b64}");
            println!("UPDATER_PUBLIC_KEY={public_b64}");
            ExitCode::SUCCESS
        }
        Some("sign") => {
            let Some(file_arg) = args.next() else {
                eprintln!("usage: posthaze-keytool sign <file>");
                return ExitCode::FAILURE;
            };
            if let Err(err) = sign_file(&PathBuf::from(file_arg)) {
                eprintln!("error: {err}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: posthaze-keytool <gen | sign <file>>");
            ExitCode::FAILURE
        }
    }
}

fn sign_file(path: &PathBuf) -> Result<(), String> {
    let key_b64 = env::var("UPDATER_SIGNING_KEY")
        .map_err(|_| "UPDATER_SIGNING_KEY environment variable is not set".to_string())?;
    let key_bytes = BASE64
        .decode(key_b64.trim())
        .map_err(|err| format!("invalid base64 in UPDATER_SIGNING_KEY: {err}"))?;
    let key_array: [u8; 32] = key_bytes
        .try_into()
        .map_err(|_| "UPDATER_SIGNING_KEY must decode to exactly 32 bytes".to_string())?;
    let signing_key = SigningKey::from_bytes(&key_array);

    let payload = fs::read(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let signature = signing_key.sign(&payload);
    let sig_b64 = BASE64.encode(signature.to_bytes());

    let sig_path = PathBuf::from(format!("{}.sig", path.display()));
    fs::write(&sig_path, format!("{sig_b64}\n"))
        .map_err(|err| format!("failed to write {}: {err}", sig_path.display()))?;
    println!("wrote {}", sig_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier as _};

    #[test]
    fn generated_keypair_signs_and_verifies_payload() {
        let sk = SigningKey::generate(&mut OsRng);
        let vk = sk.verifying_key();
        let payload = b"posthaze installer bytes";
        let sig = sk.sign(payload);
        let sig_b64 = BASE64.encode(sig.to_bytes());
        let decoded: [u8; 64] = BASE64.decode(sig_b64).unwrap().try_into().unwrap();
        let parsed = Signature::from_bytes(&decoded);
        assert!(vk.verify(payload, &parsed).is_ok());
        assert!(vk.verify(b"tampered", &parsed).is_err());
    }
}
