//! Garage subcommands: list, open, close, logs, enter, extend, watch.
//!
//! The CLI talks directly to moto-club API for all garage operations.
//! Local mode (direct K8s access) is deprecated per project-structure.md v1.2.

use clap::{Args, Subcommand};
use futures_util::StreamExt;
use moto_cli_wgtunnel::{
    ClientError, ConsoleProgress, CreateGarageRequest, EnterConfig, EnterError, GarageEvent,
    MotoClubClient, MotoClubConfig, TunnelManager, enter_garage,
};
use moto_k8s::{K8sClient, PodLogOptions, PodOps};
use serde::Serialize;
use std::io::{self, Write};

use crate::cli::GlobalFlags;
use crate::error::{CliError, Result};

/// Garage command and subcommands
#[derive(Args)]
pub struct GarageCommand {
    #[command(subcommand)]
    pub action: GarageAction,
}

/// Available garage actions
#[derive(Subcommand)]
pub enum GarageAction {
    /// List all garages
    List,

    /// Open a new garage
    Open {
        /// Human-friendly name (auto-generated if omitted)
        #[arg(short, long)]
        name: Option<String>,

        /// Owner of the garage (defaults to current user)
        #[arg(short, long)]
        owner: Option<String>,

        /// Git branch to work on (default: current branch)
        #[arg(short, long)]
        branch: Option<String>,

        /// Time-to-live (max: 48h). Format: <number><unit> where unit is m, h, or d.
        /// Default: 4h or config file setting.
        #[arg(long)]
        ttl: Option<String>,

        /// Override dev container image
        #[arg(short, long)]
        image: Option<String>,

        /// Engine to work on (default: current directory name)
        #[arg(short, long)]
        engine: Option<String>,

        /// Include `PostgreSQL` database (postgres:16)
        #[arg(long)]
        with_postgres: bool,

        /// Include Redis cache (redis:7)
        #[arg(long)]
        with_redis: bool,

        /// Create garage but don't connect to it
        #[arg(long)]
        no_attach: bool,

        /// Connect via kubectl exec instead of `WireGuard` tunnel
        #[arg(long)]
        kubectl: bool,
    },

    /// Connect to a garage terminal session
    Enter {
        /// Name of the garage to enter
        name: String,

        /// Connect via kubectl exec instead of `WireGuard` tunnel
        #[arg(long)]
        kubectl: bool,
    },

    /// Close an existing garage
    Close {
        /// Name of the garage to close
        name: String,

        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },

    /// View logs from a garage
    Logs {
        /// Name of the garage
        name: String,

        /// Stream logs continuously (Ctrl+C to stop)
        #[arg(long, short = 'f')]
        follow: bool,

        /// Show last n lines (default: 100)
        #[arg(long, short = 'n', default_value = "100")]
        tail: i64,

        /// Show logs from last duration (e.g., 5m, 1h)
        #[arg(long)]
        since: Option<String>,
    },

    /// Extend a garage's TTL
    Extend {
        /// Name of the garage to extend
        name: String,

        /// Time to add to current TTL (e.g., 4h, 30m)
        #[arg(long, default_value = "4h")]
        ttl: String,
    },

    /// Watch garage events in real time
    Watch {
        /// Comma-separated garage names to watch (default: all owned)
        #[arg(long, value_delimiter = ',')]
        garages: Option<Vec<String>>,
    },
}

/// JSON output for garage list
#[derive(Serialize)]
struct GarageListJson {
    garages: Vec<GarageJson>,
}

/// JSON representation of a garage (matches spec v0.4)
#[derive(Serialize)]
struct GarageJson {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<String>,
    branch: String,
    status: String,
    ttl_remaining_seconds: i64,
    age_seconds: i64,
}

/// JSON output for garage open (matches spec v0.4)
#[derive(Serialize)]
struct GarageOpenJson {
    id: String,
    name: String,
    branch: String,
    ttl_seconds: i64,
    expires_at: String,
    status: String,
}

/// JSON output for garage close
#[derive(Serialize)]
struct GarageCloseJson {
    name: String,
    status: String,
}

/// JSON output for garage logs (reserved for future `garage logs --json`)
#[allow(dead_code)]
#[derive(Serialize)]
struct GarageLogsJson {
    name: String,
    logs: String,
}

/// JSON output for garage enter
#[derive(Serialize)]
struct GarageEnterJson {
    name: String,
    session_id: String,
    client_ip: String,
    garage_ip: String,
    path_type: String,
    path_detail: String,
}

