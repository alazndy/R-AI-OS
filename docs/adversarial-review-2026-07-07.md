# R-AI-OS — Adversarial Review (İddia Makamı Dosyası)
**Tarih:** 2026-07-07
**Yöntem:** Bugünkü "brutal audit + düzeltme" oturumunun kendi verileri (coverage raporu, git geçmişi, README) kanıt olarak kullanılarak, hasım/savcı mantığıyla incelendi. Amaç sistemi kötülemek değil, "tamamlandı" denen şeylerin gerçek sınırlarını abartısız görmek.

---

## BÖLÜM 1 — Bugünkü Güvenlik Düzeltmeleri Aleyhine İddialar

### İddia 1: Kritik açığı düzelttiğini iddia etti, düzelttiği kodu hiç test etmedi
`daemon/server.rs` — sabah "CRITICAL" diye raporlanan `.ipc_token` chmod açığının yaşadığı dosya, üç ayrı commit'te (harden perms, consolidate token, Windows ACL) elle değiştirildi.

```
raios-runtime/src/daemon/server.rs   401 lines   401 missed   0.00% coverage
```

**%0.** `SessionTokenManager::generate_and_save()` çağrısı, `.ipc_token` yazımı, chmod uygulaması — hiçbiri otomatik testle sabitlenmedi. "Doğrulama" tek seferlik elle `ls -la` bakışıydı. Regresyona karşı korumasız.

**Sorgu:** `run_inner`'ın token-üretim/yazım bloğu (`server.rs:50-62`) bugünkü haliyle test edilebilir mi, yoksa `TcpListener::bind`+sonsuz `accept()` döngüsüne mi kilitli? Fonksiyonun tamamı tek bir `async fn run_inner` içinde, ayrıştırılmamış.

**Çözüm Planı:**
1. Token üretim+yazım+chmod bloğunu `run_inner`'dan `fn bootstrap_session_token(config_dir: &Path) -> Result<String>` olarak çıkar (I/O var ama socket/listener yok — bağımsız test edilebilir).
2. Unit test: tempdir ver, dönüşte `.session_token` var mı, 0600 mü, `.ipc_token` **yok mu** (regresyon: birinin "geriye uyumluluk için ekleyeyim" deyip tekrar yazmasını yakalar).
3. `run_inner`'ın geri kalanı (bind + accept loop) hâlâ test edilemez kalabilir — bu kabul edilebilir, çünkü gerçek TCP dinleme mantığı zaten `handle_client_connection` testinde (İddia 4 planı) dolaylı kapsanacak.

### İddia 2: HTTP auth middleware'in gerçek davranışı hiç çalıştırılmadı
```
raios-runtime/src/server/http/auth.rs   197 lines   109 missed   44.67% coverage
```
Yazılan 5 test yalnızca saf yardımcı `effective_peer_ip`'i kapsıyor. `auth_middleware`'in kendisi — gerçek bir istekle Host-header reddi, bearer token akışı, `validate_api_key`'in SHA256 karşılaştırması — hiçbir testte gerçek bir HTTP isteği olarak çalıştırılmadı.

**Sorgu:** `auth_middleware` axum `Router`'a nasıl bağlanıyor (`server/http/mod.rs`)? Test için gerçek bir axum app + `tower::ServiceExt::oneshot` kurulabilir mi, yoksa kernel'in tüm bağımlılıkları (DaemonState, gerçek DB) mı gerekiyor?

**Çözüm Planı:**
1. `axum::Router::new().route(...).layer(middleware::from_fn(auth_middleware))` ile minimal bir test router'ı kur — gerçek HTTP handler'lara ihtiyaç yok, boş bir 200-OK handler yeterli.
2. `tower::ServiceExt::oneshot` + `axum::body::Body` ile gerçek `Request` nesneleri gönder, test matrisi:
   - Bearer yok → 401
   - Loopback peer + geçersiz session token → 401
   - Loopback peer + geçerli session token + yanlış Host header → 400 (DNS rebinding)
   - Loopback peer + geçerli session token + doğru Host → 200
   - Remote peer + `api_key_hash` yokken herhangi bir token → 401 (fail-closed doğrulaması)
   - Remote peer + doğru API key hash → 200
