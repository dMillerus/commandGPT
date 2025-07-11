use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::{Db, IVec};
use std::path::Path;
use crate::error::CommandGPTError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: u64,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timestamp: DateTime<Utc>,
    pub duration_ms: u64,
}

pub struct HistoryManager {
    db: Db,
    counter: sled::Tree,
}

impl HistoryManager {
    pub fn new<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let db = sled::open(db_path)
            .context("Failed to open history database")?;
        
        let counter = db.open_tree("counter")
            .context("Failed to open counter tree")?;

        Ok(Self { db, counter })
    }

    pub async fn record_command(
        &self,
        command: &str,
        stdout: &str,
        stderr: &str,
        exit_code: i32,
        duration_ms: u64,
    ) -> Result<u64> {
        let id = self.next_id()?;
        
        let entry = HistoryEntry {
            id,
            command: command.to_string(),
            stdout: self.truncate_output(stdout, 1024),
            stderr: self.truncate_output(stderr, 1024),
            exit_code,
            timestamp: Utc::now(),
            duration_ms,
        };

        let serialized = bincode::serialize(&entry)
            .context("Failed to serialize history entry")?;

        self.db.insert(id.to_be_bytes(), serialized)
            .context("Failed to insert history entry")?;

        self.db.flush()
            .context("Failed to flush database")?;

        log::debug!("Recorded command {} in history", id);
        Ok(id)
    }

    pub fn get_last_entry(&self) -> Result<Option<HistoryEntry>> {
        let current_id = self.get_current_id()?;
        
        if current_id == 0 {
            return Ok(None);
        }

        self.get_entry(current_id)
    }

    pub fn get_entry(&self, id: u64) -> Result<Option<HistoryEntry>> {
        if let Some(data) = self.db.get(id.to_be_bytes())
            .context("Failed to get history entry")? {
            
            let entry: HistoryEntry = bincode::deserialize(&data)
                .context("Failed to deserialize history entry")?;
            
            Ok(Some(entry))
        } else {
            Ok(None)
        }
    }

    pub fn get_recent_entries(&self, count: usize) -> Result<Vec<HistoryEntry>> {
        let mut entries = Vec::new();
        
        for item in self.db.iter().rev() {
            let (_, value) = item.map_err(|e| CommandGPTError::HistoryError {
                message: "Failed to read database item".to_string(),
                source: Some(Box::new(e)),
            })?;
            
            if let Ok(entry) = bincode::deserialize::<HistoryEntry>(&value) {
                entries.push(entry);
                if entries.len() >= count {
                    break;
                }
            }
        }
        
        Ok(entries)
    }

    pub async fn clear(&self) -> Result<()> {
        self.db.clear()
            .map_err(|e| CommandGPTError::HistoryError {
                message: "Failed to clear database".to_string(),
                source: Some(Box::new(e)),
            })?;
        
        self.counter.clear()
            .map_err(|e| CommandGPTError::HistoryError {
                message: "Failed to clear counter".to_string(),
                source: Some(Box::new(e)),
            })?;
        
        self.db.flush()
            .map_err(|e| CommandGPTError::HistoryError {
                message: "Failed to flush database after clear".to_string(),
                source: Some(Box::new(e)),
            })?;

        Ok(())
    }

    pub async fn remove_entry(&self, id: u64) -> Result<bool> {
        let removed = self.db.remove(id.to_be_bytes())
            .map_err(|e| CommandGPTError::HistoryError {
                message: "Failed to remove entry".to_string(),
                source: Some(Box::new(e)),
            })?
            .is_some();

        if removed {
            self.db.flush()
                .map_err(|e| CommandGPTError::HistoryError {
                    message: "Failed to flush database after removal".to_string(),
                    source: Some(Box::new(e)),
                })?;
        }

        Ok(removed)
    }

    pub fn search_history(&self, query: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
        let mut results = Vec::new();
        let query_lower = query.to_lowercase();
        
        for item in self.db.iter().rev() {
            let (_, value) = item.map_err(|e| CommandGPTError::HistoryError {
                message: "Failed to read database item".to_string(),
                source: Some(Box::new(e)),
            })?;
            
            if let Ok(entry) = bincode::deserialize::<HistoryEntry>(&value) {
                if entry.command.to_lowercase().contains(&query_lower) {
                    results.push(entry);
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        
        Ok(results)
    }

    fn truncate_output(&self, output: &str, max_len: usize) -> String {
        if output.len() <= max_len {
            output.to_string()
        } else {
            format!("{}... [truncated: {} bytes total]", 
                   &output[..max_len], output.len())
        }
    }

    fn next_id(&self) -> Result<u64> {
        let id = self.counter
            .update_and_fetch(b"current", |old| {
                let current = old
                    .map(|bytes| {
                        let bytes_array: [u8; 8] = bytes.try_into().unwrap_or([0u8; 8]);
                        u64::from_be_bytes(bytes_array)
                    })
                    .unwrap_or(0);
                
                Some((current + 1).to_be_bytes().to_vec())
            })
            .map_err(|e| CommandGPTError::HistoryError {
                message: "Failed to update counter".to_string(),
                source: Some(Box::new(e)),
            })?
            .map(|bytes| {
                let bytes_array: [u8; 8] = bytes.as_ref().try_into().unwrap_or([0u8; 8]);
                u64::from_be_bytes(bytes_array)
            })
            .unwrap_or(1);

        Ok(id)
    }

    fn get_current_id(&self) -> Result<u64> {
        let id = self.counter
            .get(b"current")
            .map_err(|e| CommandGPTError::HistoryError {
                message: "Failed to get current counter".to_string(),
                source: Some(Box::new(e)),
            })?
            .map(|bytes| {
                let bytes_array: [u8; 8] = bytes.as_ref().try_into().unwrap_or([0u8; 8]);
                u64::from_be_bytes(bytes_array)
            })
            .unwrap_or(0);

        Ok(id)
    }
}

// Global history manager instance
static HISTORY_MANAGER: std::sync::OnceLock<Option<HistoryManager>> = std::sync::OnceLock::new();

pub async fn init_history(db_path: &Path) -> Result<()> {
    let manager = HistoryManager::new(db_path)?;
    HISTORY_MANAGER.set(Some(manager)).map_err(|_| CommandGPTError::SystemError {
        message: "History manager already initialized".to_string(),
        source: None,
    })?;
    Ok(())
}

fn get_history_manager() -> crate::error::Result<&'static HistoryManager> {
    HISTORY_MANAGER
        .get()
        .and_then(|opt| opt.as_ref())
        .ok_or_else(|| CommandGPTError::SystemError {
            message: "History manager not initialized".to_string(),
            source: None,
        })
}

pub async fn record_command(command: &str, stdout: &str, stderr: &str) -> Result<()> {
    let manager = get_history_manager()?;
    
    // Determine exit code from stderr content
    let exit_code = if stderr.is_empty() { 0 } else { 1 };
    
    manager.record_command(command, stdout, stderr, exit_code, 0).await?;
    Ok(())
}

pub async fn get_last_command() -> crate::error::Result<Option<HistoryEntry>> {
    let manager = get_history_manager()?;
    manager.get_last_entry().map_err(|e| CommandGPTError::HistoryError {
        message: format!("Failed to get last command: {}", e),
        source: None,
    })
}

pub async fn show_history(count: usize) -> Result<()> {
    let manager = get_history_manager()?;
    let entries = manager.get_recent_entries(count)?;
    
    if entries.is_empty() {
        println!("No command history found.");
        return Ok(());
    }
    
    println!("📜 Recent Commands:");
    for entry in entries {
        let status_icon = if entry.exit_code == 0 { "✅" } else { "❌" };
        println!("  {} [{}] {} - {}", 
                status_icon,
                entry.timestamp.format("%m-%d %H:%M"),
                entry.id,
                entry.command);
    }
    
    Ok(())
}

pub async fn clear_history() -> Result<()> {
    let manager = get_history_manager()?;
    manager.clear().await
}

pub async fn search_history(query: &str, limit: Option<usize>) -> Result<Vec<HistoryEntry>> {
    let manager = get_history_manager()?;
    manager.search_history(query, limit.unwrap_or(10))
}
