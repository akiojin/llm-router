#include "models/model_sync.h"

#include <filesystem>
#include <unordered_set>
#include <httplib.h>
#include <nlohmann/json.hpp>
#include <mutex>
#include <fstream>
#include <chrono>
#include <algorithm>
#include <thread>
#include "utils/config.h"
#include "utils/file_lock.h"
#include "models/model_storage.h"

namespace fs = std::filesystem;
using json = nlohmann::json;

namespace llm_node {

namespace {
}  // namespace

size_t ModelSync::defaultConcurrency() {
    auto cfg = loadDownloadConfig();
    // Fallback to sane minimum of 1 in case config is misconfigured to 0
    return cfg.max_concurrency > 0 ? cfg.max_concurrency : static_cast<size_t>(1);
}

SyncStatusInfo ModelSync::getStatus() const {
    std::lock_guard<std::mutex> lock(status_mutex_);
    return status_;
}

ModelSync::ModelSync(std::string base_url, std::string models_dir, std::chrono::milliseconds timeout)
    : base_url_(std::move(base_url)), models_dir_(std::move(models_dir)), timeout_(timeout) {
    {
        std::lock_guard<std::mutex> lock(status_mutex_);
        status_.state = SyncState::Idle;
        status_.updated_at = std::chrono::system_clock::now();
    }
    // Load persisted ETag cache if present
    const auto cache_path = fs::path(models_dir_) / ".etag_cache.json";
    if (fs::exists(cache_path)) {
        FileLock read_lock(cache_path);
        if (read_lock.locked()) {
            try {
                std::ifstream ifs(cache_path, std::ios::binary);
                auto j = json::parse(ifs);
                if (j.is_object()) {
                    std::lock_guard<std::mutex> lock(etag_mutex_);
                    for (auto it = j.begin(); it != j.end(); ++it) {
                        if (it.value().is_object()) {
                            if (it.value().contains("etag") && it.value()["etag"].is_string()) {
                                etag_cache_[it.key()] = it.value()["etag"].get<std::string>();
                            }
                            if (it.value().contains("size") && it.value()["size"].is_number_unsigned()) {
                                size_cache_[it.key()] = it.value()["size"].get<size_t>();
                            }
                        } else if (it.value().is_string()) {
                            // backward compatibility
                            etag_cache_[it.key()] = it.value().get<std::string>();
                        }
                    }
                }
            } catch (...) {
                // ignore invalid cache
            }
        }
    }
}
    

std::vector<RemoteModel> ModelSync::fetchRemoteModels() {
    httplib::Client cli(base_url_.c_str());
    cli.set_connection_timeout(static_cast<int>(timeout_.count() / 1000), static_cast<int>((timeout_.count() % 1000) * 1000));
    cli.set_read_timeout(static_cast<int>(timeout_.count() / 1000), static_cast<int>((timeout_.count() % 1000) * 1000));

    auto res = cli.Get("/v1/models");
    if (!res || res->status < 200 || res->status >= 300) {
        return {};
    }

    try {
        auto body = json::parse(res->body);
        std::vector<RemoteModel> remote;
        if (body.contains("data") && body["data"].is_array()) {
            for (const auto& m : body["data"]) {
                if (!m.contains("id")) continue;

                RemoteModel rm;
                rm.id = m["id"].get<std::string>();
                rm.path = m.value("path", "");
                rm.download_url = m.value("download_url", "");
                rm.chat_template = m.value("chat_template", "");

                if (m.contains("etag") && m["etag"].is_string()) {
                    setCachedEtag(rm.id, m["etag"].get<std::string>());
                }
                if (m.contains("size") && m["size"].is_number_unsigned()) {
                    setCachedSize(rm.id, m["size"].get<size_t>());
                }

                remote_models_[rm.id] = rm;
                remote.push_back(std::move(rm));
            }
        }
        return remote;
    } catch (...) {
        return {};
    }
}

std::vector<std::string> ModelSync::listLocalModels() const {
    std::vector<std::string> models;
    if (!fs::exists(models_dir_)) return models;

    for (const auto& entry : fs::directory_iterator(models_dir_)) {
        if (entry.is_directory()) {
            models.push_back(entry.path().filename().string());
        }
    }
    return models;
}

ModelSyncResult ModelSync::sync() {
    try {
        {
            std::lock_guard<std::mutex> lock(status_mutex_);
            status_.state = SyncState::Running;
            status_.updated_at = std::chrono::system_clock::now();
        }

        auto remote_models = fetchRemoteModels();
        auto local = listLocalModels();

        // Persist ETag cache for next run (best-effort)
        const auto cache_path = fs::path(models_dir_) / ".etag_cache.json";
        const auto temp_path = cache_path.string() + ".tmp";

        auto write_cache = [&](const fs::path& path) {
            json cache_json;
            {
                std::lock_guard<std::mutex> lock(etag_mutex_);
                for (const auto& kv : etag_cache_) {
                    json entry;
                    entry["etag"] = kv.second;
                    if (auto it = size_cache_.find(kv.first); it != size_cache_.end()) {
                        entry["size"] = it->second;
                    }
                    cache_json[kv.first] = entry;
                }
            }
            std::ofstream ofs(path, std::ios::binary | std::ios::trunc);
            ofs << cache_json.dump();
        };

        bool persisted = false;

        FileLock lock(cache_path);
        if (lock.locked()) {
            try {
                write_cache(temp_path);
                fs::rename(temp_path, cache_path);
                persisted = true;
            } catch (...) {
                // ignore
            }
        }

        if (!persisted) {
            // Fallback to lock directory to reduce collision on other platforms
            const auto lock_path = fs::path(models_dir_) / ".etag_cache.lock";
            bool locked = false;
            try {
                locked = fs::create_directory(lock_path);
            } catch (...) {
                locked = false;
            }

            if (locked) {
                try {
                    write_cache(temp_path);
                    fs::rename(temp_path, cache_path);
                } catch (...) {
                    // ignore persistence errors
                }
                std::error_code ec;
                fs::remove(lock_path, ec);
            }
        }

        std::unordered_set<std::string> remote_set;
        std::unordered_map<std::string, RemoteModel> remote_map;
        for (const auto& rm : remote_models) {
            remote_set.insert(rm.id);
            remote_map[rm.id] = rm;
        }
        std::unordered_set<std::string> local_set(local.begin(), local.end());

        ModelSyncResult result;
        ModelDownloader downloader(base_url_, models_dir_, timeout_);

        for (const auto& id : remote_set) {
            if (local_set.count(id)) continue;

            bool ok = false;
            auto it = remote_map.find(id);
            if (it != remote_map.end()) {
                const auto& info = it->second;
                if (!info.path.empty()) {
                    std::error_code ec;
                    auto src = fs::path(info.path);
                    if (fs::exists(src, ec) && fs::is_regular_file(src, ec)) {
                        auto dest_dir = fs::path(models_dir_) / ModelStorage::modelNameToDir(id);
                        auto dest = dest_dir / "model.gguf";
                        fs::create_directories(dest_dir, ec);
                        if (!ec) {
                            fs::copy_file(src, dest, fs::copy_options::overwrite_existing, ec);
                            if (ec) {
                                // Fallback: treat as success if file already exists or copy still resulted in a file
                                if (fs::exists(dest)) {
                                    ec.clear();
                                    ok = true;
                                }
                            } else {
                                ok = true;
                            }
                        }
                    }
                }

                if (!ok && !info.download_url.empty()) {
                    auto filename = ModelStorage::modelNameToDir(id) + "/model.gguf";
                    auto out = downloader.downloadBlob(info.download_url, filename, nullptr);
                    ok = !out.empty();
                }

                // metadata (chat_template)
                if (ok && !info.chat_template.empty()) {
                    auto meta_dir = fs::path(models_dir_) / ModelStorage::modelNameToDir(id);
                    auto meta_path = meta_dir / "metadata.json";
                    nlohmann::json meta;
                    meta["chat_template"] = info.chat_template;
                    std::ofstream ofs(meta_path, std::ios::binary | std::ios::trunc);
                    ofs << meta.dump();
                }
            }

            if (!ok) {
                result.to_download.push_back(id);
            }
        }
        for (const auto& id : local) {
            if (!remote_set.count(id)) {
                result.to_delete.push_back(id);
            }
        }

        {
            std::lock_guard<std::mutex> lock(status_mutex_);
            status_.state = SyncState::Success;
            status_.updated_at = std::chrono::system_clock::now();
            status_.last_to_download = result.to_download;
            status_.last_to_delete = result.to_delete;
        }

        return result;
    } catch (...) {
        std::lock_guard<std::mutex> lock(status_mutex_);
        status_.state = SyncState::Failed;
        status_.updated_at = std::chrono::system_clock::now();
        return {};
    }
}

std::string ModelSync::getCachedEtag(const std::string& model_id) const {
    std::lock_guard<std::mutex> lock(etag_mutex_);
    auto it = etag_cache_.find(model_id);
    return it == etag_cache_.end() ? std::string{} : it->second;
}

void ModelSync::setCachedEtag(const std::string& model_id, std::string etag) {
    std::lock_guard<std::mutex> lock(etag_mutex_);
    etag_cache_[model_id] = std::move(etag);
}

std::optional<size_t> ModelSync::getCachedSize(const std::string& model_id) const {
    std::lock_guard<std::mutex> lock(etag_mutex_);
    auto it = size_cache_.find(model_id);
    if (it == size_cache_.end()) return std::nullopt;
    return it->second;
}

void ModelSync::setCachedSize(const std::string& model_id, size_t size) {
    std::lock_guard<std::mutex> lock(etag_mutex_);
    size_cache_[model_id] = size;
}

DownloadHint ModelSync::getDownloadHint(const std::string& model_id) const {
    DownloadHint hint;
    hint.etag = getCachedEtag(model_id);
    hint.size = getCachedSize(model_id);
    return hint;
}

void ModelSync::setModelOverrides(std::unordered_map<std::string, ModelOverrides> overrides) {
    std::lock_guard<std::mutex> lock(etag_mutex_);
    model_overrides_ = std::move(overrides);
}

