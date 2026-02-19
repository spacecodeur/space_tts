# Space STT — Speech-to-Text Terminal Injector

> **Note:** This project was largely written through vibecoding with an LLM (Claude).

Système client/serveur en Rust pour la transcription vocale. Le **client** (léger) capture l'audio et injecte le texte, le **serveur** (lourd) fait la transcription via whisper.cpp sur GPU. Communication via SSH.

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

## Features

- **Architecture client/serveur** — deux binaires séparés, le client ne dépend pas de whisper-rs
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

---

## Client (`space_tts_client`)

Le client est léger : il capture l'audio, détecte la voix, et envoie les segments au serveur distant via SSH. Il injecte le texte transcrit dans la fenêtre active via dotool. **Pas besoin de whisper-rs ni de GPU.**

### Dépendances client

#### Fedora (dnf)

```bash
sudo dnf install -y pkg-config alsa-lib-devel

# dotool (text injection — build from source)
sudo dnf install -y golang libxkbcommon-devel scdoc
git clone https://git.sr.ht/~geb/dotool && cd dotool
./build.sh && sudo ./build.sh install

# Permissions (needed for evdev hotkey AND dotool uinput)
sudo usermod -aG input $USER
# Log out and back in for group change to take effect
```

#### Debian / Ubuntu (apt)

```bash
sudo apt install -y pkg-config libasound2-dev

# dotool (text injection — build from source)
sudo apt install -y golang libxkbcommon-dev scdoc
git clone https://git.sr.ht/~geb/dotool && cd dotool
./build.sh && sudo ./build.sh install

# Permissions (needed for evdev hotkey AND dotool uinput)
sudo usermod -aG input $USER
# Log out and back in for group change to take effect
```

### Build client

```bash
cargo build --release -p space_tts_client
```

### Usage client

```bash
space_tts_client           # lance le TUI interactif
space_tts_client --debug   # avec logs de debug
```

Le TUI demande successivement :
1. La cible SSH (ex: `user@192.168.1.34`)
2. Le modèle Whisper (découverte automatique sur le serveur)
3. La langue
4. La touche push-to-talk

---

## Serveur (`space_tts_server`)

Le serveur est lourd : il charge un modèle Whisper et fait la transcription GPU. Il est lancé automatiquement par le client via SSH — pas besoin de le démarrer manuellement.

### Dépendances serveur

#### Fedora (dnf)

```bash
# Build dependencies (whisper.cpp)
sudo dnf install -y cmake gcc gcc-c++

# CUDA (optional — GPU acceleration)
sudo dnf install -y cuda-nvcc cuda-cudart-devel cuda-cudart-static cuda-culibos-devel cuda-cccl-devel
```

#### Debian / Ubuntu (apt)

```bash
# Build dependencies (whisper.cpp)
sudo apt install -y cmake gcc g++

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

### Build serveur

```bash
# CPU only (default)
cargo build --release -p space_tts_server

# With CUDA GPU acceleration
cargo build --release -p space_tts_server --features cuda
```

### Usage serveur

```bash
space_tts_server --list-models                            # liste les modèles locaux (name\tpath)
space_tts_server --model <path> --language fr              # lance le serveur (stdin/stdout)
space_tts_server --model <path> --language fr --debug      # avec logs de debug
```

En pratique, le serveur est lancé automatiquement par le client via SSH :
```
ssh <target> space_tts_server --model <path> --language <lang>
```

---

## Prérequis SSH

- SSH sans mot de passe configuré (clé publique) vers le serveur
- `space_tts_server` dans le `PATH` du serveur
- Au moins un modèle Whisper (`ggml-*.bin`) dans le dossier `models/` du serveur
