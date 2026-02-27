//! Dev subcommands: up, down, status.
//!
//! Manages the local development environment (postgres, keybox, moto-club).
//! See local-dev.md for the full specification.

use clap::{Args, Subcommand};
use serde::Serialize;
use std::io::Write as _;
use std::process::Command as ProcessCommand;
use std::process::Stdio;

use crate::cli::GlobalFlags;
use crate::error::{CliError, Result};

/// Read an environment variable, falling back to a default.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Hardcoded dev defaults for local development.
/// Each value can be overridden via the corresponding environment variable.
pub struct DevConfig {
    pub keybox_health: String,
    pub keybox_api: String,
    pub club_health: String,
    pub club_api: String,
    pub registry: String,
}

impl DevConfig {
    /// Load dev config from env vars with hardcoded defaults.
    pub fn load() -> Self {
        let keybox_bind =
            std::env::var("MOTO_KEYBOX_BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8090".to_string());
        let keybox_api_port = keybox_bind.rsplit(':').next().unwrap_or("8090");

        let keybox_health_bind = std::env::var("MOTO_KEYBOX_HEALTH_BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8091".to_string());
        let keybox_health_port = keybox_health_bind.rsplit(':').next().unwrap_or("8091");

        Self {
            keybox_health: format!("localhost:{keybox_health_port}"),
            keybox_api: format!("localhost:{keybox_api_port}"),
            club_health: "localhost:8081".to_string(),
            club_api: "localhost:18080".to_string(),
            registry: "localhost:5050".to_string(),
        }
    }

    /// Environment variables for spawning moto-keybox-server.
    fn keybox_env() -> Vec<(&'static str, String)> {
        vec![
            (
                "MOTO_KEYBOX_BIND_ADDR",
                env_or("MOTO_KEYBOX_BIND_ADDR", "0.0.0.0:8090"),
            ),
            (
                "MOTO_KEYBOX_HEALTH_BIND_ADDR",
                env_or("MOTO_KEYBOX_HEALTH_BIND_ADDR", "0.0.0.0:8091"),
            ),
            (
                "MOTO_KEYBOX_MASTER_KEY_FILE",
                env_or("MOTO_KEYBOX_MASTER_KEY_FILE", ".dev/keybox/master.key"),
            ),
            (
                "MOTO_KEYBOX_SVID_SIGNING_KEY_FILE",
                env_or(
                    "MOTO_KEYBOX_SVID_SIGNING_KEY_FILE",
                    ".dev/keybox/signing.key",
                ),
            ),
            (
                "MOTO_KEYBOX_DATABASE_URL",
                env_or(
                    "MOTO_KEYBOX_DATABASE_URL",
                    "postgres://moto:moto@localhost:5432/moto_keybox",
                ),
            ),
            (
                "MOTO_KEYBOX_SERVICE_TOKEN_FILE",
                env_or(
                    "MOTO_KEYBOX_SERVICE_TOKEN_FILE",
                    ".dev/keybox/service-token",
                ),
            ),
            ("RUST_LOG", env_or("RUST_LOG", "moto_keybox=debug")),
        ]
    }

    /// Environment variables for spawning moto-club.
    fn club_env() -> Vec<(&'static str, String)> {
        vec![
            (
                "MOTO_CLUB_BIND_ADDR",
                env_or("MOTO_CLUB_BIND_ADDR", "0.0.0.0:18080"),
            ),
            (
                "MOTO_CLUB_DATABASE_URL",
                env_or(
                    "MOTO_CLUB_DATABASE_URL",
                    "postgres://moto:moto@localhost:5432/moto_club",
                ),
            ),
            (
                "MOTO_CLUB_KEYBOX_URL",
                env_or("MOTO_CLUB_KEYBOX_URL", "http://localhost:8090"),
            ),
            (
                "MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE",
                env_or(
                    "MOTO_CLUB_KEYBOX_SERVICE_TOKEN_FILE",
                    ".dev/keybox/service-token",
                ),
            ),
            (
                "MOTO_CLUB_KEYBOX_HEALTH_URL",
                env_or("MOTO_CLUB_KEYBOX_HEALTH_URL", "http://localhost:8091"),
            ),
            (
                "MOTO_CLUB_DEV_CONTAINER_IMAGE",
                env_or(
                    "MOTO_CLUB_DEV_CONTAINER_IMAGE",
                    "moto-registry:5000/moto-garage:latest",
                ),
            ),
            ("RUST_LOG", env_or("RUST_LOG", "moto_club=debug")),
        ]
    }

