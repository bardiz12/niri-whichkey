#!/usr/bin/env bash
# ==============================================================================
# niri-whichkey Installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/bardiz12/niri-whichkey/main/install.sh | bash
# ==============================================================================

set -euo pipefail

REPO="bardiz12/niri-whichkey"
DEFAULT_INSTALL_DIR="$HOME/.local/bin"
AUTO_YES=false
CUSTOM_VERSION=""
CUSTOM_DIR=""

# Color helpers
if [ -t 1 ]; then
    BOLD="\033[1m"
    GREEN="\033[32m"
    BLUE="\033[34m"
    YELLOW="\033[33m"
    RED="\033[31m"
    RESET="\033[0m"
else
    BOLD=""
    GREEN=""
    BLUE=""
    YELLOW=""
    RED=""
    RESET=""
fi

info() {
    echo -e "${BLUE}${BOLD}[INFO]${RESET} $1"
}

success() {
    echo -e "${GREEN}${BOLD}[SUCCESS]${RESET} $1"
}

warn() {
    echo -e "${YELLOW}${BOLD}[WARNING]${RESET} $1"
}

error() {
    echo -e "${RED}${BOLD}[ERROR]${RESET} $1" >&2
}

prompt_user() {
    local message="$1"
    local default="$2"
    local response=""

    if [ "$AUTO_YES" = true ]; then
        echo "$default"
        return
    fi

    # Read from /dev/tty to support pipes (curl ... | bash)
    if [ -e /dev/tty ]; then
        read -r -p "$message " response < /dev/tty || true
    elif [ -t 0 ]; then
        read -r -p "$message " response || true
    else
        response="$default"
    fi

    if [ -z "$response" ]; then
        response="$default"
    fi
    echo "$response"
}

# Parse command line arguments
while [ $# -gt 0 ]; do
    case "$1" in
        -y|--yes)
            AUTO_YES=true
            shift
            ;;
        --dir)
            CUSTOM_DIR="$2"
            shift 2
            ;;
        --version)
            CUSTOM_VERSION="$2"
            shift 2
            ;;
        -h|--help)
            echo "niri-whichkey installer"
            echo ""
            echo "Usage: install.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  -y, --yes          Non-interactive mode (accept all defaults)"
            echo "      --dir <DIR>    Custom directory to install binary (default: ~/.local/bin)"
            echo "      --version <V>  Specify version to install (default: latest)"
            echo "  -h, --help         Show this help message"
            exit 0
            ;;
        *)
            error "Unknown option: $1"
            exit 1
            ;;
    esac
done

echo -e "${BOLD}=========================================${RESET}"
echo -e "${BOLD}       niri-whichkey Installer           ${RESET}"
echo -e "${BOLD}=========================================${RESET}"

# 1. Architecture & OS check
OS="$(uname -s)"
ARCH="$(uname -m)"

if [ "$OS" != "Linux" ]; then
    error "niri-whichkey currently only supports Linux. Detected: $OS"
    exit 1
fi

if [ "$ARCH" != "x86_64" ]; then
    error "Pre-built binaries are currently only available for x86_64. Detected: $ARCH"
    echo "You can still build from source: cargo install --git https://github.com/$REPO"
    exit 1
fi

# 2. Check prerequisites
if ! command -v curl >/dev/null 2>&1 && ! command -v wget >/dev/null 2>&1; then
    error "Neither curl nor wget is installed. Please install one of them to proceed."
    exit 1
fi

if ! command -v tar >/dev/null 2>&1; then
    error "tar is required but not installed."
    exit 1
fi

download_file() {
    local url="$1"
    local dest="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fSL "$url" -o "$dest"
    else
        wget -qO "$dest" "$url"
    fi
}

fetch_text() {
    local url="$1"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url"
    else
        wget -qO- "$url"
    fi
}

