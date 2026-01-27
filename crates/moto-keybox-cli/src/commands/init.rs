//! Init command - generates KEK and SVID signing key.
//!
//! This command generates the cryptographic keys needed to run a keybox server:
//! - Master key (KEK): AES-256 key for envelope encryption
//! - Signing key: Ed25519 private key for SVID issuance

use std::fs;
use std::path::PathBuf;

use clap::Args;
use moto_keybox::envelope::MasterKey;
use moto_keybox::svid::SvidIssuer;

use crate::error::{CliError, Result};

/// Generate KEK and SVID signing key for keybox server.
#[derive(Args, Debug)]
pub struct InitCommand {
    /// Output directory for generated keys
    #[arg(long, default_value = "./keybox-keys")]
    pub output_dir: PathBuf,

    /// Overwrite existing keys if present
    #[arg(long)]
    pub force: bool,
}

/// Run the init command.
pub fn run(cmd: &InitCommand) -> Result<()> {
    let output_dir = &cmd.output_dir;

    // Create output directory if it doesn't exist
    if !output_dir.exists() {
        fs::create_dir_all(output_dir)?;
        tracing::debug!("Created output directory: {}", output_dir.display());
    }

    let master_key_path = output_dir.join("master.key");
    let signing_key_path = output_dir.join("signing.key");

    // Check for existing keys
    if !cmd.force {
        if master_key_path.exists() {
            return Err(CliError::invalid_input(format!(
                "Master key already exists at {}. Use --force to overwrite.",
                master_key_path.display()
            )));
        }
        if signing_key_path.exists() {
            return Err(CliError::invalid_input(format!(
                "Signing key already exists at {}. Use --force to overwrite.",
                signing_key_path.display()
            )));
        }
    }

    // Generate master key (KEK)
    let master_key = MasterKey::generate();
    let master_key_encoded = master_key.encode();
    fs::write(&master_key_path, format!("{master_key_encoded}\n"))?;
    tracing::info!("Generated master key: {}", master_key_path.display());

    // Generate SVID signing key
    let signing_key = SvidIssuer::generate_key();
    let signing_key_encoded = SvidIssuer::encode_key(&signing_key);
    fs::write(&signing_key_path, format!("{signing_key_encoded}\n"))?;
    tracing::info!("Generated signing key: {}", signing_key_path.display());

    // Set restrictive permissions on key files (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&master_key_path, permissions.clone())?;
        fs::set_permissions(&signing_key_path, permissions)?;
        tracing::debug!("Set file permissions to 0600");
    }

    println!("Generated keybox keys in {}", output_dir.display());
    println!(
        "  - {} (KEK, AES-256, base64-encoded)",
        master_key_path.display()
    );
    println!(
        "  - {} (Ed25519 private key, base64-encoded)",
        signing_key_path.display()
    );
    println!();
    println!("Keep these files secure! The master key protects all secrets.");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn init_creates_keys() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("keys");

        let cmd = InitCommand {
            output_dir: output_dir.clone(),
            force: false,
        };

        run(&cmd).unwrap();

        // Verify files were created
        assert!(output_dir.join("master.key").exists());
        assert!(output_dir.join("signing.key").exists());

        // Verify master key is valid
        let master_key_content = fs::read_to_string(output_dir.join("master.key")).unwrap();
        MasterKey::from_base64(master_key_content.trim()).unwrap();

        // Verify signing key is valid
        let signing_key_content = fs::read_to_string(output_dir.join("signing.key")).unwrap();
        SvidIssuer::from_base64(signing_key_content.trim()).unwrap();
    }

    #[test]
    fn init_refuses_overwrite_without_force() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("keys");

        // First run should succeed
        let cmd = InitCommand {
            output_dir: output_dir.clone(),
            force: false,
        };
        run(&cmd).unwrap();

        // Second run should fail
        let cmd = InitCommand {
            output_dir,
            force: false,
        };
        let result = run(&cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("already exists"));
    }

    #[test]
    fn init_overwrites_with_force() {
        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("keys");

        // First run
        let cmd = InitCommand {
            output_dir: output_dir.clone(),
            force: false,
        };
        run(&cmd).unwrap();

        // Read original keys
        let original_master = fs::read_to_string(output_dir.join("master.key")).unwrap();
        let original_signing = fs::read_to_string(output_dir.join("signing.key")).unwrap();

        // Second run with force
        let cmd = InitCommand {
            output_dir: output_dir.clone(),
            force: true,
        };
        run(&cmd).unwrap();

        // Verify keys were regenerated (different content)
        let new_master = fs::read_to_string(output_dir.join("master.key")).unwrap();
        let new_signing = fs::read_to_string(output_dir.join("signing.key")).unwrap();

        assert_ne!(original_master, new_master);
        assert_ne!(original_signing, new_signing);
    }

    #[cfg(unix)]
    #[test]
    fn init_sets_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let output_dir = temp_dir.path().join("keys");

        let cmd = InitCommand {
            output_dir: output_dir.clone(),
            force: false,
        };
        run(&cmd).unwrap();

        let master_perms = fs::metadata(output_dir.join("master.key"))
            .unwrap()
            .permissions()
            .mode();
        let signing_perms = fs::metadata(output_dir.join("signing.key"))
            .unwrap()
            .permissions()
            .mode();

        // Check that only owner can read/write (mode 0600)
        assert_eq!(master_perms & 0o777, 0o600);
        assert_eq!(signing_perms & 0o777, 0o600);
    }
}
