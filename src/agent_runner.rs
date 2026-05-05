use std::process::Command;
use std::time::{Duration, Instant};
use std::thread;

pub fn run_agent(agent: &str, project_dir: Option<String>, timeout_secs: Option<u64>) -> Result<(), String> {
    let mut cmd = match agent.to_lowercase().as_str() {
        "claude" => {
            let mut c = Command::new("claude");
            // Sadece Anthropic API key izni ver, diğerlerini kaldır
            c.env_remove("GEMINI_API_KEY");
            c.env_remove("OPENAI_API_KEY");
            c
        },
        "gemini" => {
            let mut c = Command::new("gemini");
            // Sadece Gemini API key izni ver
            c.env_remove("ANTHROPIC_API_KEY");
            c.env_remove("OPENAI_API_KEY");
            c
        },
        "cursor" => {
            Command::new("cursor")
        },
        "antigravity" => {
            Command::new("antigravity")
        },
        _ => return Err(format!("Desteklenmeyen ajan: {}. Desteklenenler: claude, gemini, cursor, antigravity", agent)),
    };

    if let Some(dir) = project_dir {
        cmd.current_dir(dir);
    }
    
    // Process'i başlat
    println!("🚀 Ajan başlatılıyor: {}...", agent);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Err(format!("Ajan başlatılamadı: {}", e)),
    };
    
    // Timeout (Death Timer) izleme mekanizması
    if let Some(timeout) = timeout_secs {
        println!("⏱️ Death timer aktif: {} saniye.", timeout);
        let start = Instant::now();
        loop {
            // Process bitti mi diye kontrol et
            match child.try_wait() {
                Ok(Some(status)) => {
                    println!("✅ Ajan ({}) çıkış yaptı: {}", agent, status);
                    return Ok(());
                }
                Ok(None) => {
                    // Hala çalışıyor, timeout kontrolü
                    if start.elapsed().as_secs() > timeout {
                        println!("💀 ZAMAN AŞIMI! Ajan ({}) limitini doldurdu. Zorla kapatılıyor...", agent);
                        let _ = child.kill();
                        return Err(format!("Agent {} timed out after {} seconds.", agent, timeout));
                    }
                    // 250ms bekle ve tekrar kontrol et
                    thread::sleep(Duration::from_millis(250));
                }
                Err(e) => return Err(format!("Process durum hatası: {}", e)),
            }
        }
    } else {
        // Timeout yoksa normal bir şekilde bitmesini bekle
        match child.wait() {
            Ok(status) => {
                println!("✅ Ajan ({}) çıkış yaptı: {}", agent, status);
                return Ok(());
            }
            Err(e) => return Err(format!("Process bekleme hatası: {}", e)),
        }
    }
}
