class OllamaNodeCpp < Formula
  desc "Ollama-compatible C++ node"
  homepage "https://github.com/akiojin/ollama-router"
  version "1.0.0"

  if Hardware::CPU.arm?
    url "https://github.com/akiojin/ollama-router/releases/latest/download/ollama-node-macos-arm64.tar.gz"
    sha256 "REPLACE_ARM64_SHA"
  else
    url "https://github.com/akiojin/ollama-router/releases/latest/download/ollama-node-macos-x64.tar.gz"
    sha256 "REPLACE_X64_SHA"
  end

  def install
    bin.install "ollama-node"
  end

  test do
    system "#{bin}/ollama-node", "--help"
  end
end
