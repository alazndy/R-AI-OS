# R-AI-OS Teknik Bulgular Raporu

Tarih: 2026-06-10

Bu rapor, `R-AI-OS` kod tabanında 2026-06-10 tarihinde yapılan mimari inceleme, derleme doğrulaması ve hedefli kod review sonucunda tespit edilen teknik bulguları özetler.

## Kapsam

- İncelenen alanlar:
  - `src/kernel.rs`
  - `src/daemon/server.rs`
  - `src/mcp/*`
  - `src/server/http.rs`
  - `src/security/*`
- Doğrulama:
  - Tam test paketi çalıştırıldı
  - Sonuç: `345` test geçti, `0` test başarısız

## Yönetici Özeti

Kod tabanı genel olarak modüler, test kapsamı güçlü ve güvenlik modeli net ayrılmış durumda. İlk turda bulunan iki yüksek öncelikli bulgu kapatıldı. İkinci turda bulunan Linux `powershell` bağımlılığı da giderildi. Aktif runtime yüzeyinde bu tur için açık kritik veya orta şiddette bulgu kalmadı.

Bu turda ayrıca daemon arka plan yükü için konfigüre edilebilir tuning katmanı eklendi. Windows varsayılanları artık daha sakin çalışıyor: eager Cortex indexing kapalı, periyodik Sentinel compile döngüsü kapalı, health/git/port polling aralıkları seyrekleştirildi.

Kapanan maddeler:

1. `get_validation_errors` yanlış negatif üretiyordu.
2. `ApproveFileChange` akışı workspace sınırını by-pass edebiliyordu.

Kalan not:

3. Bu tur sonunda aktif runtime yüzeyinde açık kritik veya orta şiddette bulgu bırakılmadı.

## Bulgular

### 1. `get_validation_errors` yanlış negatif üretebiliyor

- Şiddet: Yüksek
- Durum: Düzeltildi

#### Kanıt

- `src/daemon/server.rs:328-339`
  - `GetState` cevabı içinde `projects`, `health_reports`, `active_agents`, `index_ready`, `handover_count`, `pending_file_changes` alanları dönülüyor.
  - `latest_errors` alanı dönülmüyor.
- `src/mcp_server.rs:721-752`
  - `tool_get_validation_errors()` daemon'dan `GetState` çağırıyor.
  - Sonrasında `v["latest_errors"]` alanını okuyup boşsa `"No validation errors found."` dönüyor.

#### Etki

- Sistem derleme veya doğrulama hataları varken kullanıcıya temiz durum gösterebilir.
- Self-healing veya agent geri besleme akışları yanlış veriyle çalışabilir.
- Bu hata sessizdir; sistem explicit failure vermediği için fark edilmesi zordur.

#### Olası Kök Neden

- Daemon state şeması ile MCP tool beklentisi senkron değil.
- `ValidationError` verisi state içinde tutuluyor ancak serialization cevabına eklenmemiş.

#### Uygulanan Düzeltme

1. `DaemonState::sync_payload()` eklendi ve `latest_errors` tüm ana `StateSync` yayın yollarına dahil edildi.
2. `tool_get_validation_errors()` artık `latest_errors` alanı yoksa sessiz boş sonuç dönmek yerine protokol uyumsuzluğu hatası veriyor.
3. Bu akış için regresyon testleri eklendi.

#### İlgili Değişiklikler

- `src/daemon/state.rs`
- `src/daemon/git.rs`
- `src/daemon/health.rs`
- `src/daemon/server.rs`
- `src/mcp/tools_workspace.rs`

## 2. `ApproveFileChange` akışı sandbox sınırını by-pass edebiliyordu

- Şiddet: Yüksek
- Durum: Düzeltildi

#### Kanıt

- `src/daemon/server.rs:340-357`
  - `RequestFileChange` isteği gelen `path` değerini doğrudan queue'ya alıyor.
- `src/daemon/server.rs:358-365`
  - `ApproveFileChange` sırasında `approval.path` doğrudan `std::fs::write()` ile yazılıyor.
  - Burada `validate_path`, `SandboxGuard` veya `dev_ops_path` boundary kontrolü uygulanmıyor.
