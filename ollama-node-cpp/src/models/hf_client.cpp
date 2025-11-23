#include "models/hf_client.h"

#include <filesystem>
#include <fstream>

namespace fs = std::filesystem;

namespace ollama_node {

namespace {
std::string make_url(const std::string& repo_id, const std::string& filename) {
    return "https://huggingface.co/" + repo_id + "/resolve/main/" + filename;
}
}  // namespace

HfClient::HfClient(std::string cache_dir) : cache_dir_(std::move(cache_dir)) {}

std::vector<HfFile> HfClient::listFiles(const std::string& repo_id) const {
    // ダミー: 単一GGUFとsafetensorsを返す
    return {
        {repo_id + "/model.gguf", make_url(repo_id, "model.gguf"), 1024},
        {repo_id + "/adapter.safetensors", make_url(repo_id, "adapter.safetensors"), 2048},
    };
}

std::string HfClient::downloadFile(const std::string& repo_id, const std::string& filename) {
    fs::path dest = cache_dir_ / repo_id / filename;
    fs::create_directories(dest.parent_path());
    std::ofstream ofs(dest, std::ios::binary | std::ios::trunc);
    if (!ofs.is_open()) return "";
    ofs << "dummy data for " << filename;
    return dest.string();
}

bool HfClient::isGguf(const std::string& filename) const {
    auto pos = filename.find_last_of('.');
    if (pos == std::string::npos) return false;
    auto ext = filename.substr(pos + 1);
    return ext == "gguf";
}

bool HfClient::needsConversion(const std::string& filename) const {
    auto pos = filename.find_last_of('.');
    if (pos == std::string::npos) return false;
    auto ext = filename.substr(pos + 1);
    return ext == "bin" || ext == "safetensors";
}

bool HfClient::isLora(const std::string& filename) const {
    return filename.find("adapter") != std::string::npos || filename.find("lora") != std::string::npos;
}

bool HfClient::isDiffusersRepo(const std::string& repo_id) const {
    // 簡易判定: "diffusers" を含むリポや "unet"/"text_encoder" を含む名前
    if (repo_id.find("diffusers") != std::string::npos) return true;
    return repo_id.find("unet") != std::string::npos || repo_id.find("text_encoder") != std::string::npos;
}

}  // namespace ollama_node