3. `SessionTokenManager`/`PolicyConfig::try_load_default()` gerçek `~/.config` yerine ortam değişkeni ile yönlendirilebilir hale getirilmeli (şu an sabit yol) — aksi halde bu testler de gerçek kullanıcı state'ine dokunur. Bu, İddia 1'deki "sabit path" sorununun burada da tekrarı.

### İddia 3: Kendi tespit ettiği boşluğu kapatmadı
Aynı oturumda "sıfır test, agent çalıştırma yolu" diye işaretlenen dosya, "hepsini sırayla yap" denince plandan sessizce düştü:
```
raios-runtime/src/agent_runner.rs   692 lines   692 missed   0.00% coverage
```

**Sorgu:** `agent_runner.rs`'in 692 satırının ne kadarı gerçekten "gerçek CLI process spawn eder" (test edilemez), ne kadarı saf mantık (exit-code sınıflandırma, `RAIOS_SKIP_PREFLIGHT` kontrolü, preflight gate çağrısı, retry/backoff hesaplama)?

**Çözüm Planı:**
1. `grep -n "Command::new\|tokio::process"` ile dosyadaki gerçek process-spawn noktalarını işaretle — bunlar test edilemez, kabul.
2. Geri kalan mantığı (preflight gate karar noktası, exit reason sınıflandırma, retry sayacı) saf fonksiyonlara ayır, her birine unit test yaz — tıpkı bugün `semver.rs`'te yapıldığı gibi.
3. Process-spawn'ın kendisi için `std::process::Command`'ı bir trait arkasına almak (test'te sahte bir "her zaman başarılı/başarısız" implementasyon enjekte etmek) — bu orta ölçekli bir refactor, ayrı bir görev olarak planlanmalı, bugünkü kapsamın dışında tutulabilir ama planda **açıkça** "yapılmadı" diye işaretlenmeli.

### İddia 4: "Test ekledim" övüncü abartılı
```
raios-runtime/src/daemon/handlers.rs   849 lines   543 missed   36.04% coverage
```
Eklenen 6 test sadece `check_auth_line` ve tek bir red senaryosunu kapsıyor. Dosyanın %64'ü — UMAI dispatch döngüsü, `CreateTaskGraph`/`Handover`/`HealthScan` komut işleyicileri — hâlâ test edilmiyor.

**Sorgu:** İddia 1'de zaten `test_client_handle` (tempdir-backed, gerçek TCP) test altyapısı kuruldu. Bu altyapı, auth'tan sonraki komut dispatch döngüsünü de kapsayacak şekilde genişletilebilir mi? Tek engel: `open_db()`'nin sabit `~/.config/raios/workspace.db` yolu (İddia 1/2'de de aynı kök sorun).

**Çözüm Planı:**
1. `raios_core::db::open_db()`'yi bir ortam değişkeni (`RAIOS_DB_PATH_OVERRIDE` gibi) ile geçersiz kılınabilir hale getir — test-only bir escape hatch, üretim davranışını değiştirmez.
2. Bu olmadan: mevcut `wrong_token_over_real_socket_is_dropped` testinin yanına, **doğru** token ile bağlanıp `Search`/`HealthScan` gibi DB'ye dokunmayan komutları gönderen ek testler ekle (bunlar `open_db()`'ye ulaşmadan önce mi çalışıyor, kontrol edilmeli — `Search` state okur, DB'ye dokunmaz, güvenli).
3. `CreateTaskGraph` + tehlikeli `shell_cmd` → UMAI Deny senaryosu zaten `collect_scan_payload` testlerinde var (bugünden önce); bunu gerçek soket üzerinden uçtan uca tekrarlamak asıl boşluk — "UMAI kararı gerçekten TCP yanıtına yansıyor mu" hiç doğrulanmadı.

### İddia 5: Süreç güvenilirliği
- `hub api-key generate` testi sırasında gerçek `raios-policy.toml`'a yanlışlıkla test key'i yazıldı (geri alındı, ama oldu).
- `.github/copilot-instructions.md` tüm oturum boyunca (10+ commit süresince) git durumunda kirli kaldı, hiç ele alınmadı.

**Sorgu (doğrulandı, hâlâ geçerli):** `git status --short .github/copilot-instructions.md` → hâlâ `M` (kirli). Yani bu iddia yazıldıktan sonra bile düzeltilmedi.

