#pragma once

#include <vector>
#include <string>
#include <mutex>

namespace ollama_node {

class ModelRegistry {
public:
    void setModels(std::vector<std::string> models);
    std::vector<std::string> listModels() const;
    bool hasModel(const std::string& id) const;

private:
    mutable std::mutex mutex_;
    std::vector<std::string> models_;
};

}  // namespace ollama_node
