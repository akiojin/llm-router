#pragma once

#include <string>
#include <vector>
#include <optional>
#include <filesystem>

namespace ollama_node {

struct HfFile {
    std::string name;
    std::string url;
    size_t size{0};
};

class HfClient {
public:
    explicit HfClient(std::string cache_dir);

    // 模擬: リスト取得
    std::vector<HfFile> listFiles(const std::string& repo_id) const;

    // 模擬: ファイルダウンロード（キャッシュ）
    std::string downloadFile(const std::string& repo_id, const std::string& filename);

    // GGUF かどうかを判定
    bool isGguf(const std::string& filename) const;

    // 変換が必要か判定（簡易ロジック）
    bool needsConversion(const std::string& filename) const;

    // LoRA / Diffusers 判定
    bool isLora(const std::string& filename) const;
    bool isDiffusersRepo(const std::string& repo_id) const;

private:
    std::filesystem::path cache_dir_;
};

}  // namespace ollama_node
