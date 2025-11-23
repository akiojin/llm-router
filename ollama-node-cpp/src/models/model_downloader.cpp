#include "models/model_downloader.h"

#include <array>
#include <cstring>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <httplib.h>
#include <memory>
#include <regex>
#include <thread>
#include <vector>
#include <optional>

#include "utils/config.h"
#include "utils/file_lock.h"

namespace {

struct ParsedUrl {
    std::string scheme;
    std::string host;
    int port{0};
    std::string path;
};

// Minimal SHA-256 implementation (public domain style)
struct Sha256Ctx {
    uint64_t bitlen = 0;
    uint32_t state[8];
    std::array<uint8_t, 64> data{};
    size_t datalen = 0;
};

constexpr uint32_t rotr(uint32_t x, uint32_t n) { return (x >> n) | (x << (32 - n)); }
constexpr uint32_t ch(uint32_t x, uint32_t y, uint32_t z) { return (x & y) ^ (~x & z); }
constexpr uint32_t maj(uint32_t x, uint32_t y, uint32_t z) { return (x & y) ^ (x & z) ^ (y & z); }
constexpr uint32_t ep0(uint32_t x) { return rotr(x, 2) ^ rotr(x, 13) ^ rotr(x, 22); }
constexpr uint32_t ep1(uint32_t x) { return rotr(x, 6) ^ rotr(x, 11) ^ rotr(x, 25); }
constexpr uint32_t sig0(uint32_t x) { return rotr(x, 7) ^ rotr(x, 18) ^ (x >> 3); }
constexpr uint32_t sig1(uint32_t x) { return rotr(x, 17) ^ rotr(x, 19) ^ (x >> 10); }

const uint32_t k[64] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2};

void sha256_transform(Sha256Ctx& ctx, const uint8_t data[]) {
    uint32_t m[64];
    for (uint32_t i = 0, j = 0; i < 16; ++i, j += 4) {
        m[i] = (data[j] << 24) | (data[j + 1] << 16) | (data[j + 2] << 8) | (data[j + 3]);
    }
    for (uint32_t i = 16; i < 64; ++i) {
        m[i] = sig1(m[i - 2]) + m[i - 7] + sig0(m[i - 15]) + m[i - 16];
    }

    uint32_t a = ctx.state[0];
    uint32_t b = ctx.state[1];
    uint32_t c = ctx.state[2];
    uint32_t d = ctx.state[3];
    uint32_t e = ctx.state[4];
    uint32_t f = ctx.state[5];
    uint32_t g = ctx.state[6];
    uint32_t h = ctx.state[7];

    for (uint32_t i = 0; i < 64; ++i) {
        uint32_t t1 = h + ep1(e) + ch(e, f, g) + k[i] + m[i];
        uint32_t t2 = ep0(a) + maj(a, b, c);
        h = g;
        g = f;
        f = e;
        e = d + t1;
        d = c;
        c = b;
        b = a;
        a = t1 + t2;
    }

    ctx.state[0] += a;
    ctx.state[1] += b;
    ctx.state[2] += c;
    ctx.state[3] += d;
    ctx.state[4] += e;
    ctx.state[5] += f;
    ctx.state[6] += g;
    ctx.state[7] += h;
}

void sha256_init(Sha256Ctx& ctx) {
    ctx.datalen = 0;
    ctx.bitlen = 0;
    ctx.state[0] = 0x6a09e667;
    ctx.state[1] = 0xbb67ae85;
    ctx.state[2] = 0x3c6ef372;
    ctx.state[3] = 0xa54ff53a;
    ctx.state[4] = 0x510e527f;
    ctx.state[5] = 0x9b05688c;
    ctx.state[6] = 0x1f83d9ab;
    ctx.state[7] = 0x5be0cd19;
}

void sha256_update(Sha256Ctx& ctx, const uint8_t data[], size_t len) {
    for (size_t i = 0; i < len; ++i) {
        ctx.data[ctx.datalen] = data[i];
        ctx.datalen++;
        if (ctx.datalen == 64) {
            sha256_transform(ctx, ctx.data.data());
            ctx.bitlen += 512;
            ctx.datalen = 0;
        }
    }
}