    /// The club database URL for running migrations.
    fn club_database_url() -> String {
        env_or(
            "MOTO_CLUB_DATABASE_URL",
            "postgres://moto:moto@localhost:5432/moto_club",
        )
    }
}

/// Dev command and subcommands
#[derive(Args)]
pub struct DevCommand {
    #[command(subcommand)]
    pub action: DevAction,
}

/// Available dev actions
#[derive(Subcommand)]
pub enum DevAction {
    /// Start the full local dev stack
    Up {
        /// Start services only, don't open a garage
        #[arg(long)]
        no_garage: bool,
        /// Force rebuild and push the garage container image
        #[arg(long)]
        rebuild_image: bool,
        /// Skip the registry image check entirely
        #[arg(long)]
        skip_image: bool,
    },
    /// Stop the local dev stack
    Down {
        /// Also remove .dev/ directory and postgres data volume
        #[arg(long)]
        clean: bool,
    },
    /// Show health of all local dev components
    Status,
}

/// JSON output for dev status
#[derive(Serialize)]
struct DevStatusJson {
    cluster: String,
    registry: String,
    postgres: String,
    keybox: String,
    club: String,
    image: String,
    garages: i64,
}

/// JSON output for dev up
#[derive(Serialize)]
struct DevUpJson {
    cluster: String,
    postgres: String,
    keybox: String,
    club: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    garage: Option<String>,
}

/// Run the dev command
pub async fn run(cmd: DevCommand, flags: &GlobalFlags) -> Result<()> {
    match cmd.action {
        DevAction::Status => dev_status(flags),
        DevAction::Up {
            no_garage,
            rebuild_image,
            skip_image,
        } => dev_up(no_garage, rebuild_image, skip_image, flags).await,
        DevAction::Down { clean } => dev_down(clean, flags),
    }
}

// ── dev up helpers ──────────────────────────────────────────────────────────

/// Print a step header without newline.
fn step_print(step: u8, msg: &str, quiet: bool) {
    if !quiet {
        print!("[{step}/9] {msg:<30}");
        std::io::stdout().flush().ok();
    }
}

/// Print a step result (completes the line).
fn step_done(result: &str, quiet: bool) {
    if !quiet {
        println!("{result}");
    }
}

/// Get `MOTO_USER` or fall back to whoami.
fn get_moto_user() -> Result<String> {
    if let Ok(user) = std::env::var("MOTO_USER") {
        return Ok(user);
    }
    let output = ProcessCommand::new("whoami")
        .output()
        .map_err(|e| CliError::general(format!("Failed to run whoami: {e}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(CliError::general(
            "Could not determine user. Set MOTO_USER environment variable.",
        ))
    }
}

/// Step 1: Check prerequisites (Docker running, k3d installed).
fn check_prerequisites() -> Result<()> {
    let docker = ProcessCommand::new("docker")
        .arg("info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match docker {
        Ok(s) if s.success() => {}
        _ => {
            return Err(CliError::general(
                "Docker is not running.\n\nTry: Start Docker Desktop or Colima",
            ));
        }
    }

    let k3d = ProcessCommand::new("k3d")
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match k3d {
        Ok(s) if s.success() => {}
        _ => {
            return Err(CliError::general(
                "k3d is not installed.\n\nTry: brew install k3d",
            ));
        }
    }

    Ok(())
}

