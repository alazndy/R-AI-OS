# Hybrid Memory & Context Architecture

R-AI-OS implements a sophisticated multi-layered memory system designed to provide agents with deep project awareness while maintaining extreme token efficiency. This architecture bridges the gap between raw source code and high-level architectural intent.

## 1. The Cortex Engine: Neural Memory Layer

The **Cortex Engine** serves as the semantic backbone of R-AI-OS. It provides local, privacy-preserving vector search capabilities over the entire workspace without relying on external AI APIs.

### Technical Foundation
- **Embedding Model:** Uses `fastembed` (ONNX Runtime) to generate high-dimensional embeddings on-device.
- **Vector Store:** Implements an **HNSW (Hierarchical Navigable Small World)** index for ultra-fast approximate nearest neighbor search.
- **Privacy:** All inference and indexing happen locally; no code or metadata ever leaves the machine.

### The Indexing Pipeline
1.  **Discovery:** The engine identifies indexable files based on extensions (`.rs`, `.md`, `.ts`, etc.) and specific memory patterns (`memory.md`, `AGENTS.md`, `MASTER.md`).
2.  **Chunking:** Files are decomposed into logical segments using a line-aware chunker that preserves context and metadata (file path, start line).
3.  **Semantic Indexing:** Each chunk is transformed into a vector and stored in the HNSW index.
4.  **Incremental Updates:** Cortex tracks file modification times (`mtime`) to ensure only changed files are re-indexed, minimizing CPU overhead.

---

## 2. Hybrid Search: Precision meets Meaning

R-AI-OS employs a **Hybrid Search** strategy to ensure that agents find exactly what they need, whether they are looking for a specific variable name or a broad architectural concept.

### Reciprocal Rank Fusion (RRF)
The system fuses results from two distinct engines:
- **BM25 (Lexical):** Excellent for exact keyword matches, error codes, and specific symbol names.
- **Vector (Semantic):** Captures intent, logic patterns, and conceptual relationships.

The fusion uses the RRF formula:
$$score(d) = \sum_{i \in \{BM25, Vector\}} \frac{1}{k + rank_i(d)}$$
*(where $k=60$ is the rank-tolerance constant)*

### Why Hybrid?
- **Precision:** BM25 prevents "hallucinated" semantic matches when a specific identifier is requested.
- **Recall:** Vector search finds relevant code even when the user's terminology doesn't match the source code exactly.
- **Contextual Snippets:** Hybrid results prioritize snippets that provide the most useful context for LLM consumption.

---

## 3. Context Economics: Sigmap

Context window management is the most critical factor in agent performance and cost. R-AI-OS uses **Sigmap (Signature Mapping)** to achieve a "Skeleton-First" approach to codebase exploration.

### Signature Mapping
Sigmap generates a high-density map of the project (`SIGNATURES.md`) that contains:
- Struct and Enum definitions.
- Function and Method signatures (without bodies).
- Public API contracts and trait implementations.
- Critical TODOs and architectural markers.

### The 97% Token Advantage
By providing agents with a `SIGNATURES.md` file instead of the full source code, R-AI-OS reduces the initial context load by up to **97%**. This allows agents to:
1.  **Orient:** Understand the entire project structure in a single turn.
2.  **Target:** Identify exactly which files need to be read in full.
3.  **Scale:** Work on massive monorepos that would otherwise exceed LLM context limits.

---

## 4. Instinct Engine: Learned Behaviors

The **Instinct Engine** is the "long-term memory" of R-AI-OS, allowing agents to learn from past mistakes and adapt to specific coding styles over time.

### Dual-Layer Storage
- **Global Instincts (`~/.agents/instincts.json`):** Stores universal learnings that apply across all projects (e.g., "Always use OnceLock for regex compilation").
- **Local Memory (`memory.md ## Instincts`):** Stores project-specific constraints and tribal knowledge (e.g., "In this repo, use `pnpm` instead of `npm`").

### Automated Learning Loop
The Instinct Engine doesn't just store rules; it actively suggests them:
- **Health Analysis:** The engine analyzes `ProjectHealth` reports. If a project has a low "Refactor Grade" or security vulnerabilities, it suggests instincts to prevent further technical debt.
- **Style Enforcement:** As agents interact with the codebase, they record "Decision Logs" in `memory.md`. The Instinct Engine parses these to ensure future agents follow the same logic.
- **Prompt Injection:** Learned instincts are automatically injected into the agent's system prompt, ensuring that every subagent operates with the collective intelligence of the entire workspace history.

---

*R-AI-OS: Neural-backed, context-optimized, and instinct-driven development.*
