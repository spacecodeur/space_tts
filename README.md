# Space STT — Local Speech-to-Text Terminal Injector

> **Note:** This project was largely written through vibecoding with an LLM (Claude).

Single Rust binary that captures microphone audio, transcribes speech locally via whisper.cpp, and injects text into the focused window using dotool. Designed for Linux (Wayland/X11).

## Features

- **Local & private** — everything runs on your machine, no cloud API
- **CUDA GPU acceleration** — optional, for fast transcription on NVIDIA GPUs
- **Push-to-talk hotkey** — toggle recording with a configurable key (F2–F12, ScrollLock, Pause)
- **Voice Activity Detection** — automatically segments speech from silence
- **Whisper hallucination filtering** — strips phantom "Merci d'avoir regard la vid o" artifacts
- **Auto-detected XKB layout** — accented characters work out of the box (e.g. `us+altgr-intl`)
- **TUI setup** — interactive model and hotkey selection at startup

## Quick Setup

An automated setup script handles dependencies, dotool, model download, and build:

```bash
./setup.sh install     # install everything
./setup.sh uninstall   # remove everything cleanly
```

The script auto-detects your package manager (dnf, apt, pacman).

## Manual Setup

### Fedora (dnf)

```bash
# Build dependencies
sudo dnf install -y cmake gcc gcc-c++ pkg-config alsa-lib-devel

# dotool (text injection — build from source)
sudo dnf install -y golang libxkbcommon-devel scdoc
git clone https://git.sr.ht/~geb/dotool && cd dotool
./build.sh && sudo ./build.sh install

# Permissions (needed for evdev hotkey AND dotool uinput)
sudo usermod -aG input $USER
# Log out and back in for group change to take effect

# CUDA (optional — GPU acceleration)
sudo dnf install -y cuda-nvcc cuda-cudart-devel cuda-cudart-static cuda-culibos-devel cuda-cccl-devel
```

### Debian / Ubuntu (apt)

```bash
# Build dependencies
sudo apt install -y cmake gcc g++ pkg-config libasound2-dev

# dotool (text injection — build from source)
sudo apt install -y golang libxkbcommon-dev scdoc
git clone https://git.sr.ht/~geb/dotool && cd dotool
./build.sh && sudo ./build.sh install

# Permissions (needed for evdev hotkey AND dotool uinput)
sudo usermod -aG input $USER
# Log out and back in for group change to take effect

# CUDA (optional — GPU acceleration)
# Add NVIDIA repo first: https://developer.nvidia.com/cuda-downloads
sudo apt install -y nvidia-cuda-toolkit libcublas-dev
```

### Whisper Models

Download at least one model into the `models/` directory at the project root:

```bash
mkdir -p models
# Pick one:
wget -P models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin    # ~75 MB  — laptop CPU
wget -P models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin    # ~142 MB — CPU
wget -P models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin   # ~466 MB — mid-range GPU
wget -P models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin  # ~1.5 GB — strong GPU
wget -P models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin # ~3.1 GB — best quality
```

## Build

```bash
# CPU only (default)
cargo build --release

# With CUDA GPU acceleration
cargo build --release --features cuda
```

## Run

```bash
cargo run --release
# or with CUDA:
cargo run --release --features cuda
```
