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
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
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
        Some("verify") => {
            let Some(file_arg) = args.next() else {
                eprintln!("usage: posthaze-keytool verify <file> <sig_file> <public_key_b64>...");
                return ExitCode::FAILURE;
            };
            let Some(sig_arg) = args.next() else {
                eprintln!("usage: posthaze-keytool verify <file> <sig_file> <public_key_b64>...");
                return ExitCode::FAILURE;
            };
            let pub_keys: Vec<String> = args.collect();
            if pub_keys.is_empty() {
                eprintln!("usage: posthaze-keytool verify <file> <sig_file> <public_key_b64>...");
                return ExitCode::FAILURE;
            }
            if let Err(err) = verify_file(&PathBuf::from(&file_arg), &PathBuf::from(&sig_arg), &pub_keys)
            {
                eprintln!("error: {err}");
                return ExitCode::FAILURE;
            }
            println!("verified signature for {file_arg}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("usage: posthaze-keytool <gen | sign <file> | verify <file> <sig_file> <public_key_b64>...>");
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

fn verify_file(
    file_path: &PathBuf,
    sig_path: &PathBuf,
    public_keys_b64: &[String],
) -> Result<(), String> {
    let payload = fs::read(file_path)
        .map_err(|err| format!("failed to read {}: {err}", file_path.display()))?;
    let sig_text = fs::read_to_string(sig_path)
        .map_err(|err| format!("failed to read {}: {err}", sig_path.display()))?;
    let sig_bytes = BASE64
        .decode(sig_text.trim())
        .map_err(|err| format!("invalid base64 signature: {err}"))?;
    let sig_array: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| "Ed25519 signature must decode to 64 bytes".to_string())?;
    let signature = Signature::from_bytes(&sig_array);

    let mut keys = Vec::with_capacity(public_keys_b64.len());
    for raw in public_keys_b64 {
        let key_bytes = BASE64
            .decode(raw.trim())
            .map_err(|err| format!("invalid base64 public key: {err}"))?;
        let key_array: [u8; 32] = key_bytes
            .try_into()
            .map_err(|_| "Ed25519 public key must decode to 32 bytes".to_string())?;
        let vk = VerifyingKey::from_bytes(&key_array)
            .map_err(|err| format!("invalid Ed25519 public key: {err}"))?;
        keys.push(vk);
    }

    if keys.iter().any(|vk| vk.verify(&payload, &signature).is_ok()) {
        Ok(())
    } else {
        Err(format!(
            "Ed25519 signature in {} did not match any provided public key",
            sig_path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn verify_file_accepts_matching_key_and_rejects_mismatch() {
        let dir = env::temp_dir().join(format!(
            "posthaze-keytool-test-{}",
            std::process::id()
        ));
        let _ = fs::create_dir_all(&dir);
        let file_path = dir.join("setup.exe");
        let sig_path = dir.join("setup.exe.sig");

        let sk = SigningKey::generate(&mut OsRng);
        let other_sk = SigningKey::generate(&mut OsRng);
        let payload = b"posthaze release binary";
        fs::write(&file_path, payload).unwrap();
        let sig_b64 = BASE64.encode(sk.sign(payload).to_bytes());
        fs::write(&sig_path, format!("{sig_b64}\n")).unwrap();

        let good_pub = BASE64.encode(sk.verifying_key().to_bytes());
        let wrong_pub = BASE64.encode(other_sk.verifying_key().to_bytes());

        assert!(verify_file(&file_path, &sig_path, &[wrong_pub.clone(), good_pub]).is_ok());
        assert!(verify_file(&file_path, &sig_path, &[wrong_pub]).is_err());

        let _ = fs::remove_dir_all(&dir);
    }
}