std::array<uint8_t, 32> sha256_final(Sha256Ctx& ctx) {
    size_t i = ctx.datalen;

    // Pad whatever data is left in the buffer.
    if (ctx.datalen < 56) {
        ctx.data[i++] = 0x80;
        while (i < 56) ctx.data[i++] = 0x00;
    } else {
        ctx.data[i++] = 0x80;
        while (i < 64) ctx.data[i++] = 0x00;
        sha256_transform(ctx, ctx.data.data());
        memset(ctx.data.data(), 0, 56);
    }

    ctx.bitlen += ctx.datalen * 8;
    ctx.data[63] = static_cast<uint8_t>(ctx.bitlen);
    ctx.data[62] = static_cast<uint8_t>(ctx.bitlen >> 8);
    ctx.data[61] = static_cast<uint8_t>(ctx.bitlen >> 16);
    ctx.data[60] = static_cast<uint8_t>(ctx.bitlen >> 24);
    ctx.data[59] = static_cast<uint8_t>(ctx.bitlen >> 32);
    ctx.data[58] = static_cast<uint8_t>(ctx.bitlen >> 40);
    ctx.data[57] = static_cast<uint8_t>(ctx.bitlen >> 48);
    ctx.data[56] = static_cast<uint8_t>(ctx.bitlen >> 56);
    sha256_transform(ctx, ctx.data.data());

    std::array<uint8_t, 32> hash{};
    for (uint32_t j = 0; j < 4; ++j) {
        hash[j] = (ctx.state[0] >> (24 - j * 8)) & 0xff;
        hash[j + 4] = (ctx.state[1] >> (24 - j * 8)) & 0xff;
        hash[j + 8] = (ctx.state[2] >> (24 - j * 8)) & 0xff;
        hash[j + 12] = (ctx.state[3] >> (24 - j * 8)) & 0xff;
        hash[j + 16] = (ctx.state[4] >> (24 - j * 8)) & 0xff;
        hash[j + 20] = (ctx.state[5] >> (24 - j * 8)) & 0xff;
        hash[j + 24] = (ctx.state[6] >> (24 - j * 8)) & 0xff;
        hash[j + 28] = (ctx.state[7] >> (24 - j * 8)) & 0xff;
    }
    return hash;
}

std::string to_hex(const std::array<uint8_t, 32>& hash) {
    static const char* hex = "0123456789abcdef";
    std::string out;
    out.reserve(64);
    for (auto b : hash) {
        out.push_back(hex[(b >> 4) & 0x0f]);
        out.push_back(hex[b & 0x0f]);
    }
    return out;
}

std::string sha256_of_file(const std::filesystem::path& path) {
    Sha256Ctx ctx;
    sha256_init(ctx);

    std::ifstream ifs(path, std::ios::binary);
    if (!ifs.is_open()) return "";

    std::array<char, 4096> buf{};
    while (ifs) {
        ifs.read(buf.data(), buf.size());
        std::streamsize n = ifs.gcount();
        if (n > 0) sha256_update(ctx, reinterpret_cast<uint8_t*>(buf.data()), static_cast<size_t>(n));
    }

    return to_hex(sha256_final(ctx));
}

// Incremental SHA256 for streamed verification
class StreamingSha256 {
public:
    StreamingSha256() { sha256_init(ctx_); }
    void update(const char* data, size_t len) {
        sha256_update(ctx_, reinterpret_cast<const uint8_t*>(data), len);
    }
    std::string finalize() { return to_hex(sha256_final(ctx_)); }

private:
    Sha256Ctx ctx_;
};

ParsedUrl parseUrl(const std::string& url) {
    static const std::regex re(R"(^([a-zA-Z][a-zA-Z0-9+.-]*)://([^/:]+)(?::(\d+))?(.*)$)");
    std::smatch match;
    ParsedUrl parsed;
    if (std::regex_match(url, match, re)) {
        parsed.scheme = match[1].str();
        parsed.host = match[2].str();
        parsed.port = match[3].matched ? std::stoi(match[3].str()) : (parsed.scheme == "https" ? 443 : 80);
        parsed.path = match[4].str().empty() ? "/" : match[4].str();
    }
    return parsed;
}

std::unique_ptr<httplib::Client> makeClient(const ParsedUrl& url, std::chrono::milliseconds timeout) {
    if (url.scheme.empty() || url.host.empty()) {
        return nullptr;
    }

    std::unique_ptr<httplib::Client> client;

    const bool use_https = url.scheme == "https";

#ifdef CPPHTTPLIB_OPENSSL_SUPPORT
    if (use_https) {
        client = std::make_unique<httplib::SSLClient>(url.host, url.port);
    }
#else
    if (use_https) {
        return nullptr;  // HTTPS is not supported in this build
    }
#endif

    if (!use_https) {
        client = std::make_unique<httplib::Client>(url.host, url.port);
    }

    if (client) {
        const int sec = static_cast<int>(timeout.count() / 1000);
        const int usec = static_cast<int>((timeout.count() % 1000) * 1000);
        client->set_connection_timeout(sec, usec);
        client->set_read_timeout(sec, usec);
        client->set_write_timeout(sec, usec);
        client->set_follow_location(true);
    }

    return client;
}

}  // namespace

