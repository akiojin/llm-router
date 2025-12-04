#include "models/model_registry.h"
#include <algorithm>

namespace llm_node {

void ModelRegistry::setModels(std::vector<std::string> models) {
    std::lock_guard<std::mutex> lock(mutex_);
    models_ = std::move(models);
}

std::vector<std::string> ModelRegistry::listModels() const {
    std::lock_guard<std::mutex> lock(mutex_);
    return models_;
}

bool ModelRegistry::hasModel(const std::string& id) const {
    std::lock_guard<std::mutex> lock(mutex_);
    return std::find(models_.begin(), models_.end(), id) != models_.end();
}

}  // namespace llm_node
