# Faz 1: Hybrid Memory — Design Spec
**Date:** 2026-05-14  
**Project:** R-AI-OS  
**Status:** Approved

---

## Problem

`raios memory` komutu şu an sadece memory.md dosyalarını listeliyor veya ham içeriği basıyor. 42 proje genelinde geçmiş kararları, instinct'leri ve kuralları hızla bulmak mümkün değil.

## Goal

`raios memory --query "<soru>" --top N` komutuyla tüm projelerin `memory.md`, `AGENTS.md`, `MASTER.md` ve `CLAUDE.md` dosyalarında anlamsal (semantic) arama yapabilmek.

---

## Design

### CLI Değişikliği — `src/cli.rs`

```
Commands::Memory {
    project: Option<String>,
    query:   Option<String>,   // NEW: --query
    top:     usize,            // NEW: --top (default 5)
}
```

Mevcut davranış (query yok → dosya listesi) korunur.

### Cortex Değişiklikleri — `src/cortex/mod.rs`

**Yeni sabit:**
```rust
const MEMORY_PATTERNS: &[&str] = &["memory.md", "AGENTS.md", "MASTER.md", "CLAUDE.md"];
```

**Yeni metot 1 — `index_memory_files(root)`:**
- WalkDir ile tüm workspace'i tarar
- Sadece `MEMORY_PATTERNS`'e uyan dosyaları indeksler
- `rebuild_hnsw()` + `save()` çağırır
- `raios memory --query` çağrıldığında `chunk_count() == 0` ise otomatik tetiklenir

**Yeni metot 2 — `search_with_filter(query, top_k, patterns)`:**
```rust
pub fn search_with_filter(
    &self,
    query: &str,
    top_k: usize,
    filename_patterns: &[&str],
) -> Result<Vec<VectorResult>>
```
- `engine.query(&emb, top_k * 10)` ile geniş havuz çeker
- `result.path` üzerinde `ends_with(pattern)` filtresi uygular
- Tam `top_k` sonuç garantisi

### `VectorResult` — `src/cortex/store.rs`

`score: f32` alanı zaten varsa kullanılır; yoksa eklenir.

### Çıktı Formatı

**Terminal:**
```
[1] 87%  ProjectX / memory.md:42
    "malloc kullanma, ESP32 heap'i kritik aşamada dolabilir"

[2] 74%  LCARSLauncher / CLAUDE.md:15
    "dangerouslySetInnerHTML kullanımı açık gerekçe olmadan yasak"
```

**`--json`:**
```json
[
  {
    "rank": 1,
    "score": 0.87,
    "project": "ProjectX",
    "file": "/path/to/memory.md",
    "line": 42,
    "snippet": "malloc kullanma..."
  }
]
```

---

## Error Handling

| Durum | Davranış |
|-------|---------|
| Cortex init başarısız | `eprintln!` uyarı + mevcut liste fallback |
| 0 sonuç | `"No memory entries found. Try: raios cortex index"` |
| auto-index sırasında hata | Hata loglanır, kısmi indeksle devam edilir |

---

## Tests (3 yeni unit test)

1. `search_with_filter` sadece pattern'e uyan path'leri döndürüyor
2. `top_k` sınırı aşılmıyor
3. `index_memory_files` sonrası `chunk_count() > 0`

---

## Files Changed

| Dosya | Değişiklik |
|-------|-----------|
| `src/cortex/mod.rs` | `search_with_filter()` + `index_memory_files()` |
| `src/cortex/store.rs` | `score` alanı kontrolü |
| `src/cli.rs` | `--query`, `--top` argümanları + `print_memory_results()` |

---

## Out of Scope

- Faz 2 (Guard Watch) ve Faz 3 (Instinct) bu spec'te yok
- Semantik ranking fine-tuning (embedding modeli değişmez)