- Karşılaştırma için:
  - `src/server/http.rs:170-200`
  - HTTP approve akışında `pending_diffs` için canonicalize + `allowed_base.starts_with(...)` kontrolü var.

#### Etki

- Auth'lu ama yarı-güvenilir bir istemci daemon'a workspace dışı bir path enjekte edebilir.
- İnsan onayı alınsa bile onaylanan hedefin güvenlik politikasıyla uyumlu olduğu garanti edilmiyor.
- Bu, "approval queue" mantığını güvenlik sınırı değil sadece UX adımı haline düşürüyor.

#### Olası Kök Neden

- Eski `pending_file_changes` akışı ile yeni güvenlik kernel yaklaşımı arasında davranış farkı kalmış.
- Benzer iki approval path var ama biri hardened edilmiş, diğeri geride kalmış.

#### Uygulanan Düzeltme

1. `ApproveFileChange` akışına `SandboxGuard` tabanlı workspace ve blocked path doğrulaması eklendi.
2. Konfigürasyondan gelen `blocked_paths` bu akışa bağlandı.
3. "workspace dışı path reddedilir" ve "blocked path reddedilir" testleri eklendi.

#### İlgili Değişiklikler

- `src/daemon/server.rs`

## 3. Aktif MCP panic bulgusu yeniden değerlendirildi

- Şiddet: Bilgilendirme
- Durum: Daraltıldı

#### Not

İlk incelemede `src/mcp_server.rs` içinde `Cortex::init().unwrap()` kullanımları üzerinden bir runtime panic bulgusu not edilmişti. İkinci tur incelemede aktif stdio MCP yolunun `src/mcp/*` altında çalıştığı ve canlı path'in aynı çağrıları `map_err(...)` ile yönettiği doğrulandı.

Bu nedenle ilk panic bulgusu aktif runtime için açık risk değil, legacy / kullanılmayan kod yolu notu olarak yeniden sınıflandırıldı.

## 4. `ExecutionProxy` Linux `powershell` bağımlılığı

- Şiddet: Orta
- Durum: Düzeltildi

#### Kanıt

- `src/daemon/proxy.rs:68-70`
  - Agent spawn akışı `Command::new("powershell")` kullanıyor.
  - Çalıştırılacak ajan komutu `-Command` üzerinden PowerShell'e veriliyor.

#### Etki

- Linux ortamında `powershell` kurulu değilse agent spawn başarısız olur.
- Derleme/test yeşil olsa bile swarm / handover / agent orchestration davranışı pratikte çalışmayabilir.
- Bu, platform-destek iddiası ile runtime davranışı arasında sürtünme oluşturur.

#### Olası Kök Neden

- Process bridge katmanı Windows-first yazılmış.
- OS-seviyesi komut seçimi soyutlanmamış.

#### Uygulanan Düzeltme

1. Spawn katmanı platform ailesine göre ayrıldı.
2. Windows için `powershell -Command`, Unix için `sh -lc` kullanılıyor.
3. Platform seçimi için birim test eklendi.

#### İlgili Değişiklikler

- `src/daemon/proxy.rs`

## Önceliklendirme

Bu tur sonunda öncelikli düzeltme kuyruğunda açık madde bırakılmadı.

## Notlar

- İnceleme sırasında derleme ortamındaki OpenSSL bağımlılık zinciri sadeleştirildi.
- Ayrıca sürüm parser'ı `2.0.0-alpha` gibi prerelease semver formatlarını kabul edecek şekilde düzeltildi.
- Bu iki değişiklik review bulgusu değil, build doğrulamasını tamamlamak için yapılan bakım değişiklikleridir.
- `server/http.rs` içinde diff approval path çözümleyicisi de `unwrap()` varsayımlarından arındırıldı ve test eklendi.
- Kullanılmayan legacy `src/mcp_server.rs` dosyası kaldırıldı; aktif MCP yüzeyi `src/mcp/*` altında bırakıldı.
- Daemon tuning ayarları `Config.daemon` altına taşındı; README içinde örnek `config.toml` bloğu eklendi.