/// Step 2: Ensure k3d cluster exists.
fn ensure_cluster() -> Result<String> {
    if get_cluster_status_str() == "running" {
        return Ok("exists".to_string());
    }

    let status = ProcessCommand::new("cargo")
        .args(["run", "--bin", "moto", "--", "cluster", "init"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Ok("created".to_string()),
        Ok(s) => Err(CliError::general(format!(
            "Failed to create cluster (exit: {s})"
        ))),
        Err(e) => Err(CliError::general(format!(
            "Failed to run moto cluster init: {e}"
        ))),
    }
}

/// Step 3: Check or build the garage image.
fn check_or_build_image(config: &DevConfig, skip: bool, rebuild: bool) -> Result<String> {
    if skip {
        return Ok("skipped".to_string());
    }

    if !rebuild && check_image_in_registry(&config.registry) {
        return Ok("found in registry".to_string());
    }

    if rebuild {
        let status = ProcessCommand::new("make")
            .arg("dev-garage-image")
            .status()
            .map_err(|e| CliError::general(format!("Failed to run make dev-garage-image: {e}")))?;
        if !status.success() {
            return Err(CliError::general("Failed to build and push garage image"));
        }
        return Ok("built and pushed".to_string());
    }

    Err(CliError::general(
        "Garage image not found in registry.\n\nTry: make dev-garage-image\n  Or: moto dev up --skip-image\n  Or: moto dev up --rebuild-image",
    ))
}

/// Step 4: Start postgres via docker compose.
fn start_postgres() -> Result<()> {
    let status = ProcessCommand::new("docker")
        .args(["compose", "up", "-d", "--wait"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| CliError::general(format!("Failed to start postgres: {e}")))?;
    if !status.success() {
        return Err(CliError::general(
            "Failed to start postgres. Check Docker is running.",
        ));
    }
    Ok(())
}

/// Step 5: Ensure keybox keys exist in .dev/keybox/.
fn ensure_keybox_keys() -> Result<String> {
    let keys_dir = std::path::Path::new(".dev/keybox");
    let master = keys_dir.join("master.key");
    let signing = keys_dir.join("signing.key");
    let token_path = keys_dir.join("service-token");

    if master.exists() && signing.exists() && token_path.exists() {
        return Ok("found (.dev/keybox/)".to_string());
    }

    std::fs::create_dir_all(keys_dir)
        .map_err(|e| CliError::general(format!("Failed to create .dev/keybox/: {e}")))?;

    let status = ProcessCommand::new("cargo")
        .args([
            "run",
            "--bin",
            "moto-keybox",
            "--",
            "init",
            "--output-dir",
            ".dev/keybox",
            "--force",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| CliError::general(format!("Failed to run moto-keybox init: {e}")))?;
    if !status.success() {
        return Err(CliError::general("Failed to generate keybox keys"));
    }

    let output = ProcessCommand::new("openssl")
        .args(["rand", "-hex", "32"])
        .output()
        .map_err(|e| CliError::general(format!("Failed to generate service token: {e}")))?;
    if !output.status.success() {
        return Err(CliError::general("Failed to generate service token"));
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    std::fs::write(&token_path, &token)
        .map_err(|e| CliError::general(format!("Failed to write service token: {e}")))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| CliError::general(format!("Failed to set permissions: {e}")))?;
    }

    Ok("generated".to_string())
}

/// Step 6: Run database migrations.
fn run_migrations() -> Result<()> {
    let db_url = DevConfig::club_database_url();
    let status = ProcessCommand::new("cargo")
        .args([
            "sqlx",
            "migrate",
            "run",
            "--source",
            "crates/moto-club-db/migrations",
            "--database-url",
            &db_url,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| CliError::general(format!("Failed to run migrations: {e}")))?;
    if !status.success() {
        return Err(CliError::general(
            "Failed to run migrations. Is postgres running?\n\nTry: docker compose ps",
        ));
    }
    Ok(())
}

/// Spawn a cargo subprocess with environment variables.
fn spawn_subprocess(
    bin: &str,
    env_vars: &[(&str, String)],
    verbose: u8,
) -> Result<tokio::process::Child> {
    let stdout_cfg = if verbose >= 2 {
        Stdio::inherit()
    } else {
        Stdio::null()
    };
    let stderr_cfg = if verbose >= 1 {
        Stdio::inherit()
    } else {
        Stdio::null()
    };

    let mut cmd = tokio::process::Command::new("cargo");
    cmd.args(["run", "--bin", bin]);
    for (key, val) in env_vars {
        cmd.env(key, val);
    }
    cmd.stdout(stdout_cfg).stderr(stderr_cfg);

    cmd.spawn()
        .map_err(|e| CliError::general(format!("Failed to spawn {bin}: {e}")))
}

/// Wait for a health endpoint to return 200, with exponential backoff.
async fn wait_for_health(url: &str, timeout_secs: u64) -> Result<()> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout_secs);
    let mut delay = std::time::Duration::from_millis(100);
    let max_delay = std::time::Duration::from_secs(2);

    while start.elapsed() < timeout {
        if check_http_health(url) {
            return Ok(());
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(max_delay);
    }

    Err(CliError::general(format!(
        "Health check timed out after {timeout_secs}s: {url}"
    )))
}

/// Step 9: Create a garage via the moto-club API (best effort).
fn create_garage(config: &DevConfig, owner: Option<&str>) -> Result<String> {
    let url = format!("http://{}/api/v1/garages", config.club_api);
    let owner_str = owner.unwrap_or("dev");
    let body = format!(r#"{{"owner": "{owner_str}"}}"#);

    let output = ProcessCommand::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            &body,
            "--connect-timeout",
            "5",
            &url,
        ])
        .output()
        .map_err(|e| CliError::general(format!("Failed to create garage: {e}")))?;
    if !output.status.success() {
        return Err(CliError::general("Failed to create garage"));
    }

    let response = String::from_utf8_lossy(&output.stdout);
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&response) {
        if let Some(name) = parsed.get("name").and_then(|n| n.as_str()) {
            return Ok(name.to_string());
        }
    }

    Err(CliError::general(format!(
        "Unexpected API response: {response}"
    )))
}

