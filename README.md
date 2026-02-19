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
./setup.sh install           # install everything (client + server)
./setup.sh install client    # client only (hotkey, audio, dotool)
./setup.sh install server    # server only (whisper, CUDA, models)
./setup.sh uninstall         # remove everything cleanly
./setup.sh model             # download a Whisper model
```

The script auto-detects your package manager (dnf, apt, pacman).

---

## Client (`space_tts_client`)

Le client est léger : il capture l'audio, détecte la voix, et envoie les segments au serveur distant via SSH. Il injecte le texte transcrit dans la fenêtre active via dotool. **Pas besoin de whisper-rs ni de GPU.**

```bash
# Installation (deps + dotool + build + install dans /usr/local/bin/)
./setup.sh install client

# Build seul (si deps déjà installées)
cargo build --release -p space_tts_client

# Lancement
space_tts_client
space_tts_client --debug   # avec logs de debug
```

Le TUI demande successivement :
1. La cible SSH (ex: `user@192.168.1.34`)
2. Le modèle Whisper (découverte automatique sur le serveur)
3. La langue
4. La touche push-to-talk

---

## Serveur (`space_tts_server`)

Le serveur charge un modèle Whisper et fait la transcription (GPU ou CPU). En production il est lancé automatiquement par le client via SSH, mais c'est un binaire indépendant.

```bash
# Installation (deps + CUDA optionnel + modèle + build + install dans /usr/local/bin/)
./setup.sh install server

# Build seul (si deps déjà installées)
cargo build --release -p space_tts_server
cargo build --release -p space_tts_server --features cuda   # avec GPU

# Vérifier que les modèles sont détectés
space_tts_server --list-models

# Lancer manuellement (stdin/stdout)
space_tts_server --model small --language fr
space_tts_server --model small --language fr --debug
```

`--model` accepte un nom court (`small`), un nom de fichier (`ggml-small.bin`) ou un chemin complet. `--list-models` affiche des commandes prêtes à copier-coller.

En production, le client lance le serveur automatiquement via SSH :
```
ssh <target> space_tts_server --model small --language fr
```

---

## Prérequis SSH

Le client communique avec le serveur via SSH en mode non-interactif (pipe stdin/stdout). **La connexion par mot de passe ne fonctionne pas** — il faut une authentification par clé.

### Configurer la clé SSH

```bash
# Générer une clé (si pas déjà fait)
ssh-keygen -t ed25519

# Copier la clé sur le serveur
ssh-copy-id user@serveur

# Vérifier que la connexion fonctionne sans mot de passe
ssh user@serveur echo ok
```

Si la machine client et serveur sont la même (`localhost`) :
```bash
ssh-copy-id $USER@127.0.0.1
```

### Checklist

- SSH sans mot de passe fonctionnel (`ssh user@serveur` ne demande rien)
- `space_tts_server` dans le `PATH` du serveur (installé dans `/usr/local/bin/` par `setup.sh`)
- Au moins un modèle Whisper (`ggml-*.bin`) dans `~/.local/share/space_tts/models/` (téléchargé par `setup.sh`)
