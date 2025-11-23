#include <gtest/gtest.h>

// 共通のエントリポイント。個別テストではリンクだけで利用する。
int main(int argc, char** argv) {
    ::testing::InitGoogleTest(&argc, argv);
    return RUN_ALL_TESTS();
}
