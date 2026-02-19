# Space STT — Local Speech-to-Text Terminal Injector

Single Rust binary that captures microphone audio, transcribes speech locally via whisper.cpp, and injects text into the focused window using dotool.

## System Prerequisites

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

Download at least one model into `~/.local/share/space-stt/models/`:

```bash
mkdir -p ~/.local/share/space-stt/models
# Pick one:
wget -P ~/.local/share/space-stt/models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin    # ~75 MB  — laptop CPU
wget -P ~/.local/share/space-stt/models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin    # ~142 MB — good balance
wget -P ~/.local/share/space-stt/models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin   # ~466 MB — mid-range GPU
wget -P ~/.local/share/space-stt/models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin  # ~1.5 GB — strong GPU
wget -P ~/.local/share/space-stt/models https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin # ~3.1 GB — best quality
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
