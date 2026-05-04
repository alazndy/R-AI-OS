# R-AI-OS

AI OS Terminal Control Center — **Rust Edition**

Rust + [ratatui](https://ratatui.rs) ile sıfırdan tasarlanmış terminal kontrol paneli. Go/BubbleTea tabanlı `aios`'un yeniden tasarımı; daha temiz mimari, güçlü tip sistemi, sıfır bağımlılık ile dosya düzenleyici.

## Özellikler

| Özellik | Açıklama |
|---|---|
| Boot animasyonu | Sistem dosyası taraması + progress gauge |
| Dashboard | 6 menü öğesi, sol/sağ panel navigasyonu |
| Dosya gezgini | Kural, agent config, memory, mempalace dosyaları |
| Dosya görüntüleyici | Satır numaralı, kaydırılabilir, syntax-aware |
| Dosya düzenleyici | Custom line editor, Ctrl+S kaydet |
| CLI subcommands | Agent'ların AIOS'u script gibi kullanması için |

## Kullanım

### TUI

```bash
cargo run --release
# veya
./raios
```

### CLI (Agent araç modu)

```bash
raios rules                   # Tüm master rule dosyaları
raios rules hardware          # hardware-rules.md içeriği
raios memory                  # Son 5 memory.md
raios memory aios             # aios/memory.md
raios mempalace               # mempalace.yaml
raios projects                # Tüm proje listesi
raios agents                  # Agent config durumu
raios view GEMINI.md          # Herhangi dosyayı yazdır
```

## TUI Kısayollar

### Dashboard
| Tuş | Eylem |
|---|---|
| `↑` / `↓` veya `j` / `k` | Menü gezin |
| `→` / `l` | Dosya listesine odaklan |
| `←` / `h` | Menüye dön |
| `Enter` | Seçili dosyayı görüntüle |
| `e` | Seçili dosyayı düzenle |
| `o` | Harici editörde aç |
| `/` veya `Tab` | Komut girişi |
| `q` | Çıkış |

### Komutlar
| Komut | Eylem |
|---|---|
| `/rules` | System Rules + dosya listesi |
| `/memory` | MemPalace + memory dosyaları |
| `/mempalace` | mempalace.yaml görüntüle |
| `/view <isim>` | Dosyayı bul ve görüntüle |
| `/edit <isim>` | Dosyayı bul ve düzenle |
| `/sync` | Universe sync (MASTER.md link) |

### Dosya Görüntüleyici
| Tuş | Eylem |
|---|---|
| `↑` / `↓` | Satır satır kaydır |
| `PgUp` / `PgDn` | Sayfa kaydır |
| `e` | Düzenleme moduna geç |
| `Esc` / `q` | Geri dön |

### Dosya Düzenleyici
| Tuş | Eylem |
|---|---|
| Normal tuşlar | Yazım |
| `Ctrl+S` | Kaydet |
| `Ctrl+Q` / `Esc` | İptal |

## Kurulum

```bash
# Rust kurulu değilse:
# https://rustup.rs

cargo build --release
# Binary: target/release/raios.exe
```

## Mimari

```
src/
├── main.rs          Entry point — CLI dispatch veya TUI başlat
├── app.rs           App state machine, Editor, BgMsg kanalı
├── ui.rs            ratatui rendering (boot, dashboard, viewer, editor)
├── filebrowser.rs   FileEntry, dosya keşfi, load/save
├── discovery.rs     Agent/skill keşfi
├── sync.rs          Universe sync protokolü
└── cli.rs           Clap subcommand handler'ları
```

**Stack:** Rust 2021 · ratatui 0.29 · crossterm 0.28 · clap 4 · walkdir 2 · anyhow
