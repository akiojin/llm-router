//! Unit Test: SHA256 Checksum Calculation
//!
//! SHA256ハッシュ計算のテスト

use sha2::{Digest, Sha256};

#[test]
fn test_sha256_hash_calculation() {
    // 既知のテストデータ
    let test_data = b"hello world";

    // SHA256ハッシュを計算
    let mut hasher = Sha256::new();
    hasher.update(test_data);
    let hash = format!("{:x}", hasher.finalize());

    // 期待されるハッシュ（オンラインツールで確認可能）
    let expected_hash = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

    assert_eq!(hash, expected_hash, "SHA256 hash should match");
}

#[test]
fn test_empty_data_hash() {
    // 空のデータ
    let test_data = b"";

    let mut hasher = Sha256::new();
    hasher.update(test_data);
    let hash = format!("{:x}", hasher.finalize());

    // 空のデータのSHA256ハッシュ
    let expected_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    assert_eq!(hash, expected_hash, "Empty data hash should match");
}

#[test]
fn test_hash_is_deterministic() {
    // 同じデータからは常に同じハッシュが生成される
    let test_data = b"test data";

    let mut hasher1 = Sha256::new();
    hasher1.update(test_data);
    let hash1 = format!("{:x}", hasher1.finalize());

    let mut hasher2 = Sha256::new();
    hasher2.update(test_data);
    let hash2 = format!("{:x}", hasher2.finalize());

    assert_eq!(hash1, hash2, "Hash should be deterministic");
}

#[test]
fn test_different_data_produces_different_hash() {
    // 異なるデータからは異なるハッシュが生成される
    let data1 = b"data1";
    let data2 = b"data2";

    let mut hasher1 = Sha256::new();
    hasher1.update(data1);
    let hash1 = format!("{:x}", hasher1.finalize());

    let mut hasher2 = Sha256::new();
    hasher2.update(data2);
    let hash2 = format!("{:x}", hasher2.finalize());

    assert_ne!(hash1, hash2, "Different data should produce different hash");
}

#[test]
fn test_hash_length() {
    // SHA256ハッシュは常に64文字（256ビット = 32バイト = 64桁の16進数）
    let test_data = b"any data";

    let mut hasher = Sha256::new();
    hasher.update(test_data);
    let hash = format!("{:x}", hasher.finalize());

    assert_eq!(hash.len(), 64, "SHA256 hash should be 64 characters");
}