namespace fs = std::filesystem;

namespace ollama_node {

ModelDownloader::ModelDownloader(std::string registry_base, std::string models_dir,
                                 std::chrono::milliseconds timeout, int max_retries,
                                 std::chrono::milliseconds backoff)
    : registry_base_(std::move(registry_base)), models_dir_(std::move(models_dir)), timeout_(timeout),
      max_retries_(max_retries), backoff_(backoff) {

    // override by config
    auto cfg_pair = loadDownloadConfigWithLog();
    auto cfg = cfg_pair.first;
    log_source_ = cfg_pair.second;
    max_retries_ = cfg.max_retries;
    backoff_ = cfg.backoff;
    max_bytes_per_sec_ = cfg.max_bytes_per_sec;
    chunk_size_ = cfg.chunk_size;
}

std::string ModelDownloader::fetchManifest(const std::string& model_id) {
    ParsedUrl base = parseUrl(registry_base_);
    if (base.scheme.empty() || base.host.empty()) return "";

    auto client = makeClient(base, timeout_);
    if (!client) return "";

    std::string path = base.path;
    if (path.empty()) path = "/";
    if (path.back() != '/') path.push_back('/');
    path += model_id + "/manifest.json";

    std::string out_path = models_dir_ + "/" + model_id + "/manifest.json";
    fs::create_directories(models_dir_ + "/" + model_id);
    FileLock lock(out_path);
    // ロック取得できなくてもベストエフォートで進める
    httplib::Result res;
    for (int attempt = 0; attempt <= max_retries_; ++attempt) {
        res = client->Get(path.c_str());
        if (res && res->status >= 200 && res->status < 300) break;
        if (attempt < max_retries_) std::this_thread::sleep_for(backoff_);
    }
    if (!res || res->status < 200 || res->status >= 300) return "";

    fs::create_directories(models_dir_ + "/" + model_id);
    std::ofstream ofs(out_path, std::ios::binary | std::ios::trunc);
    ofs << res->body;
    // log applied config for diagnostics (opt-in)
    if (const char* logenv = std::getenv("OLLAMA_DL_LOG_CONFIG")) {
        if (std::string(logenv) == "1" || std::string(logenv) == "true") {
            auto cfg_pair = loadDownloadConfigWithLog();
            auto cfg = cfg_pair.first;
            std::cerr << "[config] retries=" << cfg.max_retries
                      << " backoff_ms=" << cfg.backoff.count()
                      << " concurrency=" << cfg.max_concurrency
                      << " max_bps=" << cfg.max_bytes_per_sec
                      << " chunk=" << cfg.chunk_size
                      << " sources: " << cfg_pair.second << std::endl;
            if (cfg_pair.second.find("source=default") != std::string::npos) {
                std::cerr << "[config] using defaults (no env/file overrides)" << std::endl;
            }
        }
    }
    return out_path;
}

std::string ModelDownloader::downloadBlob(const std::string& blob_url, const std::string& filename, ProgressCallback cb,
                                          const std::string& expected_sha256, const std::string& if_none_match) {
    ParsedUrl url = parseUrl(blob_url);

    // blob_url が相対パスの場合は registry_base_ を基準に解決する
    if (url.scheme.empty()) {
        url = parseUrl(registry_base_);
        if (url.scheme.empty()) {
            return "";
        }

        if (!blob_url.empty() && blob_url.front() == '/') {
            url.path = blob_url;
        } else {
            if (url.path.empty()) url.path = "/";
            if (url.path.back() != '/') url.path.push_back('/');
            url.path += blob_url;
        }
    }

    auto client = makeClient(url, timeout_);
    if (!client) {
        return "";
    }

    fs::path out_path = fs::path(models_dir_) / filename;
    fs::create_directories(out_path.parent_path());

    // Prevent concurrent writers for the same blob (best-effort)
    FileLock blob_lock(out_path);

    const size_t original_offset = [&]() {
        if (!fs::exists(out_path)) return static_cast<size_t>(0);
        std::error_code ec;
        auto size = static_cast<size_t>(fs::file_size(out_path, ec));
        return ec ? static_cast<size_t>(0) : size;
    }();

    // If conditional ETag check is requested, avoid Range-based resume for simplicity.
    const size_t resume_offset = if_none_match.empty() ? original_offset : 0;
    const bool allow_range = if_none_match.empty();

    // If-None-Match handling: use simple GET and short-circuit 304 without streaming
    if (!if_none_match.empty()) {
        httplib::Headers hdrs{{"If-None-Match", if_none_match}};
        for (int attempt = 0; attempt <= max_retries_; ++attempt) {
            auto res = client->Get(url.path, hdrs);
            if (res) {
                if (res->status == 304) {
                    if (fs::exists(out_path)) {
                        if (cb) cb(original_offset, original_offset);
                        return out_path.string();
                    }
                } else if (res->status >= 200 && res->status < 300) {
                    fs::create_directories(out_path.parent_path());
                    std::ofstream ofs(out_path, std::ios::binary | std::ios::trunc);
                    ofs << res->body;
                    ofs.flush();
                    if (cb) cb(res->body.size(), res->body.size());

                    if (!expected_sha256.empty()) {
                        auto actual = sha256_of_file(out_path);
                        if (actual.empty() || actual != expected_sha256) {
                            std::error_code ec;
                            fs::remove(out_path, ec);
                            return "";
                        }
                    }
                    return out_path.string();
                }
            }
            if (attempt < max_retries_) std::this_thread::sleep_for(backoff_);
        }
        if (fs::exists(out_path)) {
            // Assume not modified if server unreachable but cached file exists
            return out_path.string();
        }
        // could not satisfy conditional request
        return "";
    }

    auto download_once = [&](size_t offset, bool use_range) -> bool {
        for (int attempt = 0; attempt <= max_retries_; ++attempt) {
            std::ofstream ofs(out_path, std::ios::binary | (offset > 0 && use_range ? std::ios::app : std::ios::trunc));
            if (!ofs.is_open()) return false;

            size_t downloaded = offset;
            size_t total = offset;
            std::optional<StreamingSha256> streamer;
            if (!expected_sha256.empty()) streamer.emplace();

            auto start_time = std::chrono::steady_clock::now();

            httplib::Headers headers;
            if (use_range && offset > 0) {
                headers.emplace("Range", "bytes=" + std::to_string(offset) + "-");
            }
            auto result = client->Get(
                url.path,
                headers,
                [&](const httplib::Response& res) {
                    if (res.has_header("Content-Length")) {
                        try {
                            total = offset + static_cast<size_t>(std::stoull(res.get_header_value("Content-Length")));
                        } catch (...) {
                            total = offset;
                        }
                    }
                    if (res.status == 304) {
                        // Not modified; treat as success if file already exists
                        return fs::exists(out_path);
                    }
                    return res.status >= 200 && res.status < 300;
                },
                [&](const char* data, size_t data_length) {
                    ofs.write(data, data_length);
                    downloaded += data_length;
                    if (streamer) streamer->update(data, data_length);
                    if (cb) cb(downloaded, total);

                    if (max_bytes_per_sec_ > 0) {
                        auto elapsed = std::chrono::steady_clock::now() - start_time;
                        double elapsed_sec = std::chrono::duration<double>(elapsed).count();
                        double allowed = max_bytes_per_sec_ * elapsed_sec;
                        if (downloaded > allowed && elapsed_sec > 0.0) {
                            double excess = downloaded - allowed;
                            double sleep_sec = excess / static_cast<double>(max_bytes_per_sec_);
                            if (sleep_sec > 0) {
                                std::this_thread::sleep_for(std::chrono::duration<double>(sleep_sec));
                            }
                        }
                    }
                    return true;
                });

            ofs.flush();

            if (result && (result->status == 304 || (result->status >= 200 && result->status < 300))) {
                if (cb && total == offset) {
                    cb(downloaded, downloaded);
                }
                if (streamer && !expected_sha256.empty() && result->status != 304) {
                    auto actual = streamer->finalize();
                    if (actual.empty() || actual != expected_sha256) {
                        std::error_code ec;
                        fs::remove(out_path, ec);
                        return false;
                    }
                }
                return true;
            }

            if (attempt < max_retries_) std::this_thread::sleep_for(backoff_);
        }
        return false;
    };

    // 1st attempt: resume with Range if partial exists
    auto validate_and_return = [&](bool resumed) -> std::string {
        if (expected_sha256.empty()) return out_path.string();
        auto actual = sha256_of_file(out_path);
        if (!actual.empty() && actual == expected_sha256) return out_path.string();

        // checksum mismatch
        if (resumed) {
            // retry full download once
            if (download_once(0, false)) {
                actual = sha256_of_file(out_path);
                if (!actual.empty() && actual == expected_sha256) return out_path.string();
            }
        }

        std::error_code ec;
        fs::remove(out_path, ec);
        return "";
    };

    if (download_once(resume_offset, allow_range)) {
        return validate_and_return(true);
    }

    // fallback: full re-download
    if (download_once(0, false)) {
        return validate_and_return(false);
    }

    // All attempts failed. Clean up only if the original file didn't exist.
    if (original_offset == 0) {
        std::error_code ec;
        fs::remove(out_path, ec);
    }
    return "";
}

}  // namespace ollama_node