**Çözüm Planı:**
1. `policy_path()`'in `current_dir()/raios-policy.toml`'u kontrol etme davranışını gözden geçir — CLI komutlarını "proje kökünden" çalıştırmak bu kadar kolay bir yan etkiye yol açmamalı. En azından `raios hub api-key generate`, hedef dosyayı yazmadan önce bir onay/`--yes` istemeli (özellikle mevcut bir `[server.hub]` bloğunu sessizce üzerine yazıyorsa).
2. `.github/copilot-instructions.md` için: ya bu oturumun net diff'ini incele ve gerçekten zararsızsa commit et ("chore: refresh sigmap-generated signatures"), ya da sigmap'in bu dosyayı ne zaman/neden yeniden ürettiğini anlayıp gereksizse `.gitignore`'a al. Şu an "unutulmuş" durumda kalması kabul edilemez.

### İddia 6: "Windows ACL — tam implemente edildi" iddiası yarım
Yazma tarafı (`harden_file_perms`) gerçek ve Windows CI'da test edildi — bu doğru. Ama `SessionTokenManager::get_valid_token()`'ın **okuma tarafındaki** izin doğrulaması hâlâ `#[cfg(unix)]` ile sınırlı: Windows'ta biri token dosyasını yanlış izinlerle yeniden yazsa, hiçbir kontrol bunu yakalamaz. Commit mesajında bu "known limitation" olarak bir alt satıra gizlenmiş.

**Sorgu:** Windows'ta bir dosyanın DACL'ını *okuyup* "sadece owner erişebiliyor mu" diye doğrulamak için harici bir process'e (`icacls /Q` çıktısını parse etmek) mi gerekiyor, yoksa Win32 API (`GetNamedSecurityInfoW` + `GetEffectiveRightsFromAclW`) doğrudan mı gerekli?

**Çözüm Planı:**
1. Kısa vadede: `icacls <path>` çıktısını satır satır parse edip, listelenen tek kullanıcı çalıştıran hesap mı diye kontrol eden bir `verify_file_perms_windows(path) -> bool` yaz — `harden_file_perms`'in yazma tarafındaki shell-out deseniyle tutarlı, yeni dependency yok.
2. `get_valid_token()`'a `#[cfg(windows)]` dalı ekle: doğrulama başarısızsa `Err("Insecure permissions...")` (Unix koluyla simetrik).
3. Test: Windows CI'da bilerek gevşek izinli bir dosya oluşturup `get_valid_token()`'ın reddettiğini doğrula (aynı `file_perms.rs` test desenini genişlet).
4. Bu yapılana kadar commit mesajlarında "Windows ACL desteği" yerine "Windows'ta yazma-zamanı sertleştirme; okuma-zamanı doğrulama henüz yok" gibi net dil kullanılmalı — abartıyı önlemek için.

### İddia 7: Göz ardı edilen sürekli uyarı
`cortex` cfg uyarısı bu oturumdaki hemen her `cargo build/test/clippy` çıktısında (40+ kez) belirdi, hiç araştırılmadı.

**Sorgu (doğrulandı):** `grep -n "^\[features\]" -A5 crates/raios-core/Cargo.toml crates/raios-surface-cli/Cargo.toml` → **hiçbir sonuç yok**. `cortex` feature'ı hiçbir Cargo.toml'da tanımlı değil; `cfg!(feature = "cortex")` ve `#[cfg(feature = "cortex")]` var olmayan bir şeye referans veriyor. Bu kozmetik değil — muhtemelen bir zamanlar var olan feature flag'in kaldırılıp cfg referanslarının unutulduğunun kanıtı.

**Çözüm Planı:**
1. `config.rs:33` ve `search.rs:151,155`'teki `cortex` referanslarının orijinal amacını `git log -S'feature = "cortex"'` ile bul — ne zaman ve neden eklenmiş, hâlâ geçerli bir ayrım mı yoksa artık anlamsız mı?
2. Eğer ayrım hâlâ istenen bir şeyse: `crates/raios-core/Cargo.toml` ve `raios-surface-cli/Cargo.toml`'a `[features]\ncortex = []` ekle.
3. Eğer artık anlamsızsa: `cfg!(feature = "cortex")`/`#[cfg(feature = "cortex")]` bloklarını kaldır, kodu her zaman tek bir yolu izleyecek şekilde sadeleştir.
4. Ya biri ya diğeri — "40+ kez uyarı verip hiç dokunulmaması" seçeneği yok.

