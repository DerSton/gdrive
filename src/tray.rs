use ksni::menu::{CheckmarkItem, MenuItem, StandardItem};
use ksni::{Status, ToolTip, Tray};
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedSender;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveState {
    Starting,
    Connected,
    Connecting,
    NeedsAuth,
    Disconnected,
    Error(String),
}

#[derive(Debug)]
pub enum TrayCommand {
    OpenFolder,
    Reconnect,
    Disconnect,
    Reauth,
    ToggleAutostart(bool),
    Quit,
}

#[derive(Clone)]
pub struct DriveTray {
    pub state: DriveState,
    pub mount_point: PathBuf,
    pub autostart_enabled: bool,
    pub cmd_sender: UnboundedSender<TrayCommand>,
}

impl DriveTray {
    pub fn new(
        mount_point: PathBuf,
        autostart_enabled: bool,
        cmd_sender: UnboundedSender<TrayCommand>,
    ) -> Self {
        Self {
            state: DriveState::Starting,
            mount_point,
            autostart_enabled,
            cmd_sender,
        }
    }

    fn state_label(&self) -> String {
        match &self.state {
            DriveState::Starting => "Wird gestartet...".into(),
            DriveState::Connecting => "Verbinde...".into(),
            DriveState::Connected => "Verbunden".into(),
            DriveState::NeedsAuth => "Anmeldung erforderlich".into(),
            DriveState::Disconnected => "Getrennt".into(),
            DriveState::Error(e) => format!("Fehler: {}", e),
        }
    }
}

impl Tray for DriveTray {
    fn id(&self) -> String {
        "gdrive-tray".into()
    }

    fn title(&self) -> String {
        "Google Drive".into()
    }

    fn status(&self) -> Status {
        match self.state {
            DriveState::NeedsAuth => Status::NeedsAttention,
            _ => Status::Active,
        }
    }

    fn icon_name(&self) -> String {
        match &self.state {
            DriveState::Connected => "folder-gdrive".into(),
            DriveState::Connecting | DriveState::Starting => "network-transmit-receive".into(),
            DriveState::NeedsAuth => "dialog-warning".into(),
            DriveState::Disconnected => "drive-harddisk".into(),
            DriveState::Error(_) => "dialog-error".into(),
        }
    }

    fn tool_tip(&self) -> ToolTip {
        let (title, description) = match &self.state {
            DriveState::Connected => (
                "Google Drive: Verbunden".into(),
                format!("Eingebunden unter: {:?}", self.mount_point),
            ),
            DriveState::NeedsAuth => (
                "Google Drive: Anmeldung erforderlich".into(),
                "Die Sitzung ist abgelaufen. Bitte neu anmelden.".into(),
            ),
            DriveState::Connecting => (
                "Google Drive: Verbinde...".into(),
                "Netzlaufwerk wird eingerichtet...".into(),
            ),
            DriveState::Disconnected => (
                "Google Drive: Getrennt".into(),
                "Laufwerk ist nicht eingehängt.".into(),
            ),
            DriveState::Error(err) => (
                "Google Drive: Fehler".into(),
                err.clone(),
            ),
            DriveState::Starting => (
                "Google Drive".into(),
                "Wird gestartet...".into(),
            ),
        };

        ToolTip {
            icon_name: self.icon_name(),
            icon_pixmap: Vec::new(),
            title,
            description,
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        if self.state == DriveState::NeedsAuth {
            let _ = self.cmd_sender.send(TrayCommand::Reauth);
        } else {
            let _ = self.cmd_sender.send(TrayCommand::OpenFolder);
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items = Vec::new();

        if self.state == DriveState::NeedsAuth {
            let sender = self.cmd_sender.clone();
            items.push(
                StandardItem {
                    label: "⚠️ Jetzt im Browser neu anmelden".into(),
                    icon_name: "dialog-warning".into(),
                    activate: Box::new(move |_| {
                        let _ = sender.send(TrayCommand::Reauth);
                    }),
                    ..Default::default()
                }
                .into(),
            );
            items.push(MenuItem::Separator);
        }

        let sender_open = self.cmd_sender.clone();
        items.push(
            StandardItem {
                label: "Google Drive öffnen".into(),
                icon_name: "folder-open".into(),
                activate: Box::new(move |_| {
                    let _ = sender_open.send(TrayCommand::OpenFolder);
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(
            StandardItem {
                label: format!("Status: {}", self.state_label()),
                enabled: false,
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        let sender_rec = self.cmd_sender.clone();
        items.push(
            StandardItem {
                label: "Neu verbinden".into(),
                icon_name: "view-refresh".into(),
                activate: Box::new(move |_| {
                    let _ = sender_rec.send(TrayCommand::Reconnect);
                }),
                ..Default::default()
            }
            .into(),
        );

        let sender_disc = self.cmd_sender.clone();
        items.push(
            StandardItem {
                label: "Laufwerk trennen".into(),
                icon_name: "media-eject".into(),
                activate: Box::new(move |_| {
                    let _ = sender_disc.send(TrayCommand::Disconnect);
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        let sender_auto = self.cmd_sender.clone();
        items.push(
            CheckmarkItem {
                label: "Mit System starten (Autostart)".into(),
                checked: self.autostart_enabled,
                activate: Box::new(move |this: &mut DriveTray| {
                    this.autostart_enabled = !this.autostart_enabled;
                    let _ = sender_auto.send(TrayCommand::ToggleAutostart(this.autostart_enabled));
                }),
                ..Default::default()
            }
            .into(),
        );

        items.push(MenuItem::Separator);

        let sender_quit = self.cmd_sender.clone();
        items.push(
            StandardItem {
                label: "Beenden".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(move |_| {
                    let _ = sender_quit.send(TrayCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}
