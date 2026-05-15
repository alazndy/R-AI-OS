# Faz 3: Instinct Automation — Design Spec
**Date:** 2026-05-15
**Project:** R-AI-OS
**Status:** Approved

---

## Problem

Ajanların projelerden edindikleri tecrübeler (yüksek refactor skoru, güvenlik açıkları, eksik memory.md vb.) kalıcı hale getirilemiyor. Her session'da aynı sorunlar tekrar keşfediliyor.

## Goal

`raios instinct` komutu ile hem manuel hem otomatik kural ekleme; global `~/.agents/instincts.json` + per-project `memory.md ## Instincts` bölümüne kayıt.

---

## Design

### CLI

```
raios instinct add "<kural>"     → JSON + memory.md'ye ekle
raios instinct list              → global + proje instinct'leri listele
raios instinct suggest [proje]   → health analiz → öneriler → interaktif onay
```

`raios health` → mevcut çıktı + footer: `💡 N öneri — run: raios instinct suggest`

### `src/instinct.rs` Değişiklikleri

**Yeni fonksiyonlar:**

```rust
pub fn append_to_memory_md(project_path: &Path, rule: &str) -> anyhow::Result<()>
pub fn suggest_from_health(health: &ProjectHealth) -> Vec<String>
pub fn load_project_rules(project_path: &Path) -> Vec<String>
```

### Pattern → Instinct Kuralları

| Koşul | Önerilen Kural |
|-------|---------------|
| `refactor_grade ∈ ["D","F"]` | `"Refactor grade {grade} — high nesting, clean before new features"` |
| `security_critical > 0` | `"Has {n} CRITICAL security issues — run raios security before commit"` |
| `!has_memory` | `"No memory.md — add one to track decisions"` |
| `!has_sigmap` | `"No SIGMAP.md — run sigmap to generate context map"` |
| `git_dirty == true` | `"Uncommitted changes — commit before context switch"` |
| `constitution_issues.len() > 2` | `"Multiple constitution violations ({n}) — review MASTER.md"` |

### `append_to_memory_md`

`memory.md`'de `## Instincts` bölümü varsa altına ekler (duplicate kontrol). Yoksa dosyanın sonuna `\n## Instincts\n- <rule>` oluşturur. `memory.md` hiç yoksa sadece JSON'a kaydet + uyar.

### Interactive Onay Akışı

```
Suggested instincts:
  [1] Refactor grade D — clean before adding features
  [2] Has 2 CRITICAL security issues — run raios security before commit

Accept? (y=all / 1,2=specific / n=none):
```

### Health Footer

`raios health` çıktısının sonuna (json modda hariç):
```
💡 2 instinct öneri mevcut — run: raios instinct suggest <proje>
```

---

## Error Handling

| Durum | Davranış |
|-------|---------|
| `memory.md` yok | Sadece JSON kaydet + `eprintln!` uyar |
| Proje bulunamıyor | `eprintln!` + exit 1 |
| Boş öneri | `"No suggestions for this project"` |
| Duplicate kural | Sessizce skip |

---

## Tests (3 yeni)

1. `suggest_from_health` F-grade projede en az 1 öneri üretiyor
2. `append_to_memory_md` `## Instincts` bölümü yokken oluşturuyor
3. `append_to_memory_md` mevcut bölüme duplicate eklemiyor

---

## Files Changed

| Dosya | Değişiklik |
|-------|-----------|
| `src/instinct.rs` | `append_to_memory_md` + `suggest_from_health` + `load_project_rules` + tests |
| `src/cli.rs` | `Commands::Instinct` + `cmd_instinct_*` fonksiyonları + health footer |