# 3. Determine target version
TARGET_TAG="$CUSTOM_VERSION"
if [ -z "$TARGET_TAG" ]; then
    info "Fetching latest release information from GitHub..."
    LATEST_JSON=$(fetch_text "https://api.github.com/repos/$REPO/releases/latest" 2>/dev/null || true)
    
    if [ -n "$LATEST_JSON" ]; then
        TARGET_TAG=$(echo "$LATEST_JSON" | grep -m 1 '"tag_name":' | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/' || true)
    fi
    
    if [ -z "$TARGET_TAG" ]; then
        warn "Could not determine latest version from GitHub API. Falling back to default release URL."
        TARGET_TAG="v0.1.2"
    fi
fi

# Ensure tag starts with 'v'
if [[ "$TARGET_TAG" != v* ]]; then
    TARGET_TAG="v$TARGET_TAG"
fi
TARGET_VER_CLEAN="${TARGET_TAG#v}"

info "Target version: ${BOLD}$TARGET_TAG${RESET}"

# 4. Check for existing installation
EXISTING_BIN="$(command -v niri-whichkey 2>/dev/null || true)"
if [ -z "$EXISTING_BIN" ] && [ -x "$DEFAULT_INSTALL_DIR/niri-whichkey" ]; then
    EXISTING_BIN="$DEFAULT_INSTALL_DIR/niri-whichkey"
fi

if [ -n "$EXISTING_BIN" ]; then
    CURRENT_RAW="$("$EXISTING_BIN" --version 2>/dev/null || echo "")"
    CURRENT_VER=$(echo "$CURRENT_RAW" | awk '{print $2}' || true)
    CURRENT_VER_CLEAN="${CURRENT_VER#v}"

    if [ -n "$CURRENT_VER_CLEAN" ]; then
        info "Found existing installation at ${BOLD}$EXISTING_BIN${RESET} (version: ${BOLD}v$CURRENT_VER_CLEAN${RESET})"
        
        if [ "$CURRENT_VER_CLEAN" = "$TARGET_VER_CLEAN" ]; then
            echo ""
            REINSTALL_PROMPT=$(prompt_user "niri-whichkey is already up to date (v$CURRENT_VER_CLEAN). Reinstall anyway? [y/N]:" "n")
            case "$REINSTALL_PROMPT" in
                [yY]|[yY][eE][sS])
                    info "Reinstalling v$TARGET_VER_CLEAN..."
                    ;;
                *)
                    success "Nothing to do. Exiting."
                    exit 0
                    ;;
            esac
        else
            echo ""
            UPGRADE_PROMPT=$(prompt_user "Upgrade niri-whichkey from v$CURRENT_VER_CLEAN to $TARGET_TAG? [Y/n]:" "y")
            case "$UPGRADE_PROMPT" in
                [nN]|[nN][oO])
                    info "Upgrade cancelled by user. Exiting."
                    exit 0
                    ;;
                *)
                    info "Upgrading to $TARGET_TAG..."
                    ;;
            esac
        fi
    fi
fi

# 5. Determine install directory
if [ -n "$CUSTOM_DIR" ]; then
    INSTALL_DIR="$CUSTOM_DIR"
elif [ -n "$EXISTING_BIN" ]; then
    INSTALL_DIR="$(dirname "$EXISTING_BIN")"
else
    INSTALL_DIR="$DEFAULT_INSTALL_DIR"
fi

# 6. Download and install
TMP_DIR=$(mktemp -d)
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

ARCHIVE_NAME="niri-whichkey-${TARGET_TAG}-x86_64-linux.tar.gz"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/${TARGET_TAG}/${ARCHIVE_NAME}"

info "Downloading ${DOWNLOAD_URL}..."
if ! download_file "$DOWNLOAD_URL" "$TMP_DIR/$ARCHIVE_NAME"; then
    error "Failed to download $DOWNLOAD_URL"
    exit 1
fi

