//! Issue dev SVID command - issues long-lived SVIDs for local development.
//!
//! In K8s, pods authenticate via `ServiceAccount` JWT. For local development
//! without K8s, use this command to issue a dev SVID that can be used with
//! the keybox client library.

use std::fs;
use std::path::PathBuf;

use clap::Args;
use moto_keybox::svid::{SvidClaims, SvidIssuer};
use moto_keybox::types::SpiffeId;

use crate::error::{CliError, Result};

/// Dev SVID TTL in seconds (24 hours for dev convenience).
const DEV_SVID_TTL_SECS: i64 = 24 * 60 * 60;

/// Issue a dev SVID for local development.
///
/// This creates a long-lived (24h) SVID that can be used for local testing
/// without K8s. Set `MOTO_KEYBOX_SVID_FILE` to the output path for the client
/// library to use it.
#[derive(Args, Debug)]
pub struct IssueDevSvidCommand {
    /// Path to the signing key file (from `moto keybox init`)
    #[arg(
        long,
        default_value = "./keybox-keys/signing.key",
        env = "MOTO_KEYBOX_SVID_SIGNING_KEY_FILE"
    )]
    pub signing_key: PathBuf,

    /// Garage ID to issue the SVID for
    #[arg(long, conflicts_with = "bike_id", conflicts_with = "service_name")]
    pub garage_id: Option<String>,

    /// Bike ID to issue the SVID for
    #[arg(long, conflicts_with = "garage_id", conflicts_with = "service_name")]
    pub bike_id: Option<String>,

    /// Service name to issue the SVID for
    #[arg(long, conflicts_with = "garage_id", conflicts_with = "bike_id")]
    pub service_name: Option<String>,

    /// Output file path for the SVID JWT
    #[arg(long, short = 'o', default_value = "./dev-svid.jwt")]
    pub output: PathBuf,

    /// Overwrite existing output file
    #[arg(long)]
    pub force: bool,
}

