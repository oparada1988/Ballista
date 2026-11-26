use std::fs;
use std::path::PathBuf;
use std::process::Command;

const SERVICE_NAME: &str = "ballista.service";
const DESKTOP_ID: &str = "io.github.oparada1988.Ballista.desktop";

fn systemd_service_path() -> PathBuf {
    glib::user_config_dir()
        .join("systemd")
        .join("user")
        .join(SERVICE_NAME)
}

fn xdg_autostart_path() -> PathBuf {
    glib::user_config_dir().join("autostart").join(DESKTOP_ID)
}

/// Resolves the best Ballista executable path for login startup
pub fn resolve_ballista_exec() -> String {
    if let Ok(exe) = std::env::current_exe() {
        let s = exe.to_string_lossy().to_string();
        // Prefer installed binary if it exists rather than a temporary target/debug path
        if let Ok(home) = std::env::var("HOME") {
            let user_bin = PathBuf::from(&home).join(".local/bin/ballista");
            if user_bin.exists() {
                return user_bin.to_string_lossy().to_string();
            }
        }
        let sys_bin = PathBuf::from("/usr/local/bin/ballista");
        if sys_bin.exists() {
            return sys_bin.to_string_lossy().to_string();
        }
        return s;
    }
    "ballista".into()
}

/// Checks if `systemd --user` with `graphical-session.target` is active on this desktop
pub fn has_systemd_graphical_session() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "graphical-session.target"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Returns true if Ballista login startup is currently enabled (via systemd user service or XDG autostart)
pub fn is_autostart_enabled() -> bool {
    if systemd_service_path().exists() {
        if let Ok(out) = Command::new("systemctl")
            .args(["--user", "is-enabled", SERVICE_NAME])
            .output()
        {
            if out.status.success() {
                return true;
            }
        }
    }
    xdg_autostart_path().exists()
}

/// Enables or disables instant login startup (prefers `systemd --user` bound to
/// `graphical-session.target` for fastest boot & crash recovery, falling back to XDG Autostart)
pub fn set_autostart_enabled(enabled: bool) -> Result<(), String> {
    let svc_path = systemd_service_path();
    let xdg_path = xdg_autostart_path();

    if !enabled {
        // Disable both cleanly so no stale entry remains
        let _ = Command::new("systemctl")
            .args(["--user", "disable", SERVICE_NAME])
            .status();
        let _ = fs::remove_file(&svc_path);
        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        let _ = fs::remove_file(&xdg_path);
        tracing::info!("Disabled Ballista login autostart");
        return Ok(());
    }

    let exec_bin = resolve_ballista_exec();

    if has_systemd_graphical_session() {
        // Remove any duplicate XDG autostart entry when managed by systemd --user
        let _ = fs::remove_file(&xdg_path);

        if let Some(parent) = svc_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create systemd user dir: {e}"))?;
        }

        let unit = format!(
            "[Unit]\n\
             Description=Ballista - Elgato Stream Deck Background Service\n\
             PartOf=graphical-session.target\n\
             After=graphical-session.target\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart=\"{exec_bin}\" --background\n\
             Restart=on-failure\n\
             RestartSec=2\n\
             \n\
             [Install]\n\
             WantedBy=graphical-session.target\n"
        );
        fs::write(&svc_path, unit)
            .map_err(|e| format!("Failed to write {}: {e}", svc_path.display()))?;

        let _ = Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        let status = Command::new("systemctl")
            .args(["--user", "enable", SERVICE_NAME])
            .status()
            .map_err(|e| format!("Failed to run systemctl --user enable: {e}"))?;
        if !status.success() {
            return Err("systemctl --user enable ballista.service failed".into());
        }
        tracing::info!(
            "Enabled Ballista systemd user login service: {}",
            svc_path.display()
        );
        Ok(())
    } else {
        // Universal XDG Autostart fallback for non-systemd / minimal WM sessions
        if let Some(parent) = xdg_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create autostart dir: {e}"))?;
        }
        let desktop = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Ballista\n\
             Comment=Elgato Stream Deck background service\n\
             Exec=\"{exec_bin}\" --background\n\
             Icon=io.github.oparada1988.Ballista\n\
             Terminal=false\n\
             X-GNOME-Autostart-enabled=true\n\
             X-GNOME-Autostart-Delay=0\n"
        );
        fs::write(&xdg_path, desktop)
            .map_err(|e| format!("Failed to write {}: {e}", xdg_path.display()))?;
        tracing::info!("Enabled Ballista XDG autostart: {}", xdg_path.display());
        Ok(())
    }
}