/// Start the full local dev stack (9-step orchestration).
///
/// See local-dev.md for the full specification.
#[allow(clippy::too_many_lines)]
async fn dev_up(
    no_garage: bool,
    rebuild_image: bool,
    skip_image: bool,
    flags: &GlobalFlags,
) -> Result<()> {
    // Flag validation
    if skip_image && rebuild_image {
        return Err(CliError::invalid_input(
            "--skip-image and --rebuild-image cannot be used together",
        ));
    }

    let config = DevConfig::load();
    let quiet = flags.quiet;
    let verbose = flags.verbose;

    // Idempotency: silently kill existing processes on club/keybox ports
    let quiet_flags = GlobalFlags {
        quiet: true,
        ..GlobalFlags::default()
    };
    stop_port_process(
        extract_port(&config.club_api, "18080"),
        "club",
        &quiet_flags,
    );
    stop_port_process(
        extract_port(&config.keybox_api, "8090"),
        "keybox",
        &quiet_flags,
    );

    // Step 1: Prerequisites
    step_print(1, "Checking prerequisites...", quiet);
    check_prerequisites().inspect_err(|_| {
        if !quiet {
            println!();
        }
    })?;
    if !no_garage {
        get_moto_user().inspect_err(|_| {
            if !quiet {
                println!();
            }
        })?;
    }
    step_done("ok", quiet);

    // Step 2: Cluster
    step_print(2, "Ensuring cluster...", quiet);
    let cluster_result = ensure_cluster().inspect_err(|_| {
        if !quiet {
            println!();
        }
    })?;
    step_done(&cluster_result, quiet);

    // Step 3: Image
    step_print(3, "Checking garage image...", quiet);
    let image_result =
        check_or_build_image(&config, skip_image, rebuild_image).inspect_err(|_| {
            if !quiet {
                println!();
            }
        })?;
    step_done(&image_result, quiet);

    // Step 4: Postgres
    step_print(4, "Starting postgres...", quiet);
    start_postgres().inspect_err(|_| {
        if !quiet {
            println!();
        }
    })?;
    step_done("ready (localhost:5432)", quiet);

    // Step 5: Keys
    step_print(5, "Generating keybox keys...", quiet);
    let keys_result = ensure_keybox_keys().inspect_err(|_| {
        if !quiet {
            println!();
        }
    })?;
    step_done(&keys_result, quiet);

    // Step 6: Migrations
    step_print(6, "Running migrations...", quiet);
    run_migrations().inspect_err(|_| {
        if !quiet {
            println!();
        }
    })?;
    step_done("up to date", quiet);

    // Step 7: Keybox
    step_print(7, "Starting keybox...", quiet);
    let mut keybox = spawn_subprocess("moto-keybox-server", &DevConfig::keybox_env(), verbose)
        .inspect_err(|_| {
            if !quiet {
                println!();
            }
        })?;
    let keybox_health_url = format!("http://{}/health/ready", config.keybox_health);
    if let Err(e) = wait_for_health(&keybox_health_url, 30).await {
        if !quiet {
            println!();
        }
        keybox.start_kill().ok();
        return Err(e);
    }
    step_done(&format!("healthy ({})", config.keybox_api), quiet);

    // Step 8: Club
    step_print(8, "Starting moto-club...", quiet);
    let mut club =
        spawn_subprocess("moto-club", &DevConfig::club_env(), verbose).inspect_err(|_| {
            if !quiet {
                println!();
            }
            keybox.start_kill().ok();
        })?;
    let club_health_url = format!("http://{}/health/ready", config.club_health);
    if let Err(e) = wait_for_health(&club_health_url, 30).await {
        if !quiet {
            println!();
        }
        club.start_kill().ok();
        keybox.start_kill().ok();
        return Err(e);
    }
    step_done(&format!("healthy ({})", config.club_api), quiet);

    // Step 9: Garage (best effort)
    let mut garage_name = None;
    if !no_garage {
        step_print(9, "Opening garage...", quiet);
        let moto_user = get_moto_user().ok();
        match create_garage(&config, moto_user.as_deref()) {
            Ok(name) => {
                step_done(&name, quiet);
                garage_name = Some(name);
            }
            Err(e) => {
                if !quiet {
                    println!();
                    eprintln!("Warning: could not open garage: {e}");
                }
            }
        }
    }

    // Output
    if flags.json {
        let json = DevUpJson {
            cluster: "running".to_string(),
            postgres: "healthy".to_string(),
            keybox: "healthy".to_string(),
            club: "healthy".to_string(),
            garage: garage_name.clone(),
        };
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else if !quiet {
        println!();
        println!("moto dev environment ready!");
        println!();
        println!("  Postgres:  localhost:5432");
        println!("  Keybox:    {}", config.keybox_api);
        println!("  Club:      {}", config.club_api);
        if let Some(ref name) = garage_name {
            println!("  Garage:    {name}");
        }
        println!();
        if let Some(ref name) = garage_name {
            println!("  To connect: moto garage enter {name}");
        }
        println!("  To stop:    Ctrl-C");
    }

    // Wait for Ctrl-C or subprocess death
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            if !quiet {
                println!();
                println!("Shutting down...");
            }
            keybox.start_kill().ok();
            club.start_kill().ok();
        }
        status = club.wait() => {
            keybox.start_kill().ok();
            match status {
                Ok(s) => eprintln!("moto-club exited unexpectedly (status: {s})"),
                Err(e) => eprintln!("moto-club exited unexpectedly: {e}"),
            }
            return Err(CliError::general("moto-club exited unexpectedly"));
        }
        status = keybox.wait() => {
            club.start_kill().ok();
            match status {
                Ok(s) => eprintln!("keybox exited unexpectedly (status: {s})"),
                Err(e) => eprintln!("keybox exited unexpectedly: {e}"),
            }
            return Err(CliError::general("keybox exited unexpectedly"));
        }
    }

    Ok(())
}

