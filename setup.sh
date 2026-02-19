#!/usr/bin/env bash
set -euo pipefail

MODELS_DIR="$(cd "$(dirname "$0")" && pwd)/models"
DOTOOL_REPO="https://git.sr.ht/~geb/dotool"
HF_BASE="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"

# --- Helpers ---

info()  { printf '\033[1;34m[INFO]\033[0m  %s\n' "$*"; }
warn()  { printf '\033[1;33m[WARN]\033[0m  %s\n' "$*"; }
err()   { printf '\033[1;31m[ERROR]\033[0m %s\n' "$*"; exit 1; }
ask()   { printf '\033[1;36m[?]\033[0m %s ' "$1"; read -r REPLY; }

detect_pkg_manager() {
    if command -v dnf &>/dev/null; then
        echo "dnf"
    elif command -v apt-get &>/dev/null; then
        echo "apt"
    elif command -v pacman &>/dev/null; then
        echo "pacman"
    else
        err "No supported package manager found (dnf, apt, pacman)."
    fi
}

project_dir() {
    cd "$(dirname "$0")" && pwd
}

# --- System packages ---

install_client_deps() {
    local pm="$1"
    info "Installing client build dependencies ($pm)..."

    case "$pm" in
        dnf)
            sudo dnf install -y pkg-config alsa-lib-devel
            ;;
        apt)
            sudo apt-get update
            sudo apt-get install -y pkg-config libasound2-dev
            ;;
        pacman)
            sudo pacman -S --needed --noconfirm pkgconf alsa-lib
            ;;
    esac
}

install_server_deps() {
    local pm="$1"
    info "Installing server build dependencies ($pm)..."

    case "$pm" in
        dnf)
            sudo dnf install -y cmake gcc gcc-c++ pkg-config
            ;;
        apt)
            sudo apt-get update
            sudo apt-get install -y cmake gcc g++ pkg-config
            ;;
        pacman)
            sudo pacman -S --needed --noconfirm cmake gcc pkgconf
            ;;
    esac
}

install_build_deps() {
    local pm="$1"
    info "Installing all build dependencies ($pm)..."

    case "$pm" in
        dnf)
            sudo dnf install -y cmake gcc gcc-c++ pkg-config alsa-lib-devel
            ;;
        apt)
            sudo apt-get update
            sudo apt-get install -y cmake gcc g++ pkg-config libasound2-dev
            ;;
        pacman)
            sudo pacman -S --needed --noconfirm cmake gcc pkgconf alsa-lib
            ;;
    esac
}

install_dotool_deps() {
    local pm="$1"
    info "Installing dotool build dependencies ($pm)..."

    case "$pm" in
        dnf)
            sudo dnf install -y golang libxkbcommon-devel scdoc
            ;;
        apt)
            sudo apt-get install -y golang libxkbcommon-dev scdoc
            ;;
        pacman)
            sudo pacman -S --needed --noconfirm go libxkbcommon scdoc
            ;;
    esac
}

install_cuda() {
    local pm="$1"
    info "Installing CUDA toolkit ($pm)..."

    case "$pm" in
        dnf)
            sudo dnf install -y cuda-nvcc cuda-cudart-devel cuda-cudart-static cuda-culibos-devel cuda-cccl-devel
            ;;
        apt)
            info "Make sure the NVIDIA CUDA repo is configured first."
            info "See: https://developer.nvidia.com/cuda-downloads"
            ask "Continue with apt install? [y/N]"
            [[ "$REPLY" =~ ^[yY]$ ]] || return 0
            sudo apt-get install -y nvidia-cuda-toolkit libcublas-dev
            ;;
        pacman)
            sudo pacman -S --needed --noconfirm cuda
            ;;
    esac
}

# --- dotool ---

install_dotool() {
    if command -v dotool &>/dev/null; then
        info "dotool already installed: $(command -v dotool)"
        return 0
    fi

    info "Building and installing dotool from source..."
    local tmpdir
    tmpdir=$(mktemp -d)
    git clone "$DOTOOL_REPO" "$tmpdir/dotool"
    cd "$tmpdir/dotool"
    ./build.sh
    sudo ./build.sh install
    cd - >/dev/null
    rm -rf "$tmpdir"
    info "dotool installed."
}

uninstall_dotool() {
    if ! command -v dotool &>/dev/null; then
        info "dotool not found, nothing to remove."
        return 0
    fi

    info "Removing dotool..."
    # dotool installs to /usr/local/bin by default
    for f in /usr/local/bin/dotool /usr/local/bin/dotoold; do
        if [ -f "$f" ]; then
            sudo rm -f "$f"
            info "Removed $f"
        fi
    done
    # Man page
    for f in /usr/local/share/man/man1/dotool.1 /usr/local/share/man/man1/dotoold.1; do
        if [ -f "$f" ]; then
            sudo rm -f "$f"
            info "Removed $f"
        fi
    done
}

