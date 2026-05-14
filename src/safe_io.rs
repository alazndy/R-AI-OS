use anyhow::Result;
use fd_lock::RwLock;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::Duration;

/// memory.md gibi kritik dosyalara güvenli (locked) yazma yapar.
/// Eğer dosya kilitliyse, belirtilen deneme sayısı kadar bekler.
pub fn safe_write(path: &Path, content: &str) -> Result<()> {
    // 1. If Daemon is running, delegate to approval workflow
    if let Ok(mut stream) = std::net::TcpStream::connect("127.0.0.1:42069") {
        let original = std::fs::read_to_string(path).unwrap_or_default();
        let msg = serde_json::json!({
            "command": "RequestFileChange",
            "path": path.to_string_lossy(),
            "original": original,
            "new": content,
            "agent": "Agent (via SafeIO)"
        });
        let _ = stream.write_all(format!("{}\n", msg).as_bytes());
        println!(
            "[SafeIO] File change for {:?} sent to Daemon for approval.",
            path
        );
        return Ok(());
    }

    // 2. Fallback to direct write if Daemon is not running
    let file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?;

    let mut lock = RwLock::new(file);

    // Kilidi alana kadar dene (Max 5 saniye)
    let mut retries = 0;
    let mut locked_file = loop {
        match lock.try_write() {
            Ok(guard) => break guard,
            Err(_) => {
                if retries > 50 {
                    return Err(anyhow::anyhow!(
                        "Dosya kilidi alınamadı (Timeout): {:?}",
                        path
                    ));
                }
                thread::sleep(Duration::from_millis(100));
                retries += 1;
            }
        }
    };

    locked_file.write_all(content.as_bytes())?;
    locked_file.flush()?;

    Ok(())
}

/// Dosyayı güvenli bir şekilde okur (Read Lock).
#[allow(dead_code)]
pub fn safe_read(path: &Path) -> Result<String> {
    if !path.exists() {
        return Ok(String::new());
    }

    let file = File::open(path)?;
    let lock = RwLock::new(file);

    // Read lock için try_read yeterli, ancak Read trait &mut File ister.
    // fd-lock'ta guard üzerinden &File alıp direkt fs::read_to_string kullanamayız,
    // bu yüzden shared kilit olsa bile Read işlemi için guard'ın mutabilitesini sağlamalıyız.
    let _locked_file = loop {
        match lock.try_read() {
            Ok(guard) => break guard,
            Err(_) => {
                thread::sleep(Duration::from_millis(50));
            }
        }
    };

    // RwLockReadGuard does not implement DerefMut, so we can't use Read::read_to_string.
    // Instead, we use the fact that we have the lock to safely read via standard fs.
    let content = std::fs::read_to_string(path)?;
    Ok(content)
}
