#pragma once

#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>
#include <thread>

#include "core/llama_manager.h"

namespace ollama_node {

class ModelPool {
public:
    explicit ModelPool(std::shared_ptr<LlamaManager> manager);

    // モデルをロードし、コンテキストを取得する（存在しなければロード）
    std::shared_ptr<LlamaContext> acquire(const std::string& model);

    // 現在ロード済みのモデル数
    size_t loadedCount() const;

    // モデルをアンロード（存在すれば）
    bool unload(const std::string& model);

    // メモリ制限を設定（バイト）
    void setMemoryLimit(size_t bytes);
    size_t getMemoryLimit() const;

    // 強制GC（全アンロード）
    void gc();

    // スレッドごとのモデル割り当て（簡易版）
    std::shared_ptr<LlamaContext> acquireForThread(const std::string& model, std::thread::id tid);

private:
    std::shared_ptr<LlamaManager> manager_;
    mutable std::mutex mu_;
    size_t memory_limit_{0};
    std::unordered_map<std::thread::id, std::shared_ptr<LlamaContext>> thread_cache_;
};

}  // namespace ollama_node
