//! 加密备份导出/导入（§4.11 云端同步基础：本地 E2E 备份）
//!
//! 格式：`KOIDBK1` 魔数 + 12 字节随机 nonce + AES-256-GCM 密文
//! 密钥：SHA-256(口令) → 32 字节 AES 密钥
//! 明文：SQLite 快照（通过 rusqlite Backup API 在线备份，无需关闭连接）
//!
//! 云端上传为后续迭代；本模块保证任何介质上的备份文件均端到端加密。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use rusqlite::backup::Backup;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;

const MAGIC: &[u8] = b"KOIDBK1";
const NONCE_LEN: usize = 12;

fn derive_key(passphrase: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(passphrase.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest);
    key
}

fn random_nonce() -> Result<[u8; NONCE_LEN], String> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|e| format!("生成随机数失败: {e}"))?;
    Ok(nonce)
}

/// 把 live 连接在线备份到目标文件（SQLite backup API，一致性快照）
fn snapshot_to_file(live: &Connection, target: &Path) -> Result<(), String> {
    let mut dst = Connection::open(target).map_err(|e| format!("打开备份文件失败: {e}"))?;
    let backup = Backup::new(live, &mut dst).map_err(|e| e.to_string())?;
    backup
        .run_to_completion(1000, std::time::Duration::from_millis(0), None)
        .map_err(|e| format!("备份数据库失败: {e}"))?;
    Ok(())
}

/// 导出加密备份：返回生成的文件路径
pub fn export(app: &tauri::AppHandle, passphrase: &str, dest: Option<&str>) -> Result<String, String> {
    use tauri::Manager;
    let st = app.state::<crate::state::AppState>();
    let live = st.db()?;

    // 1. 在线快照到临时文件
    let tmp_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("backups");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let snapshot = tmp_dir.join("snapshot.db");
    let _ = std::fs::remove_file(&snapshot);
    snapshot_to_file(&live, &snapshot)?;
    drop(live);
    drop(st);

    // 2. 读取明文
    let mut plaintext = Vec::new();
    std::fs::File::open(&snapshot)
        .and_then(|mut f| f.read_to_end(&mut plaintext))
        .map_err(|e| format!("读取快照失败: {e}"))?;

    // 3. 加密（key: [u8;32] → Key；nonce: [u8;12] → Nonce）
    let key_arr = derive_key(passphrase);
    let nonce_arr = random_nonce()?;
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(key_arr));
    let ciphertext = cipher
        .encrypt(&Nonce::from(nonce_arr), plaintext.as_ref())
        .map_err(|e| format!("加密失败: {e}"))?;

    // 4. 写输出
    let path = match dest {
        Some(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => {
            let ts = crate::utils::now_ms();
            tmp_dir.join(format!("koid-backup-{ts}.koid-backup")).to_string_lossy().to_string()
        }
    };
    let mut out = std::fs::File::create(&path).map_err(|e| format!("创建备份文件失败: {e}"))?;
    out.write_all(MAGIC).map_err(|e| e.to_string())?;
    out.write_all(&nonce_arr).map_err(|e| e.to_string())?;
    out.write_all(&ciphertext).map_err(|e| e.to_string())?;

    let _ = std::fs::remove_file(&snapshot);
    Ok(path)
}

/// 从加密备份导入：解密后经 backup API 合并进 live 连接
pub fn import(app: &tauri::AppHandle, passphrase: &str, path: &str) -> Result<(), String> {
    use tauri::Manager;

    // 1. 读文件 + 校验魔数
    let mut data = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_end(&mut data))
        .map_err(|e| format!("读取备份失败: {e}"))?;
    if data.len() < MAGIC.len() + NONCE_LEN || &data[..MAGIC.len()] != MAGIC {
        return Err("不是有效的 Koid 备份文件".to_string());
    }
    let nonce_bytes: [u8; NONCE_LEN] = data[MAGIC.len()..MAGIC.len() + NONCE_LEN]
        .try_into()
        .map_err(|_| "备份文件损坏".to_string())?;
    let ciphertext = &data[MAGIC.len() + NONCE_LEN..];

    // 2. 解密
    let key_arr = derive_key(passphrase);
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(key_arr));
    let plaintext = cipher
        .decrypt(&Nonce::from(nonce_bytes), ciphertext)
        .map_err(|_| "解密失败：口令错误或文件已损坏".to_string())?;

    // 3. 写入临时库，backup 合并进 live
    let tmp_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("backups");
    std::fs::create_dir_all(&tmp_dir).map_err(|e| e.to_string())?;
    let tmp = tmp_dir.join("import.db");
    std::fs::write(&tmp, &plaintext).map_err(|e| format!("写入临时库失败: {e}"))?;

    let src = Connection::open(&tmp).map_err(|e| format!("打开临时库失败: {e}"))?;
    {
        let st = app.state::<crate::state::AppState>();
        let mut live = st.db()?;
        let backup = Backup::new(&src, &mut *live).map_err(|e| e.to_string())?;
        backup
            .run_to_completion(1000, std::time::Duration::from_millis(0), None)
            .map_err(|e| format!("导入数据库失败: {e}"))?;
    }
    drop(src);
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_derivation_is_stable_and_sensitive() {
        let k1 = derive_key("correct horse battery staple");
        let k2 = derive_key("correct horse battery staple");
        let k3 = derive_key("slightly different");
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
        assert_eq!(k1.len(), 32);
    }

    #[test]
    fn nonce_is_random() {
        let a = random_nonce().unwrap();
        let b = random_nonce().unwrap();
        assert_ne!(a, b);
    }
}
