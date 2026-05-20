use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

// ─── Priority ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum LockPriority {
    Automation = 0,
    Agent = 1,
    User = 2,
}

// ─── Lock key ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LockKey {
    File(PathBuf),
    Task(String),
}

impl std::fmt::Display for LockKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockKey::File(p) => write!(f, "file:{}", p.display()),
            LockKey::Task(id) => write!(f, "task:{id}"),
        }
    }
}

// ─── Lock entry ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct LockEntry {
    owner: String,
    priority: LockPriority,
    acquired_at: Instant,
    timeout: Duration,
}

impl LockEntry {
    fn is_expired(&self) -> bool {
        self.acquired_at.elapsed() > self.timeout
    }
}

// ─── Public status type ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockStatus {
    pub key: LockKey,
    pub owner: String,
    pub priority: LockPriority,
    pub elapsed_secs: u64,
    pub timeout_secs: u64,
}

// ─── Manager ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct LockManager {
    inner: Arc<Mutex<HashMap<LockKey, LockEntry>>>,
}

impl LockManager {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Acquire a lock. Fails if the resource is already locked by a higher or equal
    /// priority owner (unless the existing lock has expired).
    pub async fn acquire(
        &self,
        key: LockKey,
        owner: impl Into<String>,
        priority: LockPriority,
        timeout: Option<Duration>,
    ) -> Result<()> {
        let owner = owner.into();
        let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);
        let mut map = self.inner.lock().await;

        if let Some(existing) = map.get(&key) {
            if !existing.is_expired() {
                if existing.owner == owner {
                    // Re-entrant: same owner refreshes the lock
                    map.insert(
                        key,
                        LockEntry {
                            owner,
                            priority,
                            acquired_at: Instant::now(),
                            timeout,
                        },
                    );
                    return Ok(());
                }
                if existing.priority >= priority {
                    bail!(
                        "Resource '{}' is locked by '{}' (priority {:?}). \
                         Your priority: {:?}.",
                        key,
                        existing.owner,
                        existing.priority,
                        priority
                    );
                }
                // Incoming has higher priority — preempt
                eprintln!(
                    "[LockManager] Lock preempted: '{}' taken from '{}' by '{}' (higher priority)",
                    key, existing.owner, owner
                );
            }
        }

        map.insert(
            key,
            LockEntry {
                owner,
                priority,
                acquired_at: Instant::now(),
                timeout,
            },
        );
        Ok(())
    }

    /// Release a lock. Only the owner (or a User-priority caller) may release.
    pub async fn release(
        &self,
        key: &LockKey,
        caller: &str,
        caller_priority: LockPriority,
    ) -> Result<()> {
        let mut map = self.inner.lock().await;
        match map.get(key) {
            None => bail!("Lock '{}' does not exist", key),
            Some(entry) => {
                if entry.owner != caller && caller_priority < LockPriority::User {
                    bail!(
                        "Cannot release lock '{}': owned by '{}', not '{}'",
                        key,
                        entry.owner,
                        caller
                    );
                }
            }
        }
        map.remove(key);
        Ok(())
    }

    /// Check whether a key is locked (ignoring expired entries).
    pub async fn is_locked(&self, key: &LockKey) -> bool {
        let mut map = self.inner.lock().await;
        if let Some(entry) = map.get(key) {
            if entry.is_expired() {
                map.remove(key);
                return false;
            }
            return true;
        }
        false
    }

    /// List all active (non-expired) locks.
    pub async fn list(&self) -> Vec<LockStatus> {
        let mut map = self.inner.lock().await;
        let expired: Vec<LockKey> = map
            .iter()
            .filter(|(_, e)| e.is_expired())
            .map(|(k, _)| k.clone())
            .collect();
        for k in expired {
            map.remove(&k);
        }
        map.iter()
            .map(|(k, e)| LockStatus {
                key: k.clone(),
                owner: e.owner.clone(),
                priority: e.priority,
                elapsed_secs: e.acquired_at.elapsed().as_secs(),
                timeout_secs: e.timeout.as_secs(),
            })
            .collect()
    }

    /// Force-release all expired locks. Call this from a background sweeper task.
    pub async fn sweep_expired(&self) -> usize {
        let mut map = self.inner.lock().await;
        let before = map.len();
        map.retain(|_, e| !e.is_expired());
        before - map.len()
    }
}

impl Default for LockManager {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Background sweeper ───────────────────────────────────────────────────────

/// Spawn a background task that evicts expired locks every `interval`.
pub fn spawn_sweeper(manager: LockManager, interval: Duration) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            let evicted = manager.sweep_expired().await;
            if evicted > 0 {
                eprintln!("[LockManager] Swept {} expired locks", evicted);
            }
        }
    });
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn acquire_and_release() {
        let mgr = LockManager::new();
        let key = LockKey::File(PathBuf::from("/foo/bar.rs"));
        mgr.acquire(key.clone(), "claude", LockPriority::Agent, None)
            .await
            .unwrap();
        assert!(mgr.is_locked(&key).await);
        mgr.release(&key, "claude", LockPriority::Agent)
            .await
            .unwrap();
        assert!(!mgr.is_locked(&key).await);
    }

    #[tokio::test]
    async fn blocks_equal_priority_second_owner() {
        let mgr = LockManager::new();
        let key = LockKey::File(PathBuf::from("/foo/bar.rs"));
        mgr.acquire(key.clone(), "claude", LockPriority::Agent, None)
            .await
            .unwrap();
        let err = mgr
            .acquire(key.clone(), "gemini", LockPriority::Agent, None)
            .await;
        assert!(err.is_err());
    }

    #[tokio::test]
    async fn user_priority_preempts_agent() {
        let mgr = LockManager::new();
        let key = LockKey::File(PathBuf::from("/foo/bar.rs"));
        mgr.acquire(key.clone(), "gemini", LockPriority::Agent, None)
            .await
            .unwrap();
        mgr.acquire(key.clone(), "user", LockPriority::User, None)
            .await
            .unwrap();
        let locks = mgr.list().await;
        assert_eq!(locks[0].owner, "user");
    }

    #[tokio::test]
    async fn expired_lock_is_not_locked() {
        let mgr = LockManager::new();
        let key = LockKey::Task("task-123".into());
        mgr.acquire(
            key.clone(),
            "claude",
            LockPriority::Agent,
            Some(Duration::from_millis(1)),
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(!mgr.is_locked(&key).await);
    }

    #[tokio::test]
    async fn reentrant_same_owner_refreshes() {
        let mgr = LockManager::new();
        let key = LockKey::File(PathBuf::from("/a.rs"));
        mgr.acquire(key.clone(), "claude", LockPriority::Agent, None)
            .await
            .unwrap();
        mgr.acquire(key.clone(), "claude", LockPriority::Agent, None)
            .await
            .unwrap();
        assert!(mgr.is_locked(&key).await);
    }
}