    std::string ModelSync::downloadWithHint(ModelDownloader& downloader,
                                        const std::string& model_id,
                                        const std::string& blob_url,
                                        const std::string& filename,
                                        ProgressCallback cb,
                                        const std::string& expected_sha256) const {
    auto hint = getDownloadHint(model_id);
    // If local file does not exist yet, avoid sending If-None-Match to force download
    std::string if_none_match;
    auto full_path = std::filesystem::path(downloader.getModelsDir()) / filename;
    if (std::filesystem::exists(full_path) && !hint.etag.empty()) {
        if_none_match = hint.etag;
    }
    // If expected size known and file exists with same size, short circuit
    if (hint.size.has_value() && std::filesystem::exists(full_path)) {
        std::error_code ec;
        auto sz = std::filesystem::file_size(full_path, ec);
        if (!ec && sz == *hint.size) {
            return full_path.string();
        }
    }
    return downloader.downloadBlob(blob_url, filename, cb, expected_sha256, if_none_match);
}

bool ModelSync::downloadModel(ModelDownloader& downloader,
                              const std::string& model_id,
                              ProgressCallback cb) const {
    ModelOverrides model_cfg;
    {
        std::lock_guard<std::mutex> lock(etag_mutex_);
        auto it = model_overrides_.find(model_id);
        if (it != model_overrides_.end()) model_cfg = it->second;
    }

    auto manifest_path = downloader.fetchManifest(model_id);
    if (manifest_path.empty()) return false;

    try {
        std::ifstream ifs(manifest_path);
        auto j = json::parse(ifs);
        if (!j.contains("files") || !j["files"].is_array()) {
            return false;
        }

        struct DlTask {
            int priority;
            std::function<bool()> fn;
        };
        std::vector<DlTask> hi_tasks;
        std::vector<DlTask> lo_tasks;
        for (const auto& f : j["files"]) {
            std::string name = f.value("name", "");
            if (name.empty()) return false;
            std::string digest = f.value("digest", "");
            std::string url = f.value("url", "");
            if (url.empty()) {
                url = downloader.getRegistryBase();
                if (!url.empty() && url.back() != '/') url.push_back('/');
                url += name;
            }

            size_t file_chunk = f.value("chunk", static_cast<size_t>(0));
            size_t file_bps = f.value("max_bps", static_cast<size_t>(0));

            int priority = f.value("priority", 0);

            auto task_fn = [this, &downloader, model_id, url, name, digest, cb, model_cfg, file_chunk, file_bps, priority]() {
                size_t orig_chunk = downloader.getChunkSize();
                size_t orig_bps = downloader.getMaxBytesPerSec();

                size_t applied_chunk = orig_chunk;
                size_t applied_bps = orig_bps;

                if (file_chunk > 0) {
                    applied_chunk = file_chunk;
                } else if (model_cfg.chunk_size > 0) {
                    applied_chunk = model_cfg.chunk_size;
                }

                if (file_bps > 0) {
                    applied_bps = file_bps;
                } else if (model_cfg.max_bps > 0) {
                    applied_bps = model_cfg.max_bps;
                }

                // priority < 0 のときは帯域を抑制
                if (priority < 0 && applied_bps > 0) {
                    size_t factor = static_cast<size_t>(1 + (-priority));
                    applied_bps = std::max<size_t>(1, applied_bps / factor);
                }

                downloader.setChunkSize(applied_chunk);
                downloader.setMaxBytesPerSec(applied_bps);

                if (const char* logenv = std::getenv("LLM_DL_LOG_CONFIG")) {
                    if (std::string(logenv) == "1" || std::string(logenv) == "true") {
                        const char* source = "default";
                        if (file_chunk > 0 || file_bps > 0) source = "manifest";
                        else if (model_cfg.chunk_size > 0 || model_cfg.max_bps > 0) source = "model_override";
                        std::cerr << "[downloadModel] file=" << name
                                  << " chunk=" << applied_chunk
                                  << " max_bps=" << applied_bps
                                  << " source=" << source << std::endl;
                    }
                }

                auto out = downloadWithHint(downloader, model_id, url, model_id + "/" + name, cb, digest);

                downloader.setChunkSize(orig_chunk);
                downloader.setMaxBytesPerSec(orig_bps);
                return !out.empty();
            };

            if (priority >= 0) {
                hi_tasks.push_back({priority, task_fn});
            } else {
                lo_tasks.push_back({priority, task_fn});
            }
        }

        auto run_tasks = [](std::vector<DlTask>& list, size_t conc) {
            if (list.empty()) return true;
            std::sort(list.begin(), list.end(), [](const DlTask& a, const DlTask& b) {
                return a.priority > b.priority;  // high priority first
            });

            std::atomic<bool> ok{true};
            std::atomic<size_t> index{0};
            std::vector<std::thread> workers;
            workers.reserve(conc);
            for (size_t i = 0; i < conc; ++i) {
                workers.emplace_back([&]() {
                    while (true) {
                        size_t idx = index.fetch_add(1);
                        if (idx >= list.size() || !ok.load()) break;
                        if (!list[idx].fn()) {
                            ok.store(false);
                            break;
                        }
                    }
                });
            }
            for (auto& th : workers) {
                if (th.joinable()) th.join();
            }
            return ok.load();
        };

        const size_t base_conc = std::max<size_t>(1, defaultConcurrency());
        const size_t hi_conc = hi_tasks.empty() ? 0 : std::min(base_conc, hi_tasks.size());

        size_t lo_conc = 0;
        if (!lo_tasks.empty()) {
            int lowest = 0;
            for (const auto& t : lo_tasks) {
                lowest = std::min(lowest, t.priority);
            }
            // deeper negative priority reduces concurrency
            size_t divisor = 1 + static_cast<size_t>(-lowest);
            lo_conc = base_conc / divisor;
            if (lo_conc == 0) lo_conc = 1;
            lo_conc = std::min(lo_conc, lo_tasks.size());
        }

        bool ok = true;
        if (hi_conc > 0) {
            ok = run_tasks(hi_tasks, hi_conc);
        }
        if (ok && lo_conc > 0) {
            ok = run_tasks(lo_tasks, lo_conc);
        }
        return ok;
    } catch (...) {
        return false;
    }
}

}  // namespace llm_node