# --- Rust ---

install_rust() {
    if command -v cargo &>/dev/null; then
        info "Rust already installed: $(rustc --version)"
        return 0
    fi

    info "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
    info "Rust installed: $(rustc --version)"
}

# --- User permissions ---

setup_input_group() {
    if id -nG "$USER" | grep -qw input; then
        info "User '$USER' is already in the 'input' group."
    else
        info "Adding user '$USER' to the 'input' group..."
        sudo usermod -aG input "$USER"
        warn "You will need to log out and back in for the group change to take effect."
    fi
}

# --- Whisper model ---

download_model() {
    mkdir -p "$MODELS_DIR"

    local existing
    existing=$(find "$MODELS_DIR" -name 'ggml-*.bin' 2>/dev/null | wc -l)
    if [ "$existing" -gt 0 ]; then
        info "Found $existing model(s) in $MODELS_DIR:"
        ls -lh "$MODELS_DIR"/ggml-*.bin 2>/dev/null
        echo
        ask "Download another model? [y/N]"
        [[ "$REPLY" =~ ^[yY]$ ]] || return 0
    fi

    echo
    echo "Available Whisper models:"
    echo "  1) tiny      (~75 MB)   — laptop CPU"
    echo "  2) base      (~142 MB)  — CPU"
    echo "  3) small     (~466 MB)  — mid-range GPU"
    echo "  4) medium    (~1.5 GB)  — strong GPU"
    echo "  5) large-v3  (~3.1 GB)  — best quality"
    echo
    ask "Select model [1-5]:"

    local model_name
    case "$REPLY" in
        1) model_name="ggml-tiny.bin" ;;
        2) model_name="ggml-base.bin" ;;
        3) model_name="ggml-small.bin" ;;
        4) model_name="ggml-medium.bin" ;;
        5) model_name="ggml-large-v3.bin" ;;
        *) warn "Invalid choice, skipping model download."; return 0 ;;
    esac

    if [ -f "$MODELS_DIR/$model_name" ]; then
        info "$model_name already exists, skipping."
        return 0
    fi

    info "Downloading $model_name..."
    wget -P "$MODELS_DIR" "$HF_BASE/$model_name"
    info "Model saved to $MODELS_DIR/$model_name"
}

# --- Build ---

build_project() {
    local target="${1:-}"
    local dir
    dir=$(project_dir)

    if [ ! -f "$dir/Cargo.toml" ]; then
        err "Cargo.toml not found in $dir."
    fi

    cd "$dir"

    case "$target" in
        client)
            info "Building space_tts_client..."
            cargo build --release -p space_tts_client
            info "Build complete: $dir/target/release/space_tts_client"
            ;;
        server)
            local build_flags="--release -p space_tts_server"
            if [ "${CUDA_ENABLED:-0}" = "1" ]; then
                build_flags="$build_flags --features cuda"
            fi
            info "Building space_tts_server ($build_flags)..."
            cargo build $build_flags
            info "Build complete: $dir/target/release/space_tts_server"
            ;;
        *)
            local build_flags="--release --workspace"
            if [ "${CUDA_ENABLED:-0}" = "1" ]; then
                build_flags="$build_flags --features space_tts_server/cuda"
            fi
            info "Building workspace ($build_flags)..."
            cargo build $build_flags
            info "Build complete."
            ;;
    esac
}

# --- Uninstall ---

