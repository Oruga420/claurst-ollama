# Setup Guide

## Prerequisites

| Component | Version | Required |
|-----------|---------|----------|
| Rust | 1.75+ | Yes |
| Ollama | 0.20+ | Yes |
| GPU | 4-8GB VRAM | Recommended (CPU works but slow) |
| OS | macOS / Linux / WSL | Yes |

## macOS (Apple Silicon — M1/M2/M3/M4)

Apple Silicon Macs share unified memory between CPU and GPU, so a 16GB Mac can run 8B models comfortably.

```bash
# 1. Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 2. Install Ollama
brew install ollama
# OR
curl -fsSL https://ollama.com/install.sh | sh

# 3. Start Ollama
ollama serve &

# 4. Pull your model
ollama pull gemma4:e2b     # Smallest, fits any Mac
# OR for 16GB+ Macs:
ollama pull qwen3:8b       # Better quality

# 5. Clone and build
git clone https://github.com/Oruga420/claurst-ollama.git
cd claurst-ollama/src-rust
cargo build --release

# 6. Configure
export OLLAMA_ENDPOINT=http://localhost:11434/v1/chat/completions
export CLAUDE_MODEL=gemma4:e2b

# 7. Run
./target/release/claurst
```

### macOS Notes
- Ollama on Apple Silicon uses Metal GPU acceleration automatically
- 8GB Mac: use `gemma4:e2b` or `qwen3:1.7b`
- 16GB Mac: use `qwen3:8b` or `gemma4:e4b`
- 32GB+ Mac: use `qwen3:32b` or larger

## Linux (NVIDIA GPU)

```bash
# 1. Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# 2. Install build deps
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libssl-dev

# 3. Install Ollama
curl -fsSL https://ollama.com/install.sh | sh

# 4. Start Ollama (auto-detects NVIDIA GPU via CUDA)
ollama serve &

# 5. Pull model
ollama pull gemma4:e2b     # 4GB GPU
# OR
ollama pull qwen3:8b       # 8GB GPU

# 6. Clone and build
git clone https://github.com/Oruga420/claurst-ollama.git
cd claurst-ollama/src-rust
cargo build --release

# 7. Configure and run
export OLLAMA_ENDPOINT=http://localhost:11434/v1/chat/completions
export CLAUDE_MODEL=gemma4:e2b
./target/release/claurst
```

### Linux Notes
- Ollama auto-detects NVIDIA GPUs if CUDA drivers are installed
- Check GPU: `nvidia-smi`
- Check Ollama sees GPU: `ollama run gemma4:e2b "hello"` — should show GPU in `ollama ps`

## Linux (AMD GPU)

```bash
# Same as NVIDIA but ensure ROCm is installed
# Ollama supports AMD GPUs via ROCm
sudo apt-get install -y rocm-libs
# Then follow the NVIDIA steps above
```

## Windows (via WSL)

CLAURST is a Linux/macOS binary. On Windows, run inside WSL:

```powershell
# In PowerShell:
wsl --install -d Ubuntu

# Then inside WSL, follow the Linux steps above
```

If Ollama runs on Windows natively:
```bash
# Set endpoint to Windows host from WSL
export OLLAMA_ENDPOINT=http://$(cat /etc/resolv.conf | grep nameserver | awk '{print $2}'):11434/v1/chat/completions
```

## Verify Installation

```bash
# Check Ollama is running
curl -s http://localhost:11434/api/tags | python3 -m json.tool

# Check model is available
ollama list

# Test the adapter directly
curl -s http://localhost:11434/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gemma4:e2b",
    "messages": [{"role": "user", "content": "Say hello"}],
    "max_tokens": 50
  }' | python3 -m json.tool
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OLLAMA_ENDPOINT` | `http://localhost:11434/v1/chat/completions` | Ollama API URL |
| `CLAUDE_MODEL` | `gemma4:e2b` | Model to use |
| `ANTHROPIC_API_KEY` | (none) | Only needed for Anthropic provider |
| `OLLAMA_HOST` | `127.0.0.1` | Ollama listen address |

## Troubleshooting

### "Ollama request failed: connection refused"
Ollama isn't running. Start it: `ollama serve &`

### "Model not found"
Pull the model first: `ollama pull gemma4:e2b`

### "Out of memory"
Model too large for your GPU. Use a smaller model:
- 4GB: `gemma4:e2b` or `qwen3:1.7b`
- 8GB: `qwen3:8b` max

### Build errors on macOS
```bash
# Install Xcode CLI tools
xcode-select --install
```

### Build errors on Linux
```bash
# Install OpenSSL dev headers
sudo apt-get install -y libssl-dev pkg-config
```
