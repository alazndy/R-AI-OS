# R-AI-OS Architecture Deep Dive

This document provides a technical overview of the R-AI-OS architecture, focusing on the daemon-centric design, client-server communication, and security mechanisms.

## 1. Daemon-Centric Design (`aiosd`)

The core of R-AI-OS is the **aiosd** (AI Operating System Daemon). It runs as a background process and serves as the central intelligence and state coordinator for the entire system.

### Key Responsibilities:
- **Neural Indexing:** Maintains a hybrid search index combining BM25 (lexical) and Vector (semantic) search capabilities via the `Cortex` module.
- **Entity Discovery:** Automatically scans the workspace to identify projects, tools, and documentation.
- **Background Workers:**
    - **Health Worker:** Periodically scans projects for compliance and structural integrity.
    - **Git Worker:** Monitors repository states and handles synchronization.
    - **Sentinel Worker:** Watches for critical file changes and enforces "Sentinel Guard" policies.
    - **Cortex Worker:** Manages embedding generation and vector store updates.
- **State Management:** Holds the `DaemonState`, which is a thread-safe, shared repository of all active agents, health reports, and pending approvals.

## 2. The Client (`raios`)

The **raios** binary is the primary interface for users. It supports both a rich Terminal User Interface (TUI) and a standard Command Line Interface (CLI).

### TUI Architecture:
- **Ratatui Integration:** Uses the `ratatui` crate for high-performance, immediate-mode terminal rendering.
- **Asynchronous Core:** The TUI runs on a main loop that polls for user input while simultaneously listening for background messages (`BgMsg`) from the IPC thread.
- **Component-Based UI:** The interface is divided into specialized panels (Dashboard, Search, Sentinel, MemPalace) that react to state changes.

### CLI Mode:
- For quick commands (e.g., `raios search "query"`), the client bypasses the TUI and communicates directly with the daemon to fetch and display results.

## 3. IPC Protocol (Aura Hardened)

Communication between `raios` and `aiosd` is handled via a custom IPC protocol named **Aura**, designed for low latency and high security.

### Connection Mechanism:
- **Transport:** TCP over `127.0.0.1:42069`.
- **Handshake (Aura Hardened):**
    1. Upon startup, `aiosd` generates a unique **UUID v4 token**.
    2. This token is stored securely in `~/.config/raios/.ipc_token` (or the platform equivalent).
    3. When `raios` connects, it must immediately send an `AUTH <token>` command.
    4. `aiosd` drops any connection that fails this handshake within the first message.

### Messaging Format:
- All messages are newline-delimited JSON objects.
- **Commands (Client -> Daemon):** `{"command": "Search", "query": "..."}`
- **Events (Daemon -> Client):** `{"event": "SearchResults", "results": [...]}`

## 4. Data Flow: Task Execution

The following sequence illustrates how a search task is processed:

1. **Initiation:** The user types a query in the `raios` Search panel.
2. **Request:** `raios` sends a `VectorSearch` command over the TCP socket.
3. **Processing:**
    - `aiosd` receives the command and triggers the `Cortex` module.
    - It performs a semantic search in the vector database and a lexical search in the BM25 index.
    - The results are fused using **Reciprocal Rank Fusion (RRF)**.
4. **Response:** `aiosd` sends a `VectorResults` event back to the client.
5. **Update:** The `raios` IPC thread receives the JSON, parses it into a `BgMsg::SearchResults`, and sends it to the main UI thread via an mpsc channel.
6. **Rendering:** The TUI detects the new results in the application state and re-renders the Search panel.

---

*R-AI-OS: Empowering developers with a hardened, daemon-backed AI workspace.*