info "Extracting archive..."
tar -xzf "$TMP_DIR/$ARCHIVE_NAME" -C "$TMP_DIR"

if [ ! -f "$TMP_DIR/niri-whichkey" ]; then
    error "Extracted archive does not contain 'niri-whichkey' binary."
    exit 1
fi

# Check permissions for install directory
SUDO_CMD=""
if [ ! -d "$INSTALL_DIR" ]; then
    mkdir -p "$INSTALL_DIR" 2>/dev/null || SUDO_CMD="sudo"
fi

if [ -n "$SUDO_CMD" ] || [ ! -w "$INSTALL_DIR" ]; then
    if command -v sudo >/dev/null 2>&1; then
        info "Root permissions required to install to $INSTALL_DIR (running sudo)..."
        SUDO_CMD="sudo"
        $SUDO_CMD mkdir -p "$INSTALL_DIR"
        $SUDO_CMD install -m 755 "$TMP_DIR/niri-whichkey" "$INSTALL_DIR/niri-whichkey"
    else
        error "Directory $INSTALL_DIR is not writable and sudo is not available."
        exit 1
    fi
else
    install -m 755 "$TMP_DIR/niri-whichkey" "$INSTALL_DIR/niri-whichkey"
fi

success "Installed niri-whichkey to ${BOLD}$INSTALL_DIR/niri-whichkey${RESET}"

# 7. Check PATH
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        warn "Directory $INSTALL_DIR is not in your \$PATH."
        echo "  Add it by running:"
        echo "    export PATH=\"$INSTALL_DIR:\$PATH\""
        echo "  or add it to your ~/.bashrc / ~/.zshrc."
        ;;
esac

# 8. Check input group membership
echo ""
info "Checking input permissions..."
if id -nG | grep -qw "input"; then
    success "User '$USER' is a member of the 'input' group."
else
    warn "User '$USER' is NOT in the 'input' group!"
    echo "  niri-whichkey requires read access to /dev/input/event* to detect modifier keys."
    echo "  Please run:"
    echo -e "    ${BOLD}sudo usermod -aG input $USER${RESET}"
    echo "  and then log out and log back in."
fi

# 9. Configure spawn-at-startup in Niri
echo ""
info "Checking Niri configuration..."
NIRI_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/niri/config.kdl"

if [ -f "$NIRI_CONFIG" ]; then
    if grep -qE '^[[:space:]]*spawn-at-startup[[:space:]]+.*"niri-whichkey"' "$NIRI_CONFIG"; then
        success "'spawn-at-startup \"niri-whichkey\"' is already configured in $NIRI_CONFIG."
    else
        echo ""
        AUTORUN_PROMPT=$(prompt_user "Add 'spawn-at-startup \"niri-whichkey\"' to $NIRI_CONFIG? [Y/n]:" "y")
        case "$AUTORUN_PROMPT" in
            [nN]|[nN][oO])
                info "Skipped modifying $NIRI_CONFIG."
                ;;
            *)
                # Backup existing config
                cp "$NIRI_CONFIG" "$NIRI_CONFIG.bak"
                # Append spawn-at-startup
                cat << 'EOF' >> "$NIRI_CONFIG"

// Automatically launch niri-whichkey overlay daemon
spawn-at-startup "niri-whichkey"
EOF
                success "Added 'spawn-at-startup \"niri-whichkey\"' to $NIRI_CONFIG (backup saved as $NIRI_CONFIG.bak)."
                ;;
        esac
    fi
else
    info "No niri config found at $NIRI_CONFIG."
    echo "  To launch niri-whichkey automatically on startup, add the following to your config.kdl:"
    echo -e "    ${BOLD}spawn-at-startup \"niri-whichkey\"${RESET}"
fi

echo ""
echo -e "${GREEN}${BOLD}✨ Installation complete!${RESET}"
echo "Run 'niri-whichkey' to start, or reload niri if configured with spawn-at-startup."
