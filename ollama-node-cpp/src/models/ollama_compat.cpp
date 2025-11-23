#include "models/ollama_compat.h"

#include <filesystem>
#include <fstream>
#include <nlohmann/json.hpp>
#include <array>
#include "utils/sha256.h"

namespace fs = std::filesystem;
using json = nlohmann::json;

namespace ollama_node {

OllamaCompat::OllamaCompat(std::string models_dir) : models_dir_(std::move(models_dir)) {}

std::string OllamaCompat::resolveGguf(const std::string& model_name) const {
    const auto manifest_path = fs::path(models_dir_) / model_name / "manifest.json";
    if (!fs::exists(manifest_path)) return "";

    try {
        std::ifstream ifs(manifest_path);
        json j = json::parse(ifs);
        if (j.contains("files") && j["files"].is_array()) {
            for (const auto& f : j["files"]) {
                if (f.value("type", "") == "gguf") {
                    auto path = f.value("path", "");
                    auto name = f.value("name", "");
                    std::vector<fs::path> candidates;
                    if (!path.empty()) candidates.push_back(path);
                    if (!name.empty()) candidates.push_back(name);
                    if (candidates.empty()) continue;
                    for (const auto& rel : candidates) {
                        auto full = fs::path(models_dir_) / model_name / rel;
                        if (fs::exists(full)) return full.string();
                    }
                }
            }
        }
    } catch (...) {
        return "";
    }
    return "";
}

std::vector<OllamaManifest> OllamaCompat::listAvailable() const {
    std::vector<OllamaManifest> out;
    if (!fs::exists(models_dir_)) return out;

    for (const auto& dir : fs::directory_iterator(models_dir_)) {
        if (!dir.is_directory()) continue;
        auto name = dir.path().filename().string();
        const auto manifest_path = dir.path() / "manifest.json";
        if (!fs::exists(manifest_path)) continue;

        try {
            std::ifstream ifs(manifest_path);
            json j = json::parse(ifs);
            if (!j.contains("files") || !j["files"].is_array()) continue;

            for (const auto& f : j["files"]) {
                if (f.value("type", "") != "gguf") continue;
                auto path = f.value("path", "");
                auto name_field = f.value("name", "");
                std::vector<fs::path> candidates;
                if (!path.empty()) candidates.push_back(path);
                if (!name_field.empty()) candidates.push_back(name_field);
                for (const auto& rel : candidates) {
                    OllamaManifest m;
                    m.name = name;
                    m.digest = f.value("digest", "");
                    m.gguf_path = (dir.path() / rel).string();
                    m.metadata_json = j.dump();
                    m.valid = validateModel(m.name);
                    if (fs::exists(m.gguf_path)) {
                        out.push_back(std::move(m));
                        break;
                    }
                }
            }
        } catch (...) {
            continue;
        }
    }
    return out;
}

std::optional<nlohmann::json> OllamaCompat::loadMetadata(const std::string& model_name) const {
    const auto manifest_path = fs::path(models_dir_) / model_name / "manifest.json";
    if (!fs::exists(manifest_path)) return std::nullopt;
    try {
        std::ifstream ifs(manifest_path);
        json j = json::parse(ifs);
        return j;
    } catch (...) {
        return std::nullopt;
    }
}

bool OllamaCompat::validateModel(const std::string& model_name) const {
    const auto manifest_path = fs::path(models_dir_) / model_name / "manifest.json";
    if (!fs::exists(manifest_path)) return false;
    try {
        std::ifstream ifs(manifest_path);
        json j = json::parse(ifs);
        if (!j.contains("files") || !j["files"].is_array()) return false;
        for (const auto& f : j["files"]) {
            if (f.value("type", "") != "gguf") continue;
            auto digest = f.value("digest", "");
            auto path = f.value("path", "");
            auto name = f.value("name", "");
            std::vector<fs::path> candidates;
            if (!path.empty()) candidates.push_back(path);
            if (!name.empty()) candidates.push_back(name);
            for (const auto& rel : candidates) {
                auto full = fs::path(models_dir_) / model_name / rel;
                if (!fs::exists(full)) continue;
                if (digest.empty()) return true;  // if no digest, accept presence

                auto hexout = sha256_file(full);
                if (!hexout.empty() && hexout == digest) return true;
            }
        }
    } catch (...) {
        return false;
    }
    return false;
}

}  // namespace ollama_node
