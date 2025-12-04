#include "core/model_pool.h"

namespace llm_node {

ModelPool::ModelPool(std::shared_ptr<LlamaManager> manager) : manager_(std::move(manager)) {}

std::shared_ptr<LlamaContext> ModelPool::acquire(const std::string& model) {
    std::lock_guard<std::mutex> lock(mu_);
    const size_t before = manager_->memoryUsageBytes();
    const bool over_limit = memory_limit_ > 0 && before >= memory_limit_;
    if (over_limit) return nullptr;

    if (!manager_->loadModel(model)) return nullptr;
    const size_t after = manager_->memoryUsageBytes();
    if (memory_limit_ > 0 && after > memory_limit_) {
        manager_->unloadModel(model);
        return nullptr;
    }
    return manager_->createContext(model);
}

std::shared_ptr<LlamaContext> ModelPool::acquireForThread(const std::string& model, std::thread::id tid) {
    {
        std::lock_guard<std::mutex> lock(mu_);
        auto it = thread_cache_.find(tid);
        if (it != thread_cache_.end() && it->second && it->second->model_path.find(model) != std::string::npos) {
            return it->second;
        }
    }
    auto ctx = acquire(model);  // acquire handles locking
    {
        std::lock_guard<std::mutex> lock(mu_);
        thread_cache_[tid] = ctx;
    }
    return ctx;
}

size_t ModelPool::loadedCount() const {
    std::lock_guard<std::mutex> lock(mu_);
    return manager_->loadedCount();
}

bool ModelPool::unload(const std::string& model) {
    std::lock_guard<std::mutex> lock(mu_);
    return manager_->unloadModel(model);
}

void ModelPool::setMemoryLimit(size_t bytes) {
    std::lock_guard<std::mutex> lock(mu_);
    memory_limit_ = bytes;
}

size_t ModelPool::getMemoryLimit() const {
    std::lock_guard<std::mutex> lock(mu_);
    return memory_limit_;
}

void ModelPool::gc() {
    std::lock_guard<std::mutex> lock(mu_);
    auto loaded = manager_->getLoadedModels();
    for (const auto& m : loaded) {
        manager_->unloadModel(m);
    }
    thread_cache_.clear();
}

}  // namespace llm_node