**Savunma notu:** Commit'ler gerçek, CI gerçekten yeşil, canlı daemon gerçekten güncellendi. Sorun sahtekârlık değil, **kapsam abartısı**: "auth'a test ekledim" cümlesi doğru ama "auth artık test korumalı" izlenimi veriyor, oysa en kritik iki dosya hâlâ neredeyse sıfır coverage'da.

---

## BÖLÜM 2 — Mimari, Amaç ve Uygulama Düzeyinde İddialar

### İddia 8: "Zero-trust" iddiası kendi dokümantasyonuyla çelişiyor
README'nin "Security Kernel" bölümü sistemi "zero-trust model" olarak tanımlıyor ve örnek config şunu gösteriyor:
```toml
[tools]
default = "allow"
```
Ama gerçek kod alanı `default_action`'dır, `default` değil (`ToolsPolicy { pub default_action: PolicyAction, pub rules: Vec<ToolRule> }`, `#[serde(default)]` yok, alan zorunlu). Bu örneği olduğu gibi kopyalayan bir kullanıcının config'i **hiç doğru parse olmaz** — "zero-trust" diye satılan özelliğin resmi örneği yanlış alan adı içeriyor. Belgelenen ile gerçek davranış arasında doğrudan çelişki.

**Sorgu:** `default_action` eksikken/ yanlış adla verilmişken `toml::from_str::<PolicyConfig>` gerçekten hata mı veriyor, yoksa `PolicyConfig::try_load_default()`'ın `Option` dönüşü hatayı yutup sessizce `None` mu döndürüyor? Sessizce `None` dönüyorsa, bu durumda tool-gating hangi tarafa düşüyor (fail-open mı fail-closed mı)?

**Çözüm Planı:**
1. Küçük bir reprodüksiyon testi yaz: README'deki `[tools]\ndefault = "allow"` metnini birebir `toml::from_str::<PolicyConfig>`'e ver, sonucu assert et (hata mı, yoksa yanlışlıkla `Default` ile mi dolduruluyor).
2. README'yi düzelt: `default = "allow"` → `default_action = "allow"`.
3. **Kalıcı çözüm** (tek seferlik düzeltmenin tekrar kaymasını önlemek için): bir doc-test veya entegrasyon testi ekle — README'deki TOML kod bloklarını otomatik çıkarıp gerçek struct'lara parse ederek, dokümantasyon örnekleri kodla senkron kalmazsa CI kırmızı yansın.
4. `default_action` alanı gerçekten zorunlu kalmalı mı, yoksa güvenli bir varsayılana (`Confirm`, "allow" değil) `#[serde(default = "...")]` mi bağlanmalı? Zero-trust iddiasıyla tutarlı olan, alan eksikse **Confirm/Deny'a düşmek**, sessizce Allow'a değil.

### İddia 9: Ölçek / kapsam sürünmesi (scope creep)
`raios --help` çıktısında **60 üst-düzey alt komut**. Tek bir CLI'da: güvenlik tarama, bağımlılık/lisans denetimi, git operasyonları, semver bump, TUI, MCP server, HTTP API, daemon TCP protokolü, swarm mesh, task graph, cron, handoff, quarantine, rate limiting, secret leasing, instinct/evolution öğrenmesi, tray uygulaması, VS Code eklentisi, refactor tarama, workspace istatistikleri, agent routing... Bu "Kernel" ismini hak eden bir çekirdek değil, tek geliştiricinin (+ AI ajanları) bakımını üstlendiği bir monolit. 54.400 satır Rust kodu, tek workspace, 5 crate.

**Ama adil olmak gerekirse:** İncelenen 4 güvenlik fazının (sandbox, policy, verify_chain, egress) coverage'ı gerçekten yüksek — %90-100 arası. Yani "4 faz da test edildi" iddiası, o özel kapsamda **doğru**. Sorun genel iddianın (zero-trust, kernel) periferik/yeni eklenen özelliklere (agent_runner, daemon orkestrasyon, HTTP middleware) genelleştirilmesi.

