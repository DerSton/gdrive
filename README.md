# gdrive – Google Drive Tray Microservice

Eine schlanke, extrem robuste Single-Binary Desktop-App für Linux (optimiert für Fedora / KDE Plasma / GNOME) und Windows.
Sie bindet Google Drive beim Login automatisch als lokales Netzlaufwerk ein, läuft unauffällig als Icon im System-Tray („unten rechts“), speichert OAuth-Zugangsdaten sicher ab und fragt bei abgelaufenen Sessions automatisch per Prompt mit einem Klick-Button nach erneuter Anmeldung im Browser.

---

## Features

- **Single Binary**: Eine einzige ausführbare Datei (`target/release/gdrive`), die direkt in den Autostart gelegt werden kann.
- **Embedded FUSE Engine**: Beinhaltet die battle-tested `rclone`-Mount-Engine mit vollem VFS-Cache (`--vfs-cache-mode full`). Dateien verhalten sich wie auf einer echten lokalen Festplatte.
- **Nativer System Tray („unten rechts“)**:
  - KDE Plasma / FreeDesktop StatusNotifierItem (via reinem Rust DBus `ksni`).
  - Zeigt Status an: 🟢 *Verbunden*, 🟡 *Verbinde...*, ⚠️ *Anmeldung erforderlich*, 🔴 *Getrennt*.
  - Linksklick: Öffnet den Google Drive Ordner direkt im Standard-Dateimanager (z. B. Dolphin).
  - Rechtsklick-Menü: Öffnen, Status, Neu verbinden, Trennen, Re-Auth, Autostart-Umschalter, Beenden.
- **OAuth2 Lifecycle & Re-Auth Prompt**:
  - Fragt beim Erststart nach `Client ID` & `Client Secret`.
  - Startet temporären Callback-Server auf `127.0.0.1:53682` und öffnet den Browser.
  - Überwacht Token-Ablauf im Hintergrund.
  - Wenn Google den Token widerruft oder er abläuft: Ein Prompt/Dialog erscheint automatisch mit dem Button **[Im Browser anmelden]**. Ein Klick öffnet den Browser, erneuert die Session und bindet das Laufwerk nahtlos wieder ein.
- **Sichere Konfiguration**: Gespeichert unter `~/.config/gdrive/config.json` mit Dateirechten `0600` (nur für den aktuellen Nutzer lesbar).
- **Autostart**: Ein Klick im Tray aktiviert oder deaktiviert den Autostart (`~/.config/autostart/gdrive.desktop`).
- **Sauberes Unmount**: Fängt SIGINT / SIGTERM ab und unmountet das FUSE-Laufwerk via `fusermount3 -u`, um Hänger im Dateimanager zu verhindern.

---

## Schnellstart

### 1. Bauen
```bash
cargo build --release
```
Die fertige Binärdatei liegt unter `target/release/gdrive` (ca. 33 MB inklusive aller FUSE-Engines).

### 2. Google OAuth Credentials erstellen (Einmalig in 2 Minuten)
Google Drive verlangt für Third-Party-Clients eigene API-Zugangsdaten:
1. Rufe die [Google Cloud Console](https://console.cloud.google.com/apis/credentials) auf.
2. Erstelle ein neues Projekt (z. B. "MyGDrive").
3. Gehe zu **APIs & Dienste** > **Bibliothek** und aktiviere die **Google Drive API**.
4. Gehe zu **APIs & Dienste** > **OAuth-Zustimmungsbildschirm**, wähle **Extern** und gib einen Namen ein (z. B. "GDrive App"). Füge deine eigene Gmail-Adresse unter Testnutzer hinzu.
5. Gehe zu **Anmeldedaten** > **Anmeldedaten erstellen** > **OAuth-Client-ID**:
   - Anwendungstyp: **Desktop-App**
   - Name: **gdrive**
6. Kopiere die angezeigte **Client-ID** und das **Client-Secret**.

### 3. Starten
```bash
./target/release/gdrive
```
Beim ersten Start öffnet sich ein Dialog zur Eingabe von Client-ID und Secret, danach öffnet sich dein Standardbrowser für den Google-Login. Nach der Bestätigung ist das Laufwerk sofort unter `~/GoogleDrive` eingebunden und das Tray-Icon erscheint unten rechts.

---

## Befehle & Optionen

- **Standardstart (Tray Daemon)**:
  ```bash
  ./target/release/gdrive
  ```
- **Ersteinrichtung / Credentials neu eingeben**:
  ```bash
  ./target/release/gdrive --setup
  ```
- **Laufwerk manuell aushängen**:
  ```bash
  ./target/release/gdrive --unmount
  ```

---

## Autostart in Fedora / KDE Plasma

Die App kann den Autostart direkt selbst verwalten:
- Einfach im Tray-Menü das Häkchen bei **„Mit System starten (Autostart)“** setzen.
- Alternativ kann die Binärdatei nach `~/.local/bin/gdrive` kopiert und in KDE Plasma unter **Systemeinstellungen > Starten und Beenden > Autostart** hinzugefügt werden.