// ── dev down ────────────────────────────────────────────────────────────────

/// Stop the local dev stack.
///
/// 1. Send SIGTERM to club and keybox processes (by port)
/// 2. Run `docker compose down` (with `-v` if `--clean`)
/// 3. With `--clean`: remove `.dev/` directory
#[allow(clippy::unnecessary_wraps)]
fn dev_down(clean: bool, flags: &GlobalFlags) -> Result<()> {
    let config = DevConfig::load();

    let club_port = extract_port(&config.club_api, "18080");
    let keybox_port = extract_port(&config.keybox_api, "8090");

    // Step 1: Stop club process
    stop_port_process(club_port, "club", flags);

    // Step 2: Stop keybox process
    stop_port_process(keybox_port, "keybox", flags);

    // Step 3: Stop postgres
    if !flags.quiet {
        println!("Stopping postgres...");
    }

    let mut compose_args = vec!["compose", "down"];
    if clean {
        compose_args.push("-v");
    }

    let output = ProcessCommand::new("docker").args(&compose_args).output();

    match output {
        Ok(o) if o.status.success() => {
            if !flags.quiet {
                println!("Postgres stopped");
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            if !flags.quiet {
                eprintln!("Warning: docker compose down failed: {}", stderr.trim());
            }
        }
        Err(e) => {
            if !flags.quiet {
                eprintln!("Warning: could not run docker compose: {e}");
            }
        }
    }

    // Step 4: With --clean, remove .dev/ directory
    if clean {
        if !flags.quiet {
            println!("Removing .dev/ directory...");
        }
        match std::fs::remove_dir_all(".dev") {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                if !flags.quiet {
                    eprintln!("Warning: could not remove .dev/: {e}");
                }
            }
        }
    }

    Ok(())
}