do_uninstall() {
    echo "========================================="
    echo "  Space STT — Uninstall"
    echo "========================================="
    echo

    local dir
    dir=$(project_dir)

    # 1. Remove build artifacts
    if [ -d "$dir/target" ]; then
        ask "Remove build artifacts ($dir/target)? [Y/n]"
        if [[ ! "$REPLY" =~ ^[nN]$ ]]; then
            rm -rf "$dir/target"
            info "Build artifacts removed."
        fi
    fi

    # 2. Remove Whisper models
    if [ -d "$MODELS_DIR" ]; then
        local model_size
        model_size=$(du -sh "$MODELS_DIR" 2>/dev/null | cut -f1)
        ask "Remove Whisper models ($MODELS_DIR, $model_size)? [y/N]"
        if [[ "$REPLY" =~ ^[yY]$ ]]; then
            rm -rf "$MODELS_DIR"
            # Remove parent dir if empty
            rmdir --ignore-fail-on-non-empty "$(dirname "$MODELS_DIR")" 2>/dev/null || true
            info "Models removed."
        fi
    fi

    # 3. Remove dotool
    ask "Remove dotool? [y/N]"
    if [[ "$REPLY" =~ ^[yY]$ ]]; then
        uninstall_dotool
    fi

    # 4. Remove user from input group
    if id -nG "$USER" | grep -qw input; then
        ask "Remove user '$USER' from the 'input' group? [y/N]"
        if [[ "$REPLY" =~ ^[yY]$ ]]; then
            sudo gpasswd -d "$USER" input
            info "User removed from 'input' group (takes effect after logout)."
        fi
    fi

    # 5. Remove system packages
    local pm
    pm=$(detect_pkg_manager)
    ask "Remove system build dependencies ($pm)? [y/N]"
    if [[ "$REPLY" =~ ^[yY]$ ]]; then
        case "$pm" in
            dnf)
                sudo dnf remove -y alsa-lib-devel libxkbcommon-devel scdoc
                ;;
            apt)
                sudo apt-get remove -y libasound2-dev libxkbcommon-dev scdoc
                ;;
            pacman)
                sudo pacman -Rs --noconfirm alsa-lib scdoc 2>/dev/null || true
                ;;
        esac
        info "Build dependencies removed."

        # CUDA packages
        ask "Also remove CUDA packages? [y/N]"
        if [[ "$REPLY" =~ ^[yY]$ ]]; then
            case "$pm" in
                dnf)
                    sudo dnf remove -y cuda-nvcc cuda-cudart-devel cuda-cudart-static cuda-culibos-devel cuda-cccl-devel 2>/dev/null || true
                    ;;
                apt)
                    sudo apt-get remove -y nvidia-cuda-toolkit libcublas-dev 2>/dev/null || true
                    ;;
                pacman)
                    sudo pacman -Rs --noconfirm cuda 2>/dev/null || true
                    ;;
            esac
            info "CUDA packages removed."
        fi
    fi

    echo
    echo "========================================="
    info "Uninstall complete."
    echo "========================================="
}

# --- Install ---

do_install_client() {
    echo "========================================="
    echo "  Space TTS — Client Setup"
    echo "========================================="
    echo

    local pm
    pm=$(detect_pkg_manager)
    info "Detected package manager: $pm"
    echo

    install_rust
    install_client_deps "$pm"
    install_dotool_deps "$pm"
    install_dotool
    setup_input_group

    echo
    build_project client

    echo
    echo "========================================="
    info "Client setup complete!"
    echo
    echo "  Run:  ./target/release/space_tts_client"
    echo
    if ! id -nG "$USER" | grep -qw input; then
        warn "Remember to log out/in for the 'input' group to take effect."
    fi
    echo "========================================="
}

do_install_server() {
    echo "========================================="
    echo "  Space TTS — Server Setup"
    echo "========================================="
    echo

    local pm
    pm=$(detect_pkg_manager)
    info "Detected package manager: $pm"
    echo

    install_rust
    install_server_deps "$pm"

    echo
    ask "Install CUDA support for GPU acceleration? [y/N]"
    if [[ "$REPLY" =~ ^[yY]$ ]]; then
        install_cuda "$pm"
        CUDA_ENABLED=1
    fi

    echo
    download_model

    echo
    build_project server

    echo
    echo "========================================="
    info "Server setup complete!"
    echo
    echo "  Run:  ./target/release/space_tts_server --list-models"
    echo "========================================="
}

do_install() {
    echo "========================================="
    echo "  Space TTS — Full Setup"
    echo "========================================="
    echo

    local pm
    pm=$(detect_pkg_manager)
    info "Detected package manager: $pm"
    echo

    install_rust
    install_build_deps "$pm"
    install_dotool_deps "$pm"
    install_dotool
    setup_input_group

    echo
    ask "Install CUDA support for GPU acceleration? [y/N]"
    if [[ "$REPLY" =~ ^[yY]$ ]]; then
        install_cuda "$pm"
        CUDA_ENABLED=1
    fi

    echo
    download_model

    echo
    build_project

    echo
    echo "========================================="
    info "Setup complete!"
    echo
    echo "  Client:  ./target/release/space_tts_client"
    echo "  Server:  ./target/release/space_tts_server --list-models"
    echo
    if ! id -nG "$USER" | grep -qw input; then
        warn "Remember to log out/in for the 'input' group to take effect."
    fi
    echo "========================================="
}

# --- Entry point ---

usage() {
    echo "Usage: $0 <command> [target]"
    echo
    echo "Commands:"
    echo "  install           Install everything (client + server)"
    echo "  install client    Install client only (hotkey, audio, dotool)"
    echo "  install server    Install server only (whisper, CUDA, models)"
    echo "  uninstall         Remove everything cleanly"
    echo "  model             Download a Whisper model"
}

case "${1:-}" in
    install)
        case "${2:-}" in
            client)  do_install_client ;;
            server)  do_install_server ;;
            "")      do_install ;;
            *)       usage ;;
        esac
        ;;
    uninstall)
        do_uninstall
        ;;
    model)
        download_model
        ;;
    *)
        usage
        ;;
esac
