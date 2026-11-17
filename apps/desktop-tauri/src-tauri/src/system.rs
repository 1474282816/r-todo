use std::path::{Path, PathBuf};

#[tauri::command]
pub async fn read_file_bytes(path: String) -> Result<Vec<u8>, String> {
    tokio::fs::read(path)
        .await
        .map_err(|error| format!("读取文件失败: {error}"))
}

#[tauri::command]
pub async fn copy_file(src: String, dest: String) -> Result<(), String> {
    tokio::fs::copy(&src, &dest)
        .await
        .map(|_| ())
        .map_err(|error| format!("复制文件失败: {error}"))
}

#[tauri::command]
pub async fn file_exists(path: String) -> bool {
    tokio::fs::metadata(path).await.is_ok()
}

#[tauri::command]
pub async fn show_in_folder(path: String) -> Result<(), String> {
    let target = PathBuf::from(&path);
    if !target.exists() {
        return Err("文件不存在".into());
    }
    std::process::Command::new("explorer.exe")
        .arg("/select,")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn open_path(path: String) -> Result<(), String> {
    ensure_exists(&path)?;
    std::process::Command::new("rundll32.exe")
        .arg("url.dll,FileProtocolHandler")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

fn ensure_exists(path: &str) -> Result<(), String> {
    if Path::new(path).exists() {
        Ok(())
    } else {
        Err("文件不存在".into())
    }
}