/// JSON output for garage extend
#[derive(Serialize)]
struct GarageExtendJson {
    name: String,
    expires_at: String,
    ttl_remaining_seconds: i64,
}

/// Create a moto-club client from configuration.
///
/// Owner precedence: `--owner` flag > `MOTO_USER` env var > config file `user` > error.
fn create_client(flags: &GlobalFlags, owner_override: Option<&str>) -> Result<MotoClubClient> {
    // Get base URL from config or environment
    let base_url =
        std::env::var("MOTO_CLUB_URL").unwrap_or_else(|_| "http://localhost:18080".to_string());

    // Owner: CLI flag > MOTO_USER env var > config file user > error
    let owner = if let Some(o) = owner_override {
        o.to_string()
    } else if let Ok(u) = std::env::var("MOTO_USER") {
        u
    } else if let Some(u) = flags.config.user.as_deref() {
        u.to_string()
    } else {
        return Err(CliError::invalid_input(
            "No user identity configured.\n\n\
             Set one of the following (in precedence order):\n\
             1. --owner flag on garage open\n\
             2. MOTO_USER environment variable\n\
             3. user field in ~/.config/moto/config.toml",
        ));
    };

    let config = MotoClubConfig::new(base_url, owner);
    MotoClubClient::new(config)
        .map_err(|e| CliError::general(format!("failed to create client: {e}")))
}

/// Convert client error to CLI error.
fn client_error_to_cli_error(e: ClientError) -> CliError {
    match e {
        ClientError::GarageNotFound(msg) => CliError::not_found(msg),
        ClientError::NotAuthorized(msg) => CliError::general(format!("not authorized: {msg}")),
        ClientError::Unreachable { url, reason } => CliError::general(format!(
            "moto-club unreachable at {url}: {reason}\n\n\
             Make sure moto-club is running.\n\
             Try: moto cluster status"
        )),
        ClientError::Server { code, message } => CliError::general(format!("{code}: {message}")),
        _ => CliError::general(e.to_string()),
    }
}