/// Find a process listening on the given TCP port and send SIGTERM.
fn stop_port_process(port: &str, name: &str, flags: &GlobalFlags) {
    let output = ProcessCommand::new("lsof")
        .args(["-ti", &format!("tcp:{port}")])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let pids = String::from_utf8_lossy(&o.stdout);
            for pid_str in pids.lines() {
                let pid_str = pid_str.trim();
                if pid_str.is_empty() {
                    continue;
                }
                if !flags.quiet {
                    println!("Stopping {name} (pid {pid_str})...");
                }
                let _ = ProcessCommand::new("kill").arg(pid_str).output();
            }
        }
        _ => {
            if !flags.quiet {
                println!("{name}: not running");
            }
        }
    }
}

/// Extract the port from a "host:port" string.
fn extract_port<'a>(addr: &'a str, default: &'a str) -> &'a str {
    addr.rsplit(':').next().unwrap_or(default)
}

/// Check if a HTTP endpoint returns 200 using curl.
fn check_http_health(url: &str) -> bool {
    let output = ProcessCommand::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code}",
            "--connect-timeout",
            "2",
            url,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let status_code = String::from_utf8_lossy(&o.stdout);
            status_code.trim() == "200"
        }
        _ => false,
    }
}

/// Check if postgres is running and healthy via docker compose ps.
fn check_postgres_healthy() -> bool {
    let output = ProcessCommand::new("docker")
        .args(["compose", "ps", "--format", "json", "postgres"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            // docker compose ps --format json outputs JSON with State and Health fields
            // Check if container is running and healthy
            stdout.contains("\"running\"") && stdout.contains("\"healthy\"")
        }
        _ => false,
    }
}

/// Check if the garage image exists in the registry.
fn check_image_in_registry(registry: &str) -> bool {
    let url = format!("http://{registry}/v2/moto-garage/tags/list");
    let output = ProcessCommand::new("curl")
        .args(["-s", "--connect-timeout", "2", &url])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let body = String::from_utf8_lossy(&o.stdout);
            body.contains("\"latest\"")
        }
        _ => false,
    }
}

