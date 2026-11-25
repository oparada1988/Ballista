#!/usr/bin/env bash
set -euo pipefail

# ==============================================================================
# Ballista — Cross-Distro Linux Installer & Uninstaller
# Supports user installs (~/.local) and system-wide installs (/usr/local),
# dedicated StreamController compatibility Python venv, desktop integration,
# and complete 17-PID Elgato Stream Deck udev rules.
# ==============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_ID="io.github.oparada1988.Ballista"

SYSTEM_INSTALL=false
SKIP_DEPS=false
SKIP_UDEV=false
SKIP_BUILD=false
UNINSTALL=false
PURGE=false

# Colors
BOLD="\033[1m"
CYAN="\033[1;36m"
GREEN="\033[1;32m"
YELLOW="\033[1;33m"
RED="\033[1;31m"
RESET="\033[0m"

info()    { echo -e "${CYAN}[Ballista]${RESET} $*"; }
success() { echo -e "${GREEN}[  OK  ]${RESET} $*"; }
warn()    { echo -e "${YELLOW}[ WARN ]${RESET} $*"; }
error()   { echo -e "${RED}[ FAIL ]${RESET} $*" >&2; }

usage() {
    cat <<EOF
Usage: ./install.sh [OPTIONS]

Options:
  --system        Install system-wide into /usr/local (requires sudo/root)
  --skip-deps     Skip checking/installing distro build & runtime packages
  --skip-udev     Skip installing Elgato Stream Deck udev rules
  --skip-build    Skip running 'cargo build --release' (use existing binary)
  --uninstall     Uninstall Ballista (preserves installed plugins & settings)
  --purge         With --uninstall, also remove ~/.local/share/ballista/{plugins,settings}
  -h, --help      Show this help message

Default (no flags): installs for the current user into ~/.local/bin and
~/.local/share/ballista, sets up the standalone StreamController Python bridge
venv, installs the .desktop launcher & icon, and configures udev rules for all
17 Elgato Stream Deck models.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --system)     SYSTEM_INSTALL=true; shift ;;
        --skip-deps)  SKIP_DEPS=true; shift ;;
        --skip-udev)  SKIP_UDEV=true; shift ;;
        --skip-build) SKIP_BUILD=true; shift ;;
        --uninstall)  UNINSTALL=true; shift ;;
        --purge)      PURGE=true; shift ;;
        -h|--help)    usage; exit 0 ;;
        *)
            error "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

if [[ "$SYSTEM_INSTALL" == true ]]; then
    PREFIX="/usr/local"
    BIN_DIR="$PREFIX/bin"
    DATA_DIR="$PREFIX/share/ballista"
    APPS_DIR="$PREFIX/share/applications"
    ICONS_DIR="$PREFIX/share/icons/hicolor"
    SUDO_CMD=""
    if [[ $EUID -ne 0 ]]; then
        SUDO_CMD="sudo"
    fi
else
    PREFIX="${XDG_DATA_HOME:-$HOME/.local/share}"
    BIN_DIR="$HOME/.local/bin"
    DATA_DIR="$PREFIX/ballista"
    APPS_DIR="$PREFIX/applications"
    ICONS_DIR="$PREFIX/icons/hicolor"
    SUDO_CMD=""
fi

# ------------------------------------------------------------------------------
# Uninstall Flow
# ------------------------------------------------------------------------------
if [[ "$UNINSTALL" == true ]]; then
    info "Uninstalling Ballista from ${BIN_DIR} and ${DATA_DIR}..."
    $SUDO_CMD rm -f "$BIN_DIR/ballista"
    $SUDO_CMD rm -rf "$DATA_DIR/bridge" "$DATA_DIR/icons" "$DATA_DIR/venv"
    $SUDO_CMD rm -f "$APPS_DIR/${APP_ID}.desktop"
    $SUDO_CMD rm -f "$ICONS_DIR/scalable/apps/${APP_ID}.svg"
    $SUDO_CMD rm -f "$ICONS_DIR/scalable/apps/store-badge-symbolic.svg"

    if [[ "$PURGE" == true ]]; then
        warn "Purging Ballista plugins, settings, and cache..."
        rm -rf "${XDG_DATA_HOME:-$HOME/.local/share}/ballista"
        rm -rf "${XDG_CACHE_HOME:-$HOME/.cache}/ballista"
        rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/ballista"
    else
        info "Preserved your plugins and settings in ${XDG_DATA_HOME:-$HOME/.local/share}/ballista/{plugins,settings}."
    fi

    if [[ -f /etc/udev/rules.d/70-ballista-streamdeck.rules ]] && [[ "$SKIP_UDEV" == false ]]; then
        if [[ $EUID -eq 0 ]] || command -v sudo >/dev/null 2>&1; then
            info "Removing /etc/udev/rules.d/70-ballista-streamdeck.rules..."
            sudo rm -f /etc/udev/rules.d/70-ballista-streamdeck.rules || true
            sudo udevadm control --reload-rules || true
        fi
    fi

    success "Ballista has been uninstalled."
    exit 0
