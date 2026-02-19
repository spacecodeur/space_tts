# Space STT — Speech-to-Text Terminal Injector

> **Note:** This project was largely written through vibecoding with an LLM (Claude).

Single Rust binary that captures microphone audio, transcribes speech via whisper.cpp, and injects text into the focused window using dotool. Designed for Linux (Wayland/X11). Supports local transcription or remote transcription via SSH sur un serveur GPU.

## Features

- **Local & remote** — transcription locale ou distante via SSH sur un serveur GPU
- **CUDA GPU acceleration** — optional, for fast transcription on NVIDIA GPUs
- **Push-to-talk hotkey** — toggle recording with a configurable key (F2–F12, ScrollLock, Pause)
- **Voice Activity Detection** — automatically segments speech from silence
- **Whisper hallucination filtering** — strips phantom "Merci d'avoir regardé la vidéo" artifacts
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

## Usage

### Mode local (tout-en-un)

Le mode par défaut : capture audio, transcription et injection sur la même machine.

```bash
space-stt
```

Le TUI interactif propose de choisir le backend "Local", le modèle, la langue et la touche push-to-talk.

### Mode client/serveur (SSH)

Pour utiliser un laptop léger (client) avec un serveur GPU distant. Le même binaire sert des deux côtés.

**Sur le serveur GPU** : installer `space-stt` et les modèles Whisper. Pas besoin de le lancer manuellement — le client le démarre automatiquement via SSH.

**Sur le client** : lancer `space-stt` et choisir le backend "Remote (SSH)" dans le TUI. Il suffit d'entrer la cible SSH (ex: `user@192.168.1.34`) et le client découvrira les modèles disponibles sur le serveur.

```
CLIENT (laptop)                             SERVEUR (GPU)
┌─────────────────────────┐                ┌──────────────────┐
│ Audio capture (micro)   │                │                  │
│ Resampler → 16kHz mono  │                │ Whisper (GPU)    │
│ VAD → segments          │── SSH pipe ──→│ Transcription    │
│ Hotkey (push-to-talk)   │← SSH pipe ───│                  │
│ Injection (dotool)      │                │                  │
└─────────────────────────┘                └──────────────────┘
```

Le client spawne `ssh <target> space-stt --server --model <path> --language <lang>` et communique via stdin/stdout avec un protocole binaire.

**Prérequis** :
- SSH sans mot de passe configuré (clé publique) vers le serveur
- `space-stt` dans le `PATH` du serveur
- Au moins un modèle Whisper (`ggml-*.bin`) dans le dossier `models/` du serveur

### Commandes CLI

```bash
space-stt                                          # client TUI (mode interactif)
space-stt --server --model <path> --language fr    # serveur (lancé automatiquement par le client via SSH)
space-stt --list-models                            # liste les modèles locaux (name\tpath)
space-stt --debug                                  # active les logs de debug
```