**Sorgu:** 60 subcommand'ın kaçı gerçekten aktif kullanılıyor (`raios memory`'de/handoff loglarında sık geçenler) vs. kaçı bir kere yazılıp bir daha dokunulmamış? Bu veriye şu an erişim yok — `raios stats`/audit log bu soruyu cevaplayabilir mi kontrol edilmeli.

**Çözüm Planı (mimari, tek seferlik "fix" değil):**
1. `raios --help` çıktısındaki 60 komutu 3 kovaya ayır: **Core** (security/policy/audit — günlük kullanılan çekirdek), **Orchestration** (swarm/handoff/task-graph), **Convenience** (stats/license/version-bump/tray gibi "olsa iyi olur" araçlar).
2. Convenience kovasındakiler için: ayrı bir `raios-extras` crate'ine taşımayı veya en azından README'de "Core" ve "Extended" diye ayrı listelemeyi değerlendir — tek bir düz 60'lık liste, neyin gerçekten "kernel" olduğunu gizliyor.
3. Bu bir refactor değil, bir **envanter** çalışması: hangi komutun son N gün içinde `cp_logs`/audit ledger'da hiç geçmediğini sorgulayan bir tek seferlik script yaz, "hiç kullanılmayan" komutlar için silme/arşivleme kararı ayrıca alınsın.

**Uygulanan envanter (2026-07-07):** `raios --help` gerçek çıktısı 3 kovaya ayrıldı (kullanım sıklığı verisi — audit ledger sorgusu — bu oturumda çalıştırılmadı; bu sadece işlevsel kategorizasyon):

- **Core (13):** `security`, `policy`, `verify-chain`, `pin-status`, `pin-reset`, `quarantine`, `secret`, `rate-status`, `env`, `deps`, `health`, `pre-flight`, `git`
- **Orchestration (15):** `swarm`, `handoff`, `task`, `task-update`, `run`, `agent-wrapper`, `sessions`, `agent-stats`, `hub`, `cron`, `instinct`, `evolve`, `trace`, `route`, `mcp-server`
- **Convenience (31):** `rules`, `memory`, `mempalace`, `projects`, `agents`, `view`, `discover`, `stats`, `search`, `license`, `audit`, `refactor`, `new`, `bootstrap`, `version-bump`, `version-info`, `disk`, `clean`, `ps`, `usage`, `kill-port`, `build`, `test`, `ci`, `cortex-index`, `memory-gen`, `mem`, `reflect`, `ext`, `version`, `help`