fi

# ------------------------------------------------------------------------------
# 1. System Dependencies Check / Install
# ------------------------------------------------------------------------------
install_distro_deps() {
    if command -v apt-get >/dev/null 2>&1; then
        info "Detected Debian/Ubuntu (apt). Checking build & runtime dependencies..."
        local py_ver
        py_ver="$(python3 -c 'import sys; print(f"{sys.version_info.major}.{sys.version_info.minor}")' 2>/dev/null || echo "")"
        local pkgs=(
            build-essential pkg-config curl unzip
            libgtk-4-dev libadwaita-1-dev libusb-1.0-0-dev
            python3 python3-venv python3-pip python3-gi python3-gi-cairo
            gir1.2-gtk-4.0 gir1.2-adw-1
        )
        if [[ -n "$py_ver" ]] && apt-cache show "python${py_ver}-venv" >/dev/null 2>&1; then
            pkgs+=("python${py_ver}-venv")
        fi
        local missing=()
        for p in "${pkgs[@]}"; do
            if ! dpkg -s "$p" >/dev/null 2>&1; then
                missing+=("$p")
            fi
        done
        if [[ ${#missing[@]} -gt 0 ]]; then
            info "Installing missing packages: ${missing[*]}"
            sudo apt-get update
            sudo apt-get install -y "${missing[@]}"
        else
            success "All apt build & runtime dependencies are already installed."
        fi
    elif command -v dnf >/dev/null 2>&1; then
        info "Detected Fedora/RHEL (dnf). Ensuring build & runtime dependencies..."
        sudo dnf install -y \
            gcc pkgconf-pkg-config curl unzip \
            gtk4-devel libadwaita-devel libusb1-devel \
            python3 python3-pip python3-gobject gtk4 libadwaita
    elif command -v pacman >/dev/null 2>&1; then
        info "Detected Arch Linux (pacman). Ensuring build & runtime dependencies..."
        sudo pacman -S --needed --noconfirm \
            base-devel pkgconf curl unzip \
            gtk4 libadwaita libusb \
            python python-pip python-gobject
    elif command -v zypper >/dev/null 2>&1; then
        info "Detected openSUSE (zypper). Ensuring build & runtime dependencies..."
        sudo zypper install -y \
            gcc pkg-config curl unzip \
            gtk4-devel libadwaita-devel libusb-1_0-devel \
            python3 python3-pip python3-gobject typelib-1_0-Adw-1 typelib-1_0-Gtk-4_0
    else
        warn "Unrecognized package manager. Ensure GTK4, Libadwaita, libusb-1.0, pkg-config, Python 3, PyGObject (gi), curl, and unzip are installed."
    fi
}

if [[ "$SKIP_DEPS" == false ]]; then
    install_distro_deps
fi

# Verify Rust toolchain if building
if [[ "$SKIP_BUILD" == false ]]; then
    if ! command -v cargo >/dev/null 2>&1; then
        if [[ -f "$HOME/.cargo/env" ]]; then
            # shellcheck source=/dev/null
            source "$HOME/.cargo/env"
        fi
    fi
    if ! command -v cargo >/dev/null 2>&1; then
        error "Rust/Cargo is not installed. Install it via https://rustup.rs and re-run ./install.sh:"
        echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
        exit 1
    fi
fi

# ------------------------------------------------------------------------------
# 2. Build Release Binary
# ------------------------------------------------------------------------------
if [[ "$SKIP_BUILD" == false ]]; then
    info "Building Ballista release binary (with libusb HIDAPI & all 17 Elgato PIDs)..."
    (cd "$SCRIPT_DIR" && cargo build --release)
    success "Built $SCRIPT_DIR/target/release/ballista"
else
    if [[ ! -x "$SCRIPT_DIR/target/release/ballista" ]]; then
        error "--skip-build was specified, but $SCRIPT_DIR/target/release/ballista does not exist."
        exit 1
    fi
fi

# ------------------------------------------------------------------------------
# 3. Install Binary, Compatibility Bridge & Icons
# ------------------------------------------------------------------------------
info "Installing binary to $BIN_DIR/ballista..."
$SUDO_CMD install -Dm755 "$SCRIPT_DIR/target/release/ballista" "$BIN_DIR/ballista"

info "Installing standalone StreamController compatibility bridge to $DATA_DIR/bridge..."
$SUDO_CMD rm -rf "$DATA_DIR/bridge"
$SUDO_CMD mkdir -p "$DATA_DIR/bridge"
$SUDO_CMD cp -r "$SCRIPT_DIR/bridge/." "$DATA_DIR/bridge/"
$SUDO_CMD find "$DATA_DIR/bridge" -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true
$SUDO_CMD chmod +x "$DATA_DIR/bridge/runner.py"

info "Installing icons & desktop entry..."
$SUDO_CMD mkdir -p "$DATA_DIR/icons" "$ICONS_DIR/scalable/apps" "$APPS_DIR"
$SUDO_CMD cp -r "$SCRIPT_DIR/resources/icons/." "$DATA_DIR/icons/"
$SUDO_CMD install -Dm644 \
    "$SCRIPT_DIR/resources/icons/hicolor/scalable/apps/${APP_ID}.svg" \
    "$ICONS_DIR/scalable/apps/${APP_ID}.svg"
$SUDO_CMD install -Dm644 \
    "$SCRIPT_DIR/resources/icons/store-badge-symbolic.svg" \
    "$ICONS_DIR/scalable/apps/store-badge-symbolic.svg"

# Substitute Exec= path if ~/.local/bin might not be in graphical session PATH
DESKTOP_TMP="$(mktemp)"
sed "s|^Exec=ballista$|Exec=$BIN_DIR/ballista|" \
    "$SCRIPT_DIR/resources/${APP_ID}.desktop" > "$DESKTOP_TMP"
$SUDO_CMD install -Dm644 "$DESKTOP_TMP" "$APPS_DIR/${APP_ID}.desktop"
rm -f "$DESKTOP_TMP"

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    $SUDO_CMD gtk-update-icon-cache -f -t "$ICONS_DIR" >/dev/null 2>&1 || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    $SUDO_CMD update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
fi

# Ensure plugin & settings storage directories exist for the invoking user
mkdir -p "${XDG_DATA_HOME:-$HOME/.local/share}/ballista/plugins"
mkdir -p "${XDG_DATA_HOME:-$HOME/.local/share}/ballista/settings/plugins"

# ------------------------------------------------------------------------------
# 4. Provision Standalone Python Bridge Virtual Environment (--system-site-packages)
# ------------------------------------------------------------------------------
VENV_DIR="$DATA_DIR/venv"
info "Configuring Ballista standalone Python bridge runtime at $VENV_DIR..."

VENV_OK=false
if [[ -x "$VENV_DIR/bin/python3" ]] && [[ -f "$VENV_DIR/pyvenv.cfg" ]]; then
    VENV_OK=true
else
    $SUDO_CMD rm -rf "$VENV_DIR"
    if $SUDO_CMD python3 -m venv --system-site-packages "$VENV_DIR" >/dev/null 2>&1; then
        VENV_OK=true
    else
        warn "python3 -m venv (ensurepip) unavailable; falling back to --without-pip venv..."
        $SUDO_CMD rm -rf "$VENV_DIR"
        if $SUDO_CMD python3 -m venv --without-pip --system-site-packages "$VENV_DIR"; then
            VENV_OK=true
        fi
    fi
fi

if [[ "$VENV_OK" == true ]] && [[ -f "$SCRIPT_DIR/resources/requirements-bridge.txt" ]]; then
    info "Installing StreamController plugin runtime packages into $VENV_DIR..."
    if [[ -x "$VENV_DIR/bin/pip" ]]; then
        $SUDO_CMD "$VENV_DIR/bin/pip" install --upgrade --quiet pip >/dev/null 2>&1 || true
        $SUDO_CMD "$VENV_DIR/bin/pip" install --quiet -r "$SCRIPT_DIR/resources/requirements-bridge.txt"
    elif command -v pip3 >/dev/null 2>&1; then
        $SUDO_CMD pip3 --python "$VENV_DIR/bin/python3" install --quiet -r "$SCRIPT_DIR/resources/requirements-bridge.txt"
    elif command -v pip >/dev/null 2>&1; then
        $SUDO_CMD pip --python "$VENV_DIR/bin/python3" install --quiet -r "$SCRIPT_DIR/resources/requirements-bridge.txt"
    else
        warn "pip3 not found; skipping Python bridge package installation (plugins will use system Python packages)."
    fi
fi

# Self-test bridge runtime imports
PYTHON_CHECK="$VENV_DIR/bin/python3"
[[ -x "$PYTHON_CHECK" ]] || PYTHON_CHECK="python3"
if "$PYTHON_CHECK" -c "import gi; gi.require_version('Gtk', '4.0'); gi.require_version('Adw', '1'); from gi.repository import Gtk, Adw; import loguru, PIL, requests, rpyc, websocket, pulsectl" 2>/dev/null; then
    success "Python bridge runtime verified (GTK4, Libadwaita, loguru, Pillow, requests, rpyc, websocket, pulsectl)."
elif "$PYTHON_CHECK" -c "import gi; gi.require_version('Gtk', '4.0'); gi.require_version('Adw', '1'); from gi.repository import Gtk, Adw; import loguru, PIL, requests" 2>/dev/null; then
    success "Core Python bridge runtime verified (GTK4, Libadwaita, loguru, Pillow, requests)."
else
    warn "Some Python runtime imports failed verification — ensure python3-gi, gir1.2-adw-1, and pip packages are installed."
fi

# ------------------------------------------------------------------------------
# 5. Install Complete 17-PID Elgato Stream Deck udev Rules
# ------------------------------------------------------------------------------
if [[ "$SKIP_UDEV" == false ]]; then
    UDEV_SRC="$SCRIPT_DIR/resources/70-ballista-streamdeck.rules"
    UDEV_DEST="/etc/udev/rules.d/70-ballista-streamdeck.rules"
    if [[ -f "$UDEV_SRC" ]]; then
        info "Installing udev rules for all 17 Elgato Stream Deck models to $UDEV_DEST..."
        if [[ $EUID -eq 0 ]]; then
            install -Dm644 "$UDEV_SRC" "$UDEV_DEST"
            udevadm control --reload-rules
            udevadm trigger --subsystem-match=usb --attr-match=idVendor=0fd9 || true
            udevadm trigger --subsystem-match=hidraw || true
            success "udev rules installed and reloaded."
        elif command -v sudo >/dev/null 2>&1; then
            sudo install -Dm644 "$UDEV_SRC" "$UDEV_DEST"
            sudo udevadm control --reload-rules
            sudo udevadm trigger --subsystem-match=usb --attr-match=idVendor=0fd9 || true
            sudo udevadm trigger --subsystem-match=hidraw || true
            success "udev rules installed and reloaded."
        else
            warn "sudo not found — please copy $UDEV_SRC to $UDEV_DEST as root and run: udevadm control --reload-rules && udevadm trigger"
        fi
    fi
fi

# ------------------------------------------------------------------------------
# Done!
# ------------------------------------------------------------------------------
echo ""
success "${BOLD}Ballista installation complete!${RESET}"
echo "  Binary:      $BIN_DIR/ballista"
echo "  Bridge:      $DATA_DIR/bridge/runner.py"
echo "  Python venv: $VENV_DIR/bin/python3"
echo "  Launcher:    $APPS_DIR/${APP_ID}.desktop"
if [[ "$SYSTEM_INSTALL" == false ]] && [[ ":$PATH:" != *":$BIN_DIR:"* ]]; then
    echo ""
    warn "$BIN_DIR is not currently in your PATH. Add this to ~/.bashrc or ~/.zshrc:"
    echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
fi
echo ""
echo "Launch Ballista from your application menu or run: ballista"