/// Run the issue-dev-svid command.
pub fn run(cmd: &IssueDevSvidCommand) -> Result<()> {
    // Determine the SPIFFE ID from the provided identity
    let spiffe_id = match (&cmd.garage_id, &cmd.bike_id, &cmd.service_name) {
        (Some(id), None, None) => SpiffeId::garage(id),
        (None, Some(id), None) => SpiffeId::bike(id),
        (None, None, Some(name)) => SpiffeId::service(name),
        _ => {
            return Err(CliError::invalid_input(
                "Must specify exactly one of --garage-id, --bike-id, or --service-name",
            ));
        }
    };

    // Check if output file exists
    if cmd.output.exists() && !cmd.force {
        return Err(CliError::invalid_input(format!(
            "Output file already exists at {}. Use --force to overwrite.",
            cmd.output.display()
        )));
    }

    // Load the signing key
    let issuer = SvidIssuer::from_file(&cmd.signing_key).map_err(|e| {
        CliError::general(format!(
            "Failed to load signing key from {}: {}",
            cmd.signing_key.display(),
            e
        ))
    })?;

    // Create claims with 24h TTL
    let claims = SvidClaims::new(&spiffe_id, DEV_SVID_TTL_SECS);

    // Issue the SVID
    let issuer = issuer.with_ttl(DEV_SVID_TTL_SECS);
    let token = issuer
        .issue_with_claims(&claims)
        .map_err(|e| CliError::general(format!("Failed to issue SVID: {e}")))?;

    // Create parent directories if needed
    if let Some(parent) = cmd.output.parent()
        && !parent.exists()
    {
        fs::create_dir_all(parent)?;
        tracing::debug!("Created output directory: {}", parent.display());
    }

    // Write the token to the output file
    fs::write(&cmd.output, format!("{token}\n"))?;
    tracing::info!("Wrote dev SVID to: {}", cmd.output.display());

    // Set restrictive permissions on the file (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(0o600);
        fs::set_permissions(&cmd.output, permissions)?;
        tracing::debug!("Set file permissions to 0600");
    }

    println!("Issued dev SVID for {}", spiffe_id.to_uri());
    println!("  Output: {}", cmd.output.display());
    println!("  TTL: 24 hours");
    println!();
    println!("Usage:");
    println!("  export MOTO_KEYBOX_SVID_FILE={}", cmd.output.display());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use moto_keybox::svid::SvidValidator;
    use tempfile::TempDir;

    fn setup_signing_key(dir: &std::path::Path) -> PathBuf {
        let signing_key = SvidIssuer::generate_key();
        let signing_key_path = dir.join("signing.key");
        let encoded = SvidIssuer::encode_key(&signing_key);
        fs::write(&signing_key_path, format!("{encoded}\n")).unwrap();
        signing_key_path
    }

    #[test]
    fn issue_garage_svid() {
        let temp_dir = TempDir::new().unwrap();
        let signing_key_path = setup_signing_key(temp_dir.path());
        let output_path = temp_dir.path().join("dev.jwt");

        let cmd = IssueDevSvidCommand {
            signing_key: signing_key_path.clone(),
            garage_id: Some("test-garage".to_string()),
            bike_id: None,
            service_name: None,
            output: output_path.clone(),
            force: false,
        };

        run(&cmd).unwrap();

        // Verify the output file exists
        assert!(output_path.exists());

        // Verify the token is valid
        let token = fs::read_to_string(&output_path).unwrap();
        let token = token.trim();

        // Load the signing key to get the verifying key
        let issuer = SvidIssuer::from_file(&signing_key_path).unwrap();
        let validator = SvidValidator::new(issuer.verifying_key());

        let claims = validator.validate(token).unwrap();
        assert_eq!(claims.sub, "spiffe://moto.local/garage/test-garage");
        assert_eq!(claims.principal_id, "test-garage");

        // Verify 24h TTL
        let ttl = claims.exp - claims.iat;
        assert_eq!(ttl, DEV_SVID_TTL_SECS);
    }

    #[test]
    fn issue_bike_svid() {
        let temp_dir = TempDir::new().unwrap();
        let signing_key_path = setup_signing_key(temp_dir.path());
        let output_path = temp_dir.path().join("dev.jwt");

        let cmd = IssueDevSvidCommand {
            signing_key: signing_key_path,
            garage_id: None,
            bike_id: Some("my-bike".to_string()),
            service_name: None,
            output: output_path.clone(),
            force: false,
        };

        run(&cmd).unwrap();

        let token = fs::read_to_string(&output_path).unwrap();
        assert!(!token.trim().is_empty());
    }

    #[test]
    fn issue_service_svid() {
        let temp_dir = TempDir::new().unwrap();
        let signing_key_path = setup_signing_key(temp_dir.path());
        let output_path = temp_dir.path().join("dev.jwt");

        let cmd = IssueDevSvidCommand {
            signing_key: signing_key_path,
            garage_id: None,
            bike_id: None,
            service_name: Some("ai-proxy".to_string()),
            output: output_path.clone(),
            force: false,
        };

        run(&cmd).unwrap();

        let token = fs::read_to_string(&output_path).unwrap();
        assert!(!token.trim().is_empty());
    }

    #[test]
    fn requires_identity() {
        let temp_dir = TempDir::new().unwrap();
        let signing_key_path = setup_signing_key(temp_dir.path());
        let output_path = temp_dir.path().join("dev.jwt");

        let cmd = IssueDevSvidCommand {
            signing_key: signing_key_path,
            garage_id: None,
            bike_id: None,
            service_name: None,
            output: output_path,
            force: false,
        };

        let result = run(&cmd);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .message
                .contains("Must specify exactly one")
        );
    }

    #[test]
    fn refuses_overwrite_without_force() {
        let temp_dir = TempDir::new().unwrap();
        let signing_key_path = setup_signing_key(temp_dir.path());
        let output_path = temp_dir.path().join("dev.jwt");

        // Create existing file
        fs::write(&output_path, "existing").unwrap();

        let cmd = IssueDevSvidCommand {
            signing_key: signing_key_path,
            garage_id: Some("test".to_string()),
            bike_id: None,
            service_name: None,
            output: output_path,
            force: false,
        };

        let result = run(&cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().message.contains("already exists"));
    }

    #[test]
    fn overwrites_with_force() {
        let temp_dir = TempDir::new().unwrap();
        let signing_key_path = setup_signing_key(temp_dir.path());
        let output_path = temp_dir.path().join("dev.jwt");

        // Create existing file
        fs::write(&output_path, "existing").unwrap();

        let cmd = IssueDevSvidCommand {
            signing_key: signing_key_path,
            garage_id: Some("test".to_string()),
            bike_id: None,
            service_name: None,
            output: output_path.clone(),
            force: true,
        };

        run(&cmd).unwrap();

        let content = fs::read_to_string(&output_path).unwrap();
        assert_ne!(content, "existing");
    }

    #[cfg(unix)]
    #[test]
    fn sets_restrictive_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let signing_key_path = setup_signing_key(temp_dir.path());
        let output_path = temp_dir.path().join("dev.jwt");

        let cmd = IssueDevSvidCommand {
            signing_key: signing_key_path,
            garage_id: Some("test".to_string()),
            bike_id: None,
            service_name: None,
            output: output_path.clone(),
            force: false,
        };

        run(&cmd).unwrap();

        let perms = fs::metadata(&output_path).unwrap().permissions().mode();
        assert_eq!(perms & 0o777, 0o600);
    }
}