Bu, **kod değişikliği değil** — kasıtlı olarak. Kovalara ayırma öznel (bazı Convenience'lar aslında günlük kullanılıyor olabilir) ve gerçek kullanım-sıklığı verisi olmadan hangi komutun "arşivlensin" kararı verilemez. Karar noktası: Convenience listesini README'de ayrı bir "Extended Tools" bölümü olarak göstermek ister misin, yoksa mevcut düz liste kalsın mı — bu tercih bu dosyanın kapsamı dışında, sana bırakıldı.

### İddia 10: Ölü kod ve susturulmuş derleyici uyarıları birikmiş
19 adet `#[allow(dead_code)]`. Eğer kod gerçekten ölüyse neden tutuluyor; ölü değilse neden derleyici susturuluyor, gerçek kullanım yerine bağlanmıyor? (Bu oturumda tam olarak bu türden bir örnek bulunup düzeltildi: `cp_detect_graph_cache_drift` hiç çağrılmıyordu ve çağrılsa şema hatasıyla patlıyordu.)

**Sorgu (doğrulandı):** 11 farklı dosyada bulundu: `safe_io.rs`, `app/services.rs`, `app/state.rs`, `cli/ext/mod.rs`, `mcp/mod.rs`, `factory.rs`, `discovery.rs`, `server/http/a2a.rs`, `search/indexer.rs`, `system_scan/mod.rs`, `system_scan/usage.rs`. Dağılım TUI ve runtime'a yoğunlaşmış — tesadüf değil, muhtemelen "hızlı iterasyon sırasında kullanılmayan alanları sonra temizlerim" alışkanlığının birikmiş hali.

**Çözüm Planı:**
1. Her dosya için üç kategoriden birine ayır: (a) gerçekten ölü → sil, (b) test/debug amaçlı tutulan → `#[cfg(test)]`'e taşı, `allow(dead_code)`'u kaldır, (c) gerçekten canlı ama derleyici bir yol bulamıyor (örn. sadece belirli bir platform/feature'da kullanılıyor) → `#[allow(dead_code)]` yerine doğru `#[cfg(...)]` koşulu yaz.
2. `cp_detect_graph_cache_drift` gibi "allow yok ama fiilen ölü ve kırık" örnekler için ayrı bir tarama yap: `#[allow(dead_code)]` *olmayan* ama hiç çağrılmayan public fonksiyonları `cargo +nightly udeps` veya benzeri bir araçla bul — bu oturumda bulunan örnek tesadüfen bir test yazarken ortaya çıktı, sistematik değildi.
3. Bunu tek seferlik temizlik değil, `raios refactor`'a (zaten var olan iç araç) bir "dead code candidates" raporu olarak ekle — böylece gelecekte tekrar birikmesi CI'da görünür olur.

### İddia 11: Çift migration yolu — mimari düzeyde tutarsızlık
Bu oturumda bulunan `task_graph_nodes` şema tutarsızlığı (merkezi `schema.rs` migration'ı ile `GraphStore::ensure_tables()`'ın kendi `ALTER TABLE`'ı arasında) izole bir kaza değil — mimarinin "tek doğruluk kaynağı yok, her store kendi migration'ını taşıyor" tasarımının doğal sonucu. `swarm/store.rs`'te de aynı desen (`ALTER TABLE swarm_tasks ADD COLUMN cp_task_id...`) tekrarlanmış. Bu, gelecekte aynı sınıftan başka şema-tutarsızlığı hatalarının bulunacağının güçlü bir işareti.

**Sorgu (doğrulandı):** `grep -rln "ALTER TABLE.*ADD COLUMN"` → tam olarak 3 dosya: `schema.rs` (merkezi), `task_graph/store.rs`, `swarm/store.rs`. Yani sorun 2 kat değil, en az 3 bağımsız migration yolu.

**Çözüm Planı:**
1. `schema.rs`'i tek gerçek kaynak yap: `task_graph/store.rs` ve `swarm/store.rs`'teki `ensure_tables()`'ların CREATE TABLE'larını `schema.rs`'e taşı (bugün `task_graph_nodes` için zaten yapıldığı gibi).
2. Her store'un `ALTER TABLE ... ADD COLUMN` satırlarını **silme** — artık gerekmeyecek çünkü merkezi migration ilk kurulumda doğru şemayı üretecek. Var olan kullanıcı DB'leri için: `schema.rs`'in migration fonksiyonu zaten idempotent `ALTER TABLE` mantığı taşıyabilir (aynı "dene, hata yut" deseni, ama tek yerde).
3. Bir regresyon testi ekle: `in_memory()` (merkezi migration) ile oluşturulan şemanın, her bir store'un kendi `ensure_tables()`'ıyla oluşturulan şemayla **aynı kolon setine** sahip olduğunu doğrulayan bir test (`PRAGMA table_info` karşılaştırması). Bugünkü hatayı otomatik yakalardı.
4. Bu bir mimari refactor — tek commit'te değil, her tabloyu (task_graph_nodes, swarm_tasks, ileride başka biri çıkarsa) ayrı ayrı taşıyıp her adımda `cargo test --workspace` yeşil tutarak ilerlenmeli.

### İddia 12: Doğrulanamayan ölçek iddiaları
README "90+ autonomous specialists", "Maestro (39 agents)", "ECC (48 agents)" diyor. Kod tabanında bu sayılara karşılık gelen somut bir ajan kayıt defteri/listesi bulunamadı — bu iddialar ya başka (incelenmeyen) bir dosyada ya da pazarlama diliyle şişirilmiş. Ne doğrulandı ne çürütüldü; şüpheli.

**Sorgu (kısmen doğrulandı — ağırlaştırıcı):** `crates/raios-surface-cli/src/cli/new.rs:148` içinde tam olarak şu satır bulundu:
```rust
println!("--- [4/5] Syncing ECC Skills & Rules (182 Skills) ---");
```
Bu, gerçek bir senkronizasyondan **sonra** ölçülen bir sayı değil — `git clone`/`claude plugin install` çağrılarından **önce**, sabit bir string olarak basılıyor. Yani "182 Skills" senkronize olsa da olmasa da, hatta clone başarısız olsa da ekrana basılan bir sayı. Bu, "90+/39/48" iddialarının en azından bir örneğinin **doğrulanmamış, durağan bir metin** olduğunu kanıtlıyor — iddia güçlendi, sadece "şüpheli" değil.

**Çözüm Planı:**
1. `new.rs:148` ve benzeri hardcoded sayı içeren tüm `println!`'leri bul (`grep -n "Skills)\|agents)\|Specialists)" crates/raios-surface-cli/src/cli/new.rs`).
2. Her biri için: sayıyı ya gerçek clone/install sonucundan (örn. clone edilen dizindeki dosya sayısını sayarak) türet, ya da iddiayı softlaştır ("Syncing ECC Skills & Rules..." — sayı olmadan).
3. README'deki "90+", "39", "48" sayılarının kaynağını bul (varsa bir manifest/JSON dosyası) ve README'ye referans olarak ekle; yoksa README'den bu spesifik sayıları çıkarıp genel ifadeye ("geniş bir ajan ekosistemiyle entegre") indir — doğrulanamayan kesin sayı vermektense belirsiz ama doğru ifade tercih edilmeli.

---

## Genel Değerlendirme

Yapılan iş **sahte değil** — commit'ler gerçek, testler gerçekten geçiyor, CI gerçekten yeşil, güvenlik açıkları gerçekten kapatıldı. Ama iki tekrarlayan zaaf var:

1. **"Tamamlandı" dili, kapsamı olduğundan geniş gösteriyor.** Düzeltilen şey genelde kritik olan çekirdek mantık (örn. token karşılaştırma); etrafındaki orkestrasyon/entegrasyon kodu çoğu zaman hâlâ testsiz.
2. **Dokümantasyon (README, commit mesajları) ile kod arasında küçük ama gerçek kopukluklar var** (yanlış alan adı, "known limitation" olarak gömülen eksik Windows davranışı).

Öneri: Bundan sonraki her "X tamamlandı" iddiası, o X'in coverage/log kanıtıyla birlikte sunulmalı — bugün yapıldığı gibi.

---

## Öncelik Sırası (tüm 12 iddia için)

| Sıra | İddia | Kapsam | Bağımlılık |
|------|-------|--------|------------|
| 1 | #7 — `cortex` cfg uyarısı | Çok düşük (Cargo.toml + 3 satır) | Yok |
| 2 | #5 — copilot-instructions.md kirliliği | Çok düşük (commit veya .gitignore kararı) | Yok |
| 3 | #8 — README `default_action` düzeltmesi | Düşük | Yok |
| 4 | #1 — `bootstrap_session_token` çıkarma + test | Düşük-orta | Yok |
| 5 | #6 — Windows okuma-zamanı ACL doğrulama | Orta | #1 ile aynı dosya ailesinde, birlikte yapılabilir |
| 6 | #2 — HTTP auth middleware entegrasyon testleri | Orta | `open_db`/`PolicyConfig` path injection gerekebilir |
| 7 | #4 — UMAI dispatch döngüsü testleri | Orta | #2'deki path-injection çözümünden faydalanır |
| 8 | #3 — `agent_runner.rs` saf mantık testleri | Orta | Yok |
| 9 | #11 — Şema migration konsolidasyonu | Yüksek (mimari) | Her tabloyu ayrı commit'te taşı |
| 10 | #10 — Ölü kod envanteri | Orta (araştırma) + değişken (temizlik) | Yok |
| 11 | #12 — Doğrulanamayan sayıları düzelt/softlaştır | Düşük | Yok |
| 12 | #9 — Komut envanteri / kapsam ayrımı | Yüksek (mimari karar, kod değişikliği değil) | Diğerlerinden bağımsız, ayrı bir karar süreci gerektirir |

Sıralama mantığı: önce bedavaya yakın, sıfır riskli düzeltmeler (7, 5, 8); sonra bugünkü güvenlik çalışmasının doğrudan devamı olan test boşlukları (1, 6, 2, 4, 3); en sona mimari kararlar (11, 10, 9) — bunlar tek oturumda "bitmez", süreklilik ister.
