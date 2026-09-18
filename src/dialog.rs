use anyhow::Result;
use std::process::Command;
use tracing::info;

pub fn send_notification(title: &str, body: &str) {
    let _ = Command::new("notify-send")
        .arg("-a")
        .arg("Google Drive")
        .arg(title)
        .arg(body)
        .spawn();
}

pub fn prompt_reauth_dialog() -> bool {
    // 1. Try kdialog (KDE Plasma native)
    if let Ok(status) = Command::new("kdialog")
        .arg("--title")
        .arg("Google Drive – Sitzung abgelaufen")
        .arg("--yesno")
        .arg("Deine Google Drive Sitzung ist abgelaufen oder wurde widerrufen.\n\nMöchtest du dich jetzt im Browser neu anmelden?")
        .arg("--yes-label")
        .arg("Im Browser anmelden")
        .arg("--no-label")
        .arg("Später")
        .status()
    {
        return status.success();
    }

    // 2. Try zenity (GNOME / GTK native)
    if let Ok(status) = Command::new("zenity")
        .arg("--question")
        .arg("--title=Google Drive – Sitzung abgelaufen")
        .arg("--text=Deine Google Drive Sitzung ist abgelaufen oder wurde widerrufen.\n\nMöchtest du dich jetzt im Browser neu anmelden?")
        .arg("--ok-label=Im Browser anmelden")
        .arg("--cancel-label=Später")
        .status()
    {
        return status.success();
    }

    // Default fallback
    send_notification(
        "Google Drive: Sitzung abgelaufen",
        "Bitte klicke im Tray-Menü auf 'Im Browser neu anmelden'.",
    );
    false
}

pub fn prompt_input_dialog(title: &str, prompt: &str, is_password: bool) -> Option<String> {
    // 1. Try kdialog
    let mut cmd = Command::new("kdialog");
    cmd.arg("--title").arg(title);
    if is_password {
        cmd.arg("--password").arg(prompt);
    } else {
        cmd.arg("--inputbox").arg(prompt);
    }

    if let Ok(output) = cmd.output() {
        if output.status.success() {
            let res = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !res.is_empty() {
                return Some(res);
            }
        }
    }

    // 2. Try zenity
    let mut zcmd = Command::new("zenity");
    zcmd.arg("--entry").arg(format!("--title={}", title)).arg(format!("--text={}", prompt));
    if is_password {
        zcmd.arg("--hide-text");
    }

    if let Ok(output) = zcmd.output() {
        if output.status.success() {
            let res = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !res.is_empty() {
                return Some(res);
            }
        }
    }

    None
}

pub fn prompt_setup_gui() -> Result<Option<(String, String)>> {
    info!("Starte grafischen Setup-Dialog für Google Drive Credentials...");
    send_notification(
        "Google Drive Einrichtung",
        "Bitte Client-ID und Client-Secret eingeben...",
    );

    let client_id = match prompt_input_dialog(
        "Google Drive Setup (1/2)",
        "Bitte deine Google OAuth Client-ID eingeben:\n(z.B. xxxxx.apps.googleusercontent.com)",
        false,
    ) {
        Some(id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => return Ok(None),
    };

    let client_secret = match prompt_input_dialog(
        "Google Drive Setup (2/2)",
        "Bitte dein Google OAuth Client-Secret eingeben:",
        true,
    ) {
        Some(sec) if !sec.trim().is_empty() => sec.trim().to_string(),
        _ => return Ok(None),
    };

    Ok(Some((client_id, client_secret)))
}