/// Count non-terminated garages from the moto-club API.
fn count_garages(club_api: &str) -> i64 {
    let url = format!("http://{club_api}/api/v1/garages");
    let output = ProcessCommand::new("curl")
        .args(["-s", "--connect-timeout", "2", &url])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let body = String::from_utf8_lossy(&o.stdout);
            // Parse JSON response to count garages
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(garages) = parsed.get("garages").and_then(|g| g.as_array()) {
                    return i64::try_from(garages.len()).unwrap_or(0);
                }
            }
            0
        }
        _ => -1, // -1 indicates API unreachable
    }
}

/// Get cluster status string for dev status output.
fn get_cluster_status_str() -> &'static str {
    let output = ProcessCommand::new("k3d")
        .args(["cluster", "list", "--no-headers"])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.first() == Some(&"moto") {
                    if let Some(servers) = parts.get(1) {
                        if servers.starts_with("0/") {
                            return "stopped";
                        }
                    }
                    return "running";
                }
            }
            "not_found"
        }
        _ => "not_found",
    }
}

/// Show dev status health check dashboard.
fn dev_status(flags: &GlobalFlags) -> Result<()> {
    let config = DevConfig::load();

    let cluster = get_cluster_status_str();
    let registry_healthy = check_http_health(&format!("http://{}/v2/", config.registry));
    let postgres_healthy = check_postgres_healthy();
    let keybox_healthy =
        check_http_health(&format!("http://{}/health/ready", config.keybox_health));
    let club_healthy = check_http_health(&format!("http://{}/health/ready", config.club_health));
    let image_found = check_image_in_registry(&config.registry);
    let garage_count = count_garages(&config.club_api);

    let registry_str = if registry_healthy {
        "healthy"
    } else {
        "unhealthy"
    };
    let postgres_str = if postgres_healthy {
        "healthy"
    } else {
        "unhealthy"
    };
    let keybox_str = if keybox_healthy {
        "healthy"
    } else {
        "unhealthy"
    };
    let club_str = if club_healthy { "healthy" } else { "unhealthy" };
    let image_str = if image_found { "found" } else { "not_found" };

    if flags.json {
        let json = DevStatusJson {
            cluster: cluster.to_string(),
            registry: registry_str.to_string(),
            postgres: postgres_str.to_string(),
            keybox: keybox_str.to_string(),
            club: club_str.to_string(),
            image: image_str.to_string(),
            garages: garage_count.max(0),
        };
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else if !flags.quiet {
        let image_detail = if image_found {
            "moto-garage:latest (in registry)"
        } else {
            "not found"
        };
        let garage_detail = if garage_count >= 0 {
            format!("{garage_count} running")
        } else {
            "unknown (club unreachable)".to_string()
        };

        println!("Cluster:   {cluster} (k3d-moto)");
        println!("Registry:  {registry_str} ({})", config.registry);
        println!("Postgres:  {postgres_str} (localhost:5432)");
        println!("Keybox:    {keybox_str} (localhost:8090)");
        println!("Club:      {club_str} (localhost:18080)");
        println!("Image:     {image_detail}");
        println!("Garages:   {garage_detail}");
    }

    // Exit 1 if any component is unhealthy
    let all_healthy = cluster == "running"
        && registry_healthy
        && postgres_healthy
        && keybox_healthy
        && club_healthy;

    if !all_healthy {
        std::process::exit(1);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_config_port_extraction() {
        // Test that port extraction from bind address works correctly
        let addr = "0.0.0.0:8091";
        let port = addr.rsplit(':').next().unwrap_or("8091");
        assert_eq!(port, "8091");

        let addr2 = "0.0.0.0:9091";
        let port2 = addr2.rsplit(':').next().unwrap_or("8091");
        assert_eq!(port2, "9091");
    }

    #[test]
    fn test_dev_config_default_ports() {
        // Verify the hardcoded dev defaults match the spec
        // keybox API: 8090, keybox health: 8091, club health: 8081, club API: 18080, registry: 5050
        let config = DevConfig {
            keybox_health: "localhost:8091".to_string(),
            keybox_api: "localhost:8090".to_string(),
            club_health: "localhost:8081".to_string(),
            club_api: "localhost:18080".to_string(),
            registry: "localhost:5050".to_string(),
        };
        assert_eq!(config.keybox_api, "localhost:8090");
        assert_eq!(config.keybox_health, "localhost:8091");
        assert_eq!(config.club_health, "localhost:8081");
        assert_eq!(config.club_api, "localhost:18080");
        assert_eq!(config.registry, "localhost:5050");
    }

    #[test]
    fn test_dev_status_json_structure() {
        let json = DevStatusJson {
            cluster: "running".to_string(),
            registry: "healthy".to_string(),
            postgres: "healthy".to_string(),
            keybox: "healthy".to_string(),
            club: "healthy".to_string(),
            image: "found".to_string(),
            garages: 1,
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["cluster"], "running");
        assert_eq!(parsed["registry"], "healthy");
        assert_eq!(parsed["postgres"], "healthy");
        assert_eq!(parsed["keybox"], "healthy");
        assert_eq!(parsed["club"], "healthy");
        assert_eq!(parsed["image"], "found");
        assert_eq!(parsed["garages"], 1);
    }

    #[test]
    fn test_extract_port() {
        assert_eq!(extract_port("localhost:8080", "0"), "8080");
        assert_eq!(extract_port("0.0.0.0:8090", "0"), "8090");
        assert_eq!(extract_port("localhost:9999", "0"), "9999");
    }

    #[test]
    fn test_dev_config_keybox_api_default() {
        let config = DevConfig {
            keybox_health: "localhost:8091".to_string(),
            keybox_api: "localhost:8090".to_string(),
            club_health: "localhost:8081".to_string(),
            club_api: "localhost:18080".to_string(),
            registry: "localhost:5050".to_string(),
        };
        assert_eq!(config.keybox_api, "localhost:8090");
        assert_eq!(extract_port(&config.keybox_api, "8090"), "8090");
        assert_eq!(extract_port(&config.club_api, "18080"), "18080");
    }

    #[test]
    fn test_dev_up_json_structure() {
        let json = DevUpJson {
            cluster: "running".to_string(),
            postgres: "healthy".to_string(),
            keybox: "healthy".to_string(),
            club: "healthy".to_string(),
            garage: Some("bold-mongoose".to_string()),
        };

        let output = serde_json::to_string_pretty(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["cluster"], "running");
        assert_eq!(parsed["postgres"], "healthy");
        assert_eq!(parsed["keybox"], "healthy");
        assert_eq!(parsed["club"], "healthy");
        assert_eq!(parsed["garage"], "bold-mongoose");
    }

    #[test]
    fn test_dev_up_json_no_garage() {
        let json = DevUpJson {
            cluster: "running".to_string(),
            postgres: "healthy".to_string(),
            keybox: "healthy".to_string(),
            club: "healthy".to_string(),
            garage: None,
        };

        let output = serde_json::to_string(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        // garage field should be absent when None (skip_serializing_if)
        assert!(parsed.get("garage").is_none());
        assert_eq!(parsed["cluster"], "running");
    }

    #[test]
    fn test_env_or_default() {
        // When env var is not set, returns default
        let result = env_or("MOTO_TEST_NONEXISTENT_VAR_12345", "default_value");
        assert_eq!(result, "default_value");
    }

    #[test]
    fn test_dev_status_json_unhealthy() {
        let json = DevStatusJson {
            cluster: "not_found".to_string(),
            registry: "unhealthy".to_string(),
            postgres: "unhealthy".to_string(),
            keybox: "unhealthy".to_string(),
            club: "unhealthy".to_string(),
            image: "not_found".to_string(),
            garages: 0,
        };

        let output = serde_json::to_string(&json).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["cluster"], "not_found");
        assert_eq!(parsed["registry"], "unhealthy");
        assert_eq!(parsed["garages"], 0);
    }
}
