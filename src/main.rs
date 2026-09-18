mod auth;
mod autostart;
mod config;
mod dialog;
mod engine;
mod tray;

use anyhow::Result;
use auth::AuthError;
use config::AppConfig;
use ksni::TrayMethods;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};
use tray::{DriveState, DriveTray, TrayCommand};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("gdrive=info".parse()?),
        )
        .init();

    info!("Google Drive Tray Microservice gestartet.");

    let args: Vec<String> = std::env::args().collect();
    if args.contains(&"--unmount".to_string()) {
        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let default_mount = home.join("GoogleDrive");
        let _ = engine::unmount_drive(&default_mount);
        println!("Unmount durchgeführt.");
        return Ok(());
    }

    // 1. Load or prompt config
    let mut config_opt = AppConfig::load()?;
    let needs_setup = match &config_opt {
        Some(cfg) => !cfg.is_configured() || args.contains(&"--setup".to_string()),
        None => true,
    };

    if needs_setup {
        info!("Keine vollständige Konfiguration gefunden oder --setup übergeben. Starte Ersteinrichtung...");

        let creds = match dialog::prompt_setup_gui()? {
            Some((id, sec)) => (id, sec),
            None => {
                println!("\n--- Google Drive Ersteinrichtung ---");
                println!("Bitte erstelle ein Desktop-OAuth Projekt in der Google Cloud Console:");
                println!("1. https://console.cloud.google.com/apis/credentials aufrufen");
                println!("2. Google Drive API aktivieren");
                println!("3. OAuth-Client-ID (Typ: Desktop-App) erstellen\n");

                let mut id_buf = String::new();
                let mut sec_buf = String::new();
                print!("Google Client-ID: ");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                let _ = std::io::stdin().read_line(&mut id_buf);

                print!("Google Client-Secret: ");
                let _ = std::io::Write::flush(&mut std::io::stdout());
                let _ = std::io::stdin().read_line(&mut sec_buf);

                (id_buf.trim().to_string(), sec_buf.trim().to_string())
            }
        };

        if creds.0.is_empty() || creds.1.is_empty() {
            eprintln!("Fehler: Client-ID und Client-Secret dürfen nicht leer sein.");
            std::process::exit(1);
        }

        info!("Starte Google OAuth Autorisierung...");
        dialog::send_notification(
            "Google Drive Autorisierung",
            "Bitte bestätige den Zugriff im geöffneten Browser-Fenster...",
        );

        let token_json = auth::run_oauth_flow(&creds.0, &creds.1).await?;

        let mut new_config = AppConfig::default();
        new_config.client_id = creds.0;
        new_config.client_secret = creds.1;
        new_config.token_json = Some(token_json);
        new_config.save()?;

        if new_config.autostart {
            let _ = autostart::enable_autostart();
        }

        dialog::send_notification(
            "Google Drive eingerichtet",
            "Ersteinrichtung erfolgreich! Das Netzlaufwerk wird gestartet.",
        );

        config_opt = Some(new_config);
    }

    let config = Arc::new(Mutex::new(config_opt.unwrap()));

    // 2. Setup Tray
    let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel::<TrayCommand>();
    let mount_point = { config.lock().await.mount_point.clone() };
    let autostart_status = autostart::is_autostart_enabled();

    let drive_tray = DriveTray::new(mount_point.clone(), autostart_status, cmd_tx.clone());
    let tray_handle = drive_tray.spawn().await?;

    // 3. Mount Drive
    let child_process: Arc<Mutex<Option<std::process::Child>>> = Arc::new(Mutex::new(None));

    let mount_drive_helper = {
        let config = Arc::clone(&config);
        let child_proc = Arc::clone(&child_process);
        let tray_h = tray_handle.clone();
        let mp = mount_point.clone();

        Arc::new(move || {
            let config = Arc::clone(&config);
            let child_proc = Arc::clone(&child_proc);
            let tray_h = tray_h.clone();
            let mp = mp.clone();

            tokio::spawn(async move {
                let _ = tray_h.update(|t| t.state = DriveState::Connecting).await;
                let cfg = config.lock().await.clone();

                match engine::mount_drive(&cfg) {
                    Ok(child) => {
                        let mut proc_guard = child_proc.lock().await;
                        *proc_guard = Some(child);

                        // Wait for mount to appear
                        let mut mounted = false;
                        for _ in 0..15 {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            if engine::is_mounted(&mp) {
                                mounted = true;
                                break;
                            }
                        }

                        if mounted {
                            info!("Google Drive erfolgreich gemountet unter {:?}", mp);
                            let _ = tray_h.update(|t| t.state = DriveState::Connected).await;
                            dialog::send_notification(
                                "Google Drive eingebunden",
                                &format!("Eingebunden unter {:?}", mp),
                            );
                        } else {
                            warn!("Mount-Prozess läuft, aber Verzeichnis noch nicht als FUSE gemeldet.");
                            let _ = tray_h.update(|t| t.state = DriveState::Connected).await;
                        }
                    }
                    Err(e) => {
                        error!("Fehler beim Mounten: {}", e);
                        let _ = tray_h.update(|t| t.state = DriveState::Error(e.to_string())).await;
                        dialog::send_notification(
                            "Google Drive Fehler",
                            &format!("Konnte nicht einbinden: {}", e),
                        );
                    }
                }
            })
        })
    };

    // Initial mount
    mount_drive_helper();

    // 4. Background Token Validator & Re-auth Monitor Task
    {
        let config = Arc::clone(&config);
        let tray_h = tray_handle.clone();
        let cmd_tx = cmd_tx.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(600));
            loop {
                interval.tick().await;

                let (cid, csec, tok_opt) = {
                    let c = config.lock().await;
                    (c.client_id.clone(), c.client_secret.clone(), c.token_json.clone())
                };

                if let Some(tok_json) = tok_opt {
                    match auth::refresh_token_if_needed(&cid, &csec, &tok_json).await {
                        Ok(Some(updated_tok)) => {
                            info!("Token aktualisiert, speichere neue Konfiguration...");
                            let mut c = config.lock().await;
                            c.token_json = Some(updated_tok);
                            let _ = c.save();
                        }
                        Ok(None) => {
                            // Token is fine
                        }
                        Err(AuthError::NeedsReauth) => {
                            warn!("Token abgelaufen oder widerrufen. Fordere Neuanmeldung an...");
                            let _ = tray_h.update(|t| t.state = DriveState::NeedsAuth).await;
                            dialog::send_notification(
                                "Google Drive: Sitzung abgelaufen",
                                "Bitte erneut im Browser anmelden, um die Synchronisation fortzusetzen.",
                            );

                            // Trigger dialog prompt in a separate blocking task
                            let cmd_tx_clone = cmd_tx.clone();
                            tokio::task::spawn_blocking(move || {
                                if dialog::prompt_reauth_dialog() {
                                    let _ = cmd_tx_clone.send(TrayCommand::Reauth);
                                }
                            });
                        }
                        Err(e) => {
                            warn!("Fehler bei Token-Prüfung: {}", e);
                        }
                    }
                }
            }
        });
    }

    // 5. Main event loop: process tray commands & system signals
    loop {
        tokio::select! {
            Some(cmd) = cmd_rx.recv() => {
                match cmd {
                    TrayCommand::OpenFolder => {
                        let mp = { config.lock().await.mount_point.clone() };
                        info!("Öffne Verzeichnis {:?}", mp);
                        if let Err(e) = opener::open(&mp) {
                            warn!("Konnte Dateimanager nicht öffnen: {}", e);
                        }
                    }
                    TrayCommand::Reconnect => {
                        info!("Neu verbinden angefordert...");
                        let mp = { config.lock().await.mount_point.clone() };
                        let _ = engine::unmount_drive(&mp);
                        mount_drive_helper();
                    }
                    TrayCommand::Disconnect => {
                        info!("Laufwerk trennen angefordert...");
                        let mp = { config.lock().await.mount_point.clone() };
                        let mut proc_guard = child_process.lock().await;
                        if let Some(mut child) = proc_guard.take() {
                            let _ = child.kill();
                        }
                        let _ = engine::unmount_drive(&mp);
                        let _ = tray_handle.update(|t| t.state = DriveState::Disconnected).await;
                        dialog::send_notification("Google Drive", "Netzlaufwerk getrennt.");
                    }
                    TrayCommand::Reauth => {
                        info!("Re-Auth gestartet...");
                        let _ = tray_handle.update(|t| t.state = DriveState::Connecting).await;
                        let (cid, csec) = {
                            let c = config.lock().await;
                            (c.client_id.clone(), c.client_secret.clone())
                        };

                        match auth::run_oauth_flow(&cid, &csec).await {
                            Ok(new_tok) => {
                                info!("Erfolgreich neu autorisiert!");
                                {
                                    let mut c = config.lock().await;
                                    c.token_json = Some(new_tok);
                                    let _ = c.save();
                                }
                                dialog::send_notification("Google Drive", "Erfolgreich neu angemeldet! Binde Laufwerk ein...");
                                mount_drive_helper();
                            }
                            Err(e) => {
                                error!("Re-Auth fehlgeschlagen: {}", e);
                                let _ = tray_handle.update(|t| t.state = DriveState::NeedsAuth).await;
                                dialog::send_notification("Google Drive Fehler", &format!("Anmeldung fehlgeschlagen: {}", e));
                            }
                        }
                    }
                    TrayCommand::ToggleAutostart(enable) => {
                        if enable {
                            let _ = autostart::enable_autostart();
                        } else {
                            let _ = autostart::disable_autostart();
                        }
                        let mut c = config.lock().await;
                        c.autostart = enable;
                        let _ = c.save();
                    }
                    TrayCommand::Quit => {
                        info!("Beenden angefordert. Führe sauberes Unmount durch...");
                        let mp = { config.lock().await.mount_point.clone() };
                        let mut proc_guard = child_process.lock().await;
                        if let Some(mut child) = proc_guard.take() {
                            let _ = child.kill();
                        }
                        let _ = engine::unmount_drive(&mp);
                        break;
                    }
                }
            }

            _ = tokio::signal::ctrl_c() => {
                info!("SIGINT / Ctrl-C empfangen. Beende und trenne Mount...");
                let mp = { config.lock().await.mount_point.clone() };
                let mut proc_guard = child_process.lock().await;
                if let Some(mut child) = proc_guard.take() {
                    let _ = child.kill();
                }
                let _ = engine::unmount_drive(&mp);
                break;
            }
        }
    }

    info!("Google Drive Tray Microservice beendet.");
    Ok(())
}