/// Run the garage command
#[allow(clippy::too_many_lines)]
#[allow(clippy::future_not_send)] // StdoutLock in log streaming loop is not Send
pub async fn run(cmd: GarageCommand, flags: &GlobalFlags) -> Result<()> {
    match cmd.action {
        GarageAction::List => {
            let client = create_client(flags, None)?;
            let now = chrono::Utc::now();

            let effective_context = flags.context.as_deref();
            let show_all = effective_context == Some("all");

            // Validate context if specified (and not "all")
            if let Some(ctx) = effective_context
                && ctx != "all"
            {
                let contexts = moto_k8s::K8sClient::list_contexts().unwrap_or_default();
                if !contexts.is_empty() && !contexts.contains(&ctx.to_string()) {
                    return Err(CliError::not_found(format!(
                        "Context '{ctx}' not found in kubeconfig.\n\n\
                             Available contexts: {}\n\
                             Try: moto garage list --context all",
                        contexts.join(", ")
                    )));
                }
            }

            // Resolve the current context for filtering and display
            let current_context = resolve_current_context();

            let response = client
                .list_garages()
                .await
                .map_err(client_error_to_cli_error)?;

            // Filter garages by context. Garages from this moto-club belong to
            // the current kubectl context. When --context <name> targets a
            // different context, we have no garages to show from it.
            let garages = if let Some(ctx) = effective_context {
                if ctx == "all" || ctx == current_context {
                    response.garages
                } else {
                    Vec::new()
                }
            } else {
                response.garages
            };

            if flags.json {
                let json = GarageListJson {
                    garages: garages
                        .iter()
                        .map(|g| {
                            let created_at = chrono::DateTime::parse_from_rfc3339(&g.created_at)
                                .map(|dt| dt.with_timezone(&chrono::Utc))
                                .unwrap_or(now);
                            let age_seconds = (now - created_at).num_seconds();

                            let ttl_remaining_seconds =
                                chrono::DateTime::parse_from_rfc3339(&g.expires_at)
                                    .ok()
                                    .map_or(0, |exp| {
                                        let remaining =
                                            (exp.with_timezone(&chrono::Utc) - now).num_seconds();
                                        remaining.max(0)
                                    });

                            GarageJson {
                                id: format_short_id(&g.id),
                                name: g.name.clone(),
                                context: if show_all {
                                    Some(current_context.clone())
                                } else {
                                    None
                                },
                                branch: g.branch.clone(),
                                status: g.status.clone(),
                                ttl_remaining_seconds,
                                age_seconds,
                            }
                        })
                        .collect(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if garages.is_empty() {
                if !flags.quiet {
                    println!("No garages found.");
                }
            } else if show_all {
                // Output with CONTEXT column when --context all
                println!(
                    "{:<9} {:<24} {:<14} {:<9} {:<12} {:<10} AGE",
                    "ID", "NAME", "CONTEXT", "BRANCH", "STATUS", "TTL"
                );
                for g in garages {
                    let created_at = chrono::DateTime::parse_from_rfc3339(&g.created_at)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or(now);
                    let age = format_duration((now - created_at).num_seconds());

                    let ttl = format_ttl_remaining(&g.expires_at, now);
                    let branch = if g.branch.is_empty() { "-" } else { &g.branch };
                    println!(
                        "{:<9} {:<24} {:<14} {:<9} {:<12} {:<10} {}",
                        format_short_id(&g.id),
                        truncate(&g.name, 24),
                        truncate(&current_context, 14),
                        truncate(branch, 9),
                        g.status,
                        ttl,
                        age
                    );
                }
            } else {
                // Default output (no context column)
                println!(
                    "{:<9} {:<24} {:<9} {:<12} {:<10} AGE",
                    "ID", "NAME", "BRANCH", "STATUS", "TTL"
                );
                for g in garages {
                    let created_at = chrono::DateTime::parse_from_rfc3339(&g.created_at)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or(now);
                    let age = format_duration((now - created_at).num_seconds());

                    let ttl = format_ttl_remaining(&g.expires_at, now);
                    let branch = if g.branch.is_empty() { "-" } else { &g.branch };
                    println!(
                        "{:<9} {:<24} {:<9} {:<12} {:<10} {}",
                        format_short_id(&g.id),
                        truncate(&g.name, 24),
                        truncate(branch, 9),
                        g.status,
                        ttl,
                        age
                    );
                }
            }
        }

        GarageAction::Open {
            name,
            owner,
            branch,
            ttl,
            image,
            engine,
            with_postgres,
            with_redis,
            no_attach,
            kubectl,
        } => {
            let client = create_client(flags, owner.as_deref())?;
            let name = name.unwrap_or_else(crate::names::generate);

            // Use CLI flag, then config default, then hardcoded default
            let ttl_str = ttl
                .as_deref()
                .or(flags.config.garage.ttl.as_deref())
                .unwrap_or("4h");
            let ttl_seconds = parse_ttl(ttl_str)?;

            if !flags.quiet && !flags.json {
                println!("Opening garage...");
            }

            let request = CreateGarageRequest {
                name: Some(name.clone()),
                branch: branch.clone(),
                ttl_seconds: Some(ttl_seconds),
                image,
                engine: engine.clone(),
                with_postgres: if with_postgres { Some(true) } else { None },
                with_redis: if with_redis { Some(true) } else { None },
            };

            let garage = client
                .create_garage(&request)
                .await
                .map_err(client_error_to_cli_error)?;

            if flags.json {
                let json = GarageOpenJson {
                    id: format_short_id(&garage.id),
                    name: garage.name.clone(),
                    branch: garage.branch.clone(),
                    ttl_seconds,
                    expires_at: garage.expires_at.clone(),
                    status: garage.status.clone(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                // Format expiration time for display
                let expires_display = format_expires_at(&garage.expires_at);

                // Output format per spec v0.4:
                // Garage created: abc123
                //   Name:    feature-tokenization
                //   Branch:  main
                //   TTL:     4h (expires 2026-01-20 02:48:00)
                //   Status:  Ready
                println!("Garage created: {}", format_short_id(&garage.id));
                println!("  Name:    {}", garage.name);
                println!("  Branch:  {}", garage.branch);
                println!("  TTL:     {ttl_str} (expires {expires_display})");
                println!("  Status:  {}", garage.status);
                println!();

                if no_attach {
                    // --no-attach: just show how to connect later
                    println!("To connect: moto garage enter {}", garage.name);
                } else if kubectl {
                    // --kubectl: connect via kubectl exec
                    println!(
                        "Connecting via kubectl... (use `moto garage enter {}` to reconnect)",
                        garage.name
                    );
                    println!();

                    let namespace = resolve_namespace(&garage.namespace, &garage.id);
                    let pod_name = resolve_pod_name(&garage.pod_name);

                    kubectl_exec(&namespace, &pod_name, flags).await?;
                } else {
                    // Default: connect via WireGuard tunnel
                    println!(
                        "Connecting... (use `moto garage enter {}` to reconnect)",
                        garage.name
                    );
                    println!();

                    // Initialize tunnel manager and enter the garage
                    let manager = TunnelManager::new().await.map_err(|e| {
                        CliError::general(format!("failed to initialize tunnel: {e}"))
                    })?;

                    let config = EnterConfig::default();
                    let progress = ConsoleProgress::new(flags.quiet);

                    let session = Box::pin(enter_garage(&manager, &garage.name, config, &progress))
                        .await
                        .map_err(|e| match e {
                            EnterError::GarageNotFound(_) => CliError::not_found(format!(
                                "Garage '{}' not found.\n\nTry: moto garage list",
                                garage.name
                            )),
                            EnterError::NotAuthorized(_) => CliError::general(format!(
                                "Not authorized to access garage '{}'.\n\nCheck your permissions.",
                                garage.name
                            )),
                            EnterError::ConnectionFailed(msg) => CliError::general(format!(
                                "Connection failed: {msg}\n\nTry: moto garage logs {}",
                                garage.name
                            )),
                            _ => CliError::general(e.to_string()),
                        })?;

                    // Connect to ttyd - this blocks until the terminal session ends
                    session.connect_ttyd().await.map_err(|e| match e {
                        EnterError::TtydFailed(msg) => CliError::general(format!(
                            "Terminal connection failed: {msg}\n\n\
                             This may happen if the garage is still starting up.\n\
                             Try: moto garage logs {}",
                            garage.name
                        )),
                        _ => CliError::general(e.to_string()),
                    })?;
                }
            }
        }

        GarageAction::Enter { name, kubectl } => {
            let client = create_client(flags, None)?;

            // Get garage details
            let garage = client
                .get_garage(&name)
                .await
                .map_err(client_error_to_cli_error)?;

            if kubectl {
                // --kubectl: connect via kubectl exec
                if !flags.quiet && !flags.json {
                    eprintln!("Connecting to {name} via kubectl...");
                }

                let namespace = resolve_namespace(&garage.namespace, &garage.id);
                let pod_name = resolve_pod_name(&garage.pod_name);

                kubectl_exec(&namespace, &pod_name, flags).await?;
            } else {
                if !flags.quiet && !flags.json {
                    eprintln!("Connecting to garage {name}...");
                }

                // Initialize tunnel manager
                let manager = TunnelManager::new()
                    .await
                    .map_err(|e| CliError::general(format!("failed to initialize tunnel: {e}")))?;

                // Configure enter
                let config = EnterConfig::default();
                let progress = ConsoleProgress::new(flags.quiet);

                // Enter the garage
                let session = Box::pin(enter_garage(&manager, &name, config, &progress))
                    .await
                    .map_err(|e| match e {
                        EnterError::GarageNotFound(_) => CliError::not_found(format!(
                            "Garage '{name}' not found.\n\nTry: moto garage list"
                        )),
                        EnterError::NotAuthorized(_) => CliError::general(format!(
                            "Not authorized to access garage '{name}'.\n\nCheck your permissions."
                        )),
                        EnterError::ConnectionFailed(msg) => CliError::general(format!(
                            "Connection failed: {msg}\n\nTry: moto garage logs {name}"
                        )),
                        _ => CliError::general(e.to_string()),
                    })?;

                if flags.json {
                    // JSON mode: output session info without connecting
                    let json = GarageEnterJson {
                        name: session.garage_name().to_string(),
                        session_id: session.session_id().to_string(),
                        client_ip: String::new(), // Not exposed on session handle
                        garage_ip: session.garage_ip().to_string(),
                        path_type: "derp".to_string(), // Default for now
                        path_detail: "primary".to_string(),
                    };
                    println!("{}", serde_json::to_string_pretty(&json)?);
                } else {
                    // Interactive mode: connect to ttyd terminal
                    if !flags.quiet {
                        eprintln!("  Opening terminal session... done");
                        eprintln!();
                    }

                    // Connect to ttyd - this blocks until the terminal session ends
                    session.connect_ttyd().await.map_err(|e| match e {
                        EnterError::TtydFailed(msg) => CliError::general(format!(
                            "Terminal connection failed: {msg}\n\n\
                             This may happen if the garage is still starting up.\n\
                             Try: moto garage logs {name}"
                        )),
                        _ => CliError::general(e.to_string()),
                    })?;
                }
            }
        }

        GarageAction::Close { name, force } => {
            let client = create_client(flags, None)?;

            // Check if garage exists first
            let garage = client
                .get_garage(&name)
                .await
                .map_err(client_error_to_cli_error)?;

            // Check for unsaved changes unless --force is used
            if !force && !flags.json {
                let namespace = resolve_namespace(&garage.namespace, &garage.id);
                let pod_name = if garage.pod_name.is_empty() {
                    format!("garage-{}", &garage.id.to_string()[..8])
                } else {
                    garage.pod_name.clone()
                };

                // Check for unsaved changes
                if let Ok(has_changes) = has_unsaved_changes(&namespace, &pod_name, flags).await
                    && has_changes
                {
                    eprintln!("Warning: This garage has unsaved changes.");
                    eprintln!("Consider syncing your work first (push changes or create a PR).");
                    eprintln!();
                }
            }

            // Prompt for confirmation unless --force is used
            if !force && !flags.json {
                eprint!("Close garage '{name}'? [y/N] ");
                io::stderr().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    if !flags.quiet {
                        eprintln!("Aborted.");
                    }
                    return Ok(());
                }
            }

            if !flags.quiet && !flags.json {
                println!("Closing garage '{name}'...");
            }

            client
                .close_garage(&name)
                .await
                .map_err(client_error_to_cli_error)?;

            if flags.json {
                let json = GarageCloseJson {
                    name,
                    status: "closed".to_string(),
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                println!("Garage closed.");
            }
        }

        GarageAction::Logs {
            name,
            follow,
            tail,
            since,
        } => {
            if follow && flags.json {
                return Err(CliError::invalid_input(
                    "JSON output is not supported with --follow",
                ));
            }

            let client = create_client(flags, None)?;

            // Try WebSocket first, fall back to direct K8s API
            let ws_result = client
                .stream_logs_ws(&name, tail, follow, since.as_deref())
                .await;

            match ws_result {
                Ok(mut rx) => {
                    tracing::debug!("streaming logs via WebSocket");
                    let mut stdout = io::stdout().lock();
                    while let Some(result) = rx.recv().await {
                        match result {
                            Ok(line) => {
                                stdout.write_all(line.as_bytes())?;
                                stdout.flush()?;
                            }
                            Err(e) => {
                                return Err(CliError::general(format!("log stream error: {e}")));
                            }
                        }
                    }
                }
                Err(ws_err) => {
                    // Fall back to direct K8s API
                    tracing::debug!(error = %ws_err, "WebSocket log streaming unavailable, falling back to K8s API");

                    let garage = client.get_garage(&name).await.map_err(|e| match e {
                        ClientError::GarageNotFound(_) => CliError::not_found(format!(
                            "Garage '{name}' not found.\n\nTry: moto garage list"
                        )),
                        _ => client_error_to_cli_error(e),
                    })?;

                    let namespace = format!("moto-garage-{}", &garage.id.to_string()[..8]);
                    let since_seconds = since.as_deref().map(parse_duration).transpose()?;

                    let k8s_client = if let Some(ctx) = flags.context.as_deref() {
                        K8sClient::with_context(ctx).await?
                    } else {
                        K8sClient::new().await?
                    };

                    let log_options = PodLogOptions {
                        tail_lines: Some(tail),
                        since_seconds,
                        follow,
                    };

                    if follow {
                        let mut stream = k8s_client
                            .stream_pod_logs(&namespace, None, &log_options)
                            .await
                            .map_err(|_| {
                                CliError::not_found(format!(
                                    "Garage '{name}' pod not found.\n\nThe garage may still be starting up.\nTry: moto garage list"
                                ))
                            })?;

                        let mut stdout = io::stdout().lock();
                        while let Some(result) = stream.next().await {
                            match result {
                                Ok(line) => {
                                    stdout.write_all(line.as_bytes())?;
                                    stdout.flush()?;
                                }
                                Err(e) => {
                                    return Err(CliError::general(format!(
                                        "log stream error: {e}"
                                    )));
                                }
                            }
                        }
                    } else {
                        let logs = k8s_client
                            .get_pod_logs(&namespace, None, &log_options)
                            .await?;

                        if logs.is_empty() {
                            if !flags.quiet {
                                eprintln!("No logs found for garage '{name}'.");
                            }
                        } else {
                            print!("{logs}");
                            if !logs.ends_with('\n') {
                                println!();
                            }
                        }
                    }
                }
            }
        }

        GarageAction::Watch { garages } => {
            let client = create_client(flags, None)?;

            // Print header unless quiet or json
            if !flags.quiet && !flags.json {
                if let Some(ref names) = garages {
                    eprintln!("Watching {}... (Ctrl+C to stop)", names.join(", "));
                } else {
                    eprintln!("Watching all garages... (Ctrl+C to stop)");
                }
                eprintln!();
            }

            let backoff_steps: &[u64] = &[1, 2, 4, 10];
            let mut backoff_index: usize = 0;

            loop {
                let connect_result = client.stream_events_ws(garages.as_deref()).await;

                let mut rx = match connect_result {
                    Ok(rx) => {
                        // Reset backoff on successful connection
                        backoff_index = 0;
                        rx
                    }
                    Err(e) => {
                        let delay = backoff_steps[backoff_index.min(backoff_steps.len() - 1)];
                        if !flags.quiet && !flags.json {
                            eprintln!("Connection failed: {e}. Reconnecting in {delay}s...");
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                        backoff_index = (backoff_index + 1).min(backoff_steps.len() - 1);
                        continue;
                    }
                };

                // Process events until channel closes (disconnect)
                {
                    let mut stdout = io::stdout().lock();
                    while let Some(result) = rx.recv().await {
                        match result {
                            Ok(event) => {
                                if flags.json {
                                    let line = serde_json::to_string(&event)?;
                                    writeln!(stdout, "{line}")?;
                                } else {
                                    let formatted = format_event(&event);
                                    writeln!(stdout, "{formatted}")?;
                                }
                                stdout.flush()?;
                            }
                            Err(e) => {
                                if !flags.quiet && !flags.json {
                                    eprintln!("Stream error: {e}");
                                }
                                break;
                            }
                        }
                    }
                }

                // WebSocket disconnected — reconnect with backoff
                let delay = backoff_steps[backoff_index.min(backoff_steps.len() - 1)];
                if !flags.quiet && !flags.json {
                    eprintln!("Disconnected. Reconnecting in {delay}s...");
                }
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                backoff_index = (backoff_index + 1).min(backoff_steps.len() - 1);

                // Fetch current state via REST before resuming WebSocket
                if let Ok(response) = client.list_garages().await {
                    let now = chrono::Utc::now();
                    let target_garages: Vec<_> = if let Some(ref names) = garages {
                        response
                            .garages
                            .iter()
                            .filter(|g| names.contains(&g.name))
                            .collect()
                    } else {
                        response.garages.iter().collect()
                    };

                    if !target_garages.is_empty() {
                        let mut stdout = io::stdout().lock();
                        for g in &target_garages {
                            if flags.json {
                                let event = GarageEvent::StatusChange {
                                    garage: g.name.clone(),
                                    from: "unknown".to_string(),
                                    to: g.status.clone(),
                                    reason: Some("reconnect_sync".to_string()),
                                };
                                if let Ok(line) = serde_json::to_string(&event) {
                                    let _ = writeln!(stdout, "{line}");
                                }
                            } else {
                                let ttl = format_ttl_remaining(&g.expires_at, now);
                                let _ = writeln!(
                                    stdout,
                                    "[{}] Current state: {} (TTL: {})",
                                    g.name, g.status, ttl
                                );
                            }
                        }
                        let _ = stdout.flush();
                    }
                }

                if !flags.quiet && !flags.json {
                    eprintln!("Reconnected. Resuming watch...");
                    eprintln!();
                }
            }
        }

        GarageAction::Extend { name, ttl } => {
            let client = create_client(flags, None)?;

            // Parse TTL extension
            let seconds = parse_duration(&ttl)?;
            if seconds <= 0 {
                return Err(CliError::invalid_input("TTL extension must be positive"));
            }

            if !flags.quiet && !flags.json {
                println!("Extending garage '{name}' TTL by {ttl}...");
            }

            let response = client
                .extend_garage(&name, seconds)
                .await
                .map_err(|e| match e {
                    ClientError::GarageNotFound(_) => CliError::not_found(format!(
                        "Garage '{name}' not found.\n\nTry: moto garage list"
                    )),
                    ClientError::Server { code, message } if code == "GARAGE_EXPIRED" => {
                        CliError::general(format!(
                            "Garage '{name}' has expired and cannot be extended."
                        ))
                    }
                    ClientError::Server { code, message } if code == "INVALID_TTL" => {
                        CliError::invalid_input(message)
                    }
                    _ => client_error_to_cli_error(e),
                })?;

            if flags.json {
                let json = GarageExtendJson {
                    name: name.clone(),
                    expires_at: response.expires_at.clone(),
                    ttl_remaining_seconds: response.ttl_remaining_seconds,
                };
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if !flags.quiet {
                let ttl_formatted = format_duration(response.ttl_remaining_seconds);
                println!("Garage TTL extended.");
                println!("  New TTL: {ttl_formatted}");
                println!("  Expires: {}", format_expires_at(&response.expires_at));
            }
        }
    }

    Ok(())
}

/// Resolve the K8s namespace for a garage.
///
/// Uses the API response value, falling back to `moto-garage-{id[..8]}` if empty.
fn resolve_namespace(ns: &str, id: &uuid::Uuid) -> String {
    if ns.is_empty() {
        let hex = id.simple().to_string();
        format!("moto-garage-{}", &hex[..8])
    } else {
        ns.to_string()
    }
}

/// Resolve the K8s pod name for a garage.
///
/// Uses the API response value, falling back to `dev-container` if empty.
fn resolve_pod_name(pod: &str) -> String {
    if pod.is_empty() {
        "dev-container".to_string()
    } else {
        pod.to_string()
    }
}

/// Check if a garage has unsaved changes.
///
/// Runs `git status --porcelain` in the garage's workspace to detect uncommitted changes.
/// Returns `true` if there are unsaved changes, `false` otherwise.
///
/// # Errors
///
/// Returns an error if kubectl exec fails or the command cannot be executed.
async fn has_unsaved_changes(namespace: &str, pod_name: &str, flags: &GlobalFlags) -> Result<bool> {
    let mut cmd = tokio::process::Command::new("kubectl");

    // Global flags must come before the subcommand
    if let Some(ctx) = flags.context.as_deref() {
        cmd.args(["--context", ctx]);
    }

    cmd.args([
        "exec",
        "-n",
        namespace,
        pod_name,
        "--",
        "git",
        "-C",
        "/workspace",
        "status",
        "--porcelain",
    ]);

    let output = cmd.output().await.map_err(|e| {
        CliError::general(format!(
            "failed to run kubectl: {e}\n\n\
             Make sure kubectl is installed and in your PATH."
        ))
    })?;

    if !output.status.success() {
        // If git status fails (e.g., not a git repo, pod not ready), treat as no unsaved changes
        // to avoid blocking garage close
        return Ok(false);
    }

    // If output is non-empty, there are unsaved changes
    Ok(!output.stdout.is_empty())
}

/// Connect to a garage via `kubectl exec`.
///
/// Runs `kubectl exec -it -n {namespace} {pod_name} -- tmux new-session -A -s garage`
/// which takes over the terminal until the user detaches. The `-A` flag attaches to
/// an existing `garage` session or creates one if it doesn't exist.
async fn kubectl_exec(namespace: &str, pod_name: &str, flags: &GlobalFlags) -> Result<()> {
    let mut cmd = tokio::process::Command::new("kubectl");

    // Global flags must come before the subcommand
    if let Some(ctx) = flags.context.as_deref() {
        cmd.args(["--context", ctx]);
    }

    cmd.args([
        "exec",
        "-it",
        "-n",
        namespace,
        pod_name,
        "--",
        "tmux",
        "new-session",
        "-A",
        "-s",
        "garage",
    ]);

    let status = cmd.status().await.map_err(|e| {
        CliError::general(format!(
            "failed to run kubectl: {e}\n\n\
             Make sure kubectl is installed and in your PATH."
        ))
    })?;

    if !status.success() {
        let code = status.code().unwrap_or(1);
        return Err(CliError::general(format!(
            "kubectl exec exited with code {code}\n\n\
             The garage pod may not be ready yet.\n\
             Try: moto garage list"
        )));
    }

    Ok(())
}

/// Format a garage event for human-readable output.
fn format_event(event: &GarageEvent) -> String {
    match event {
        GarageEvent::StatusChange {
            garage,
            from,
            to,
            reason,
        } => {
            let suffix = reason
                .as_deref()
                .map_or(String::new(), |r| format!(" ({r})"));
            format!("[{garage}] Status: {from} \u{2192} {to}{suffix}")
        }
        GarageEvent::TtlWarning {
            garage,
            minutes_remaining,
            expires_at,
        } => {
            let expires_display = format_expires_at(expires_at);
            format!(
                "[{garage}] TTL warning: {minutes_remaining} minutes remaining (expires {expires_display})"
            )
        }
        GarageEvent::Error { garage, message } => {
            format!("[{garage}] Error: {message}")
        }
    }
}

/// Truncate a string to a maximum length, adding "..." if truncated
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Format a UUID as a short 6-character ID (first 6 hex chars).
/// e.g., "abc123" from "abc12345-6789-..."
fn format_short_id(id: &uuid::Uuid) -> String {
    let hex = id.simple().to_string();
    hex[..6].to_string()
}

/// Get the current kubectl context name from kubeconfig.
fn resolve_current_context() -> String {
    moto_k8s::K8sClient::current_context()
        .ok()
        .flatten()
        .unwrap_or_else(|| "default".to_string())
}

/// Format an `expires_at` timestamp as a TTL remaining string.
fn format_ttl_remaining(expires_at: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    chrono::DateTime::parse_from_rfc3339(expires_at)
        .ok()
        .map_or_else(
            || "-".to_string(),
            |exp| {
                let remaining = (exp.with_timezone(&chrono::Utc) - now).num_seconds();
                if remaining <= 0 {
                    "expired".to_string()
                } else {
                    format_duration(remaining)
                }
            },
        )
}

/// Format an ISO 8601 timestamp for display (e.g., "2026-01-20 02:48:00").
fn format_expires_at(expires_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(expires_at).map_or_else(
        |_| expires_at.to_string(),
        |dt| dt.format("%Y-%m-%d %H:%M:%S").to_string(),
    )
}

/// Format a duration in seconds as a human-readable string (e.g., "2h15m", "45m", "3d").
fn format_duration(seconds: i64) -> String {
    if seconds < 0 {
        return "0s".to_string();
    }

    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;

    if days > 0 {
        if hours > 0 {
            format!("{days}d{hours}h")
        } else {
            format!("{days}d")
        }
    } else if hours > 0 {
        if minutes > 0 {
            format!("{hours}h{minutes}m")
        } else {
            format!("{hours}h")
        }
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    }
}

/// Parse a duration string like "5m", "1h", "2d" into seconds.
fn parse_duration(s: &str) -> Result<i64> {
    let s = s.trim();
    if s.is_empty() {
        return Err(CliError::invalid_input("empty duration"));
    }

    let (num_str, unit) = s.split_at(s.len() - 1);
    let num: i64 = num_str
        .parse()
        .map_err(|_| CliError::invalid_input(format!("invalid duration number: {num_str}")))?;

    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => {
            return Err(CliError::invalid_input(format!(
                "invalid duration unit: {unit} (use s, m, h, or d)"
            )));
        }
    };

    Ok(num * multiplier)
}

/// Parse a TTL duration string with validation (max 48h).
fn parse_ttl(s: &str) -> Result<i64> {
    const MAX_TTL_SECONDS: i64 = 48 * 3600; // 48 hours
    let seconds = parse_duration(s)?;

    if seconds <= 0 {
        return Err(CliError::invalid_input("TTL must be positive"));
    }
    if seconds > MAX_TTL_SECONDS {
        return Err(CliError::invalid_input(format!(
            "TTL exceeds maximum of 48h (got {s})"
        )));
    }

    Ok(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(90), "1m");
        assert_eq!(format_duration(3540), "59m");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3660), "1h1m");
        assert_eq!(format_duration(8100), "2h15m");
        assert_eq!(format_duration(7200), "2h");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d1h");
        assert_eq!(format_duration(172_800), "2d");
    }

    #[test]
    fn test_format_duration_negative() {
        assert_eq!(format_duration(-100), "0s");
    }

    #[test]
    fn test_format_short_id() {
        let id = uuid::Uuid::parse_str("abc12345-6789-0def-1234-567890abcdef").unwrap();
        assert_eq!(format_short_id(&id), "abc123");
    }

    #[test]
    fn test_format_expires_at() {
        assert_eq!(
            format_expires_at("2026-01-20T02:48:00Z"),
            "2026-01-20 02:48:00"
        );
        // Invalid timestamp returns as-is
        assert_eq!(format_expires_at("invalid"), "invalid");
    }
}
