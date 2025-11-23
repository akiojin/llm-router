#pragma once

#include <string>
#include <filesystem>
#include <array>
#include <openssl/sha.h>
#include <fstream>

namespace ollama_node {

inline std::string sha256_file(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary);
    if (!file.is_open()) return "";
    SHA256_CTX ctx;
    if (SHA256_Init(&ctx) != 1) return "";
    std::array<char, 8192> buf{};
    while (file) {
        file.read(buf.data(), buf.size());
        std::streamsize n = file.gcount();
        if (n > 0) {
            if (SHA256_Update(&ctx, buf.data(), static_cast<size_t>(n)) != 1) return "";
        }
    }
    std::array<unsigned char, SHA256_DIGEST_LENGTH> hash{};
    if (SHA256_Final(hash.data(), &ctx) != 1) return "";
    static const char* hex = "0123456789abcdef";
    std::string hexout;
    hexout.reserve(64);
    for (auto b : hash) {
        hexout.push_back(hex[(b >> 4) & 0x0F]);
        hexout.push_back(hex[b & 0x0F]);
    }
    return hexout;
}

}  // namespace ollama_node
