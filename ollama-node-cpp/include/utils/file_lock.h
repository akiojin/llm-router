#pragma once

#include <filesystem>
#include <optional>
#ifdef __unix__
#include <sys/file.h>
#include <fcntl.h>
#include <unistd.h>
#endif
#ifdef _WIN32
#include <windows.h>
#endif

namespace ollama_node {

// 簡易ファイルロック（ベストエフォート）。
// Unix: flock、Windows: LockFileEx、その他: lockディレクトリ作成。
// 非ブロッキングで獲得できない場合は locked() が false になる。
class FileLock {
public:
    explicit FileLock(const std::filesystem::path& target)
        : target_(target) {
        acquire();
    }

    ~FileLock() { release(); }

    bool locked() const { return locked_; }

private:
    void acquire();
    void release();

    std::filesystem::path target_;
    bool locked_{false};

#ifdef __unix__
    int fd_{-1};
#elif defined(_WIN32)
    void* handle_{nullptr};
#endif
    bool used_dir_lock_{false};
    std::filesystem::path dir_lock_path_;
};

// ---- Implementation ----

inline void FileLock::acquire() {
#ifdef __unix__
    fd_ = ::open(target_.c_str(), O_CREAT | O_RDWR, 0644);
    if (fd_ >= 0 && ::flock(fd_, LOCK_EX | LOCK_NB) == 0) {
        locked_ = true;
        return;
    }
    if (fd_ >= 0) {
        ::close(fd_);
        fd_ = -1;
    }
#elif defined(_WIN32)
    handle_ = CreateFileA(target_.string().c_str(), GENERIC_READ | GENERIC_WRITE,
                          0, nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (handle_ != INVALID_HANDLE_VALUE && handle_ != nullptr) {
        OVERLAPPED ov = {};
        if (LockFileEx(handle_, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0,
                       MAXDWORD, MAXDWORD, &ov)) {
            locked_ = true;
            return;
        }
        CloseHandle(handle_);
        handle_ = nullptr;
    }
#endif
    // Fallback: lock directory
    dir_lock_path_ = target_.string() + ".lock";
    std::error_code ec;
    if (std::filesystem::create_directory(dir_lock_path_, ec)) {
        locked_ = true;
        used_dir_lock_ = true;
    }
}

inline void FileLock::release() {
    if (!locked_) return;
#ifdef __unix__
    if (fd_ >= 0) {
        ::flock(fd_, LOCK_UN);
        ::close(fd_);
        fd_ = -1;
    }
#elif defined(_WIN32)
    if (handle_ != nullptr) {
        OVERLAPPED ov = {};
        UnlockFileEx(handle_, 0, MAXDWORD, MAXDWORD, &ov);
        CloseHandle(handle_);
        handle_ = nullptr;
    }
#endif
    if (used_dir_lock_) {
        std::error_code ec;
        std::filesystem::remove(dir_lock_path_, ec);
    }
    locked_ = false;
}

}  // namespace ollama_node
