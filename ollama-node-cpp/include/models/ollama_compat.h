#pragma once

#include <string>
#include <vector>
#include <optional>
#include <nlohmann/json.hpp>

namespace ollama_node {

struct OllamaManifest {
    std::string name;
    std::string digest;
    std::string gguf_path;
    std::string metadata_json;
    bool valid{false};
};

// シンプルな Ollama 互換マニフェスト読み込みヘルパー
class OllamaCompat {
public:
    explicit OllamaCompat(std::string models_dir);

    // ~/.ollama/models 配下を走査し、manifest 存在・GGUF の有無を検証
    std::vector<OllamaManifest> listAvailable() const;

    // 個別マニフェストを検証して GGUF パスを返す（存在しない場合は空文字）
    std::string resolveGguf(const std::string& model_name) const;

    // メタデータ（manifestに含まれる任意フィールド）を返す
    std::optional<nlohmann::json> loadMetadata(const std::string& model_name) const;

    // GGUF が存在し、digest が一致していれば true
    bool validateModel(const std::string& model_name) const;

private:
    std::string models_dir_;
};

}  // namespace ollama_node
