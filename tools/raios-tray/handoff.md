# raios-tray Codex Handoff Prompt

## Kimlik
Sen Codex Kaira'sın. Chat Türkçe, kod ve teknik içerik İngilizce.

## Proje
- **Canonical source of truth:** `/home/alaz/dev/core/R-AI-OS/tools/raios-tray/`
- **Compatibility launcher:** `/home/alaz/dev/tools/raios-tray/raios-tray.py`
- **Canonical main file:** `/home/alaz/dev/core/R-AI-OS/tools/raios-tray/raios-tray.py`
- **Active venv:** `/home/alaz/dev/tools/raios-tray/.venv/`

## Son Mimari Karar
`raios-tray` için tek doğru kaynak artık: `/home/alaz/dev/core/R-AI-OS/tools/raios-tray/`

`/home/alaz/dev/tools/raios-tray/` artık gerçek app kopyası değil; launcher/wrapper.
Servis dosyaları da canonical path'e yönlendirildi.

## Mevcut Durum
Önceden iki farklı UI vardı:
1. Tray popup içindeki tek sütun proje submenu'leri
2. Ayrı açılan `ProjectManagerDialog`

Bu karışıklık giderildi.
Artık tray popup proje submenu üretmiyor.
Onun yerine proje erişimi `ProjectManagerDialog` üzerinden akıyor.

## Şu An Beklenen UI
**Tray popup içinde:**
- `Projects: N`
- `Pinned: N`
- `Open Project Manager...`

Bu satırlar Project Manager'a götürmeli.

**Project Manager içinde:**
- 2 sütunlu kart grid
- `VSCode` butonu
- `Agent` dropdown
- pin/edit/remove aksiyonları

## Yakın Zamandaki Önemli Commitler
**Canonical repo:** `/home/alaz/dev/core/R-AI-OS`
- `b1d2c0e` `refactor: make raios-tray canonical`
- `3fc75f7` `ui: route tray projects to manager`
- `a00010a` `fix: make project summaries clickable`

**Launcher repo:** `/home/alaz/dev/tools/raios-tray`
- `d81ad90` `refactor: delegate to canonical tray source`

## Kritik Tespit
Kod tarafında canonical dosyada şu özellikler kesin mevcut:
- `QGridLayout`
- `ProjectManagerDialog`
- `VSCode` button
- `Agent` menu
- `show_projects_dialog()` → `open_manage_projects()`

Yani kullanıcı eski UI görüyorsa sebep çoğunlukla eski tray process'inin açık kalması veya restart edilmemesi.

## Servis
**User service:** `/home/alaz/.config/systemd/user/raios-tray.service`

Şu an canonical path'e bakıyor:
- `WorkingDirectory=/home/alaz/dev/core/R-AI-OS/tools/raios-tray`
- `ExecStart=/home/alaz/dev/tools/raios-tray/.venv/bin/python /home/alaz/dev/core/R-AI-OS/tools/raios-tray/raios-tray.py`

## Manuel Çalıştırma
**Canonical path'ten test:**
```bash
pkill -f raios-tray.py
cd /home/alaz/dev/core/R-AI-OS/tools/raios-tray
QT_QPA_PLATFORM=xcb /home/alaz/dev/tools/raios-tray/.venv/bin/python ./raios-tray.py
```

**Servis restart:**
```bash
systemctl --user daemon-reload
systemctl --user restart raios-tray
```

## Kullanıcıdan Gelen Son Geri Bildirim
- Daha önce tray popup'ta tek sütun ve eski proje görünümü görüyordu
- VSCode ve Agent görünmüyordu
- Son fix sonrası Projects ve Pinned satırları gri kaldı; bu da düzeltildi ve tıklanabilir yapıldı

## Bir Sonraki Agent İçin Hedef
1. **Kullanıcıyla canlı teyit et:**
   - Tray popup artık proje submenu göstermiyor mu?
   - Projects veya Pinned tıklanınca 2 sütunlu Project Manager açılıyor mu?
   - VSCode ve Agent görünüyor mu?

2. **Hâlâ eski UI görünüyorsa çalışan process path'ini doğrula:**
   ```bash
   ps -ef | rg 'raios-tray.py'
   systemctl --user cat raios-tray
   ```

3. **Gerekirse service yerine kullanıcı terminalinden canonical dosyayı doğrudan başlatıp davranışı karşılaştır.**

4. **Sorun devam ederse tray popup ile dialog açılışı arasındaki event path'i runtime log ile izle.**

## Not
`dev/tools/raios-tray` içindeki kodu bundan sonra gerçek uygulama gibi geliştirme.
Tüm gerçek değişiklikleri sadece canonical path altında yap:
`/home/alaz/dev/core/R-AI-OS/tools/raios-tray/`