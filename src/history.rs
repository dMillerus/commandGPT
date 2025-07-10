use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sled::{Db, IVec};
use std::path::Path;

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
        let current_id = self.get_current_id()?;
        let mut entries = Vec::new();

        let start_id = if current_id > count as u64 {
            current_id - count as u64 + 1
        } else {
            1
        };

        for id in start_id..=current_id {
            if let Some(entry) = self.get_entry(id)? {
                entries.push(entry);
            }
        }

        entries.reverse(); // Most recent first
        Ok(entries)
    }

    pub fn search_commands(&self, query: &str) -> Result<Vec<HistoryEntry>> {
        let mut matches = Vec::new();
        let query_lower = query.to_lowercase();

        for item in self.db.iter() {
            let (_, value) = item.context("Failed to read database item")?;
            
            if let Ok(entry) = bincode::deserialize::<HistoryEntry>(&value) {
                if entry.command.to_lowercase().contains(&query_lower) {
                    matches.push(entry);
                }
            }
        }

        matches.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        Ok(matches)
    }

    pub fn clear_all(&self) -> Result<()> {
        self.db.clear()
            .context("Failed to clear database")?;
        
        self.counter.clear()
            .context("Failed to clear counter")?;

        self.db.flush()
            .context("Failed to flush database")?;

        Ok(())
    }

    pub fn delete_entry(&self, id: u64) -> Result<bool> {
        let removed = self.db.remove(id.to_be_bytes())
            .context("Failed to remove entry")?
            .is_some();

        if removed {
            self.db.flush()
                .context("Failed to flush database")?;
        }

        Ok(removed)
    }

    pub fn get_stats(&self) -> Result<HistoryStats> {
        let mut total_commands = 0;
        let mut successful_commands = 0;
        let mut total_duration_ms = 0;
        let mut oldest_timestamp = None;
        let mut newest_timestamp = None;

        for item in self.db.iter() {
            let (_, value) = item.context("Failed to read database item")?;
            
            if let Ok(entry) = bincode::deserialize::<HistoryEntry>(&value) {
                total_commands += 1;
                
                if entry.exit_code == 0 {
                    successful_commands += 1;
                }
                
                total_duration_ms += entry.duration_ms;
                
                if oldest_timestamp.is_none() || entry.timestamp < oldest_timestamp.unwrap() {
                    oldest_timestamp = Some(entry.timestamp);
                }
                
                if newest_timestamp.is_none() || entry.timestamp > newest_timestamp.unwrap() {
                    newest_timestamp = Some(entry.timestamp);
                }
            }
        }

        Ok(HistoryStats {
            total_commands,
            successful_commands,
            success_rate: if total_commands > 0 {
                successful_commands as f64 / total_commands as f64
            } else {
                0.0
            },
            average_duration_ms: if total_commands > 0 {
                total_duration_ms / total_commands
            } else {
                0
            },
            oldest_entry: oldest_timestamp,
            newest_entry: newest_timestamp,
        })
    }

    fn next_id(&self) -> Result<u64> {
        let id = self.counter
            .update_and_fetch(b"current", |old| {
                let current = old
                    .map(|bytes| {
                        let bytes_array: [u8; 8] = bytes.as_ref().try_into().unwrap_or([0u8; 8]);
                        u64::from_be_bytes(bytes_array)
                    })
                    .unwrap_or(0);
                Some(IVec::from((current + 1).to_be_bytes().as_ref()))
            })
            .context("Failed to update counter")?
            .map(|bytes| u64::from_be_bytes(bytes.as_ref().try_into().unwrap_or([0u8; 8])))
            .unwrap_or(1);

        Ok(id)
    }

    fn get_current_id(&self) -> Result<u64> {
        let id = self.counter
            .get(b"current")
            .context("Failed to get current counter")?
            .map(|bytes| {
                let bytes_array: [u8; 8] = bytes.as_ref().try_into().unwrap_or([0u8; 8]);
                u64::from_be_bytes(bytes_array)
            })
            .unwrap_or(0);

        Ok(id)
    }

    fn truncate_output(&self, output: &str, max_len: usize) -> String {
        if output.len() <= max_len {
            output.to_string()
        } else {
            format!("{}... (truncated)", &output[..max_len])
        }
    }
}

#[derive(Debug)]
pub struct HistoryStats {
    pub total_commands: u64,
    pub successful_commands: u64,
    pub success_rate: f64,
    pub average_duration_ms: u64,
    pub oldest_entry: Option<DateTime<Utc>>,
    pub newest_entry: Option<DateTime<Utc>>,
}

// Global functions for easier access
static mut HISTORY_MANAGER: Option<HistoryManager> = None;
static INIT: std::sync::Once = std::sync::Once::new();

fn get_history_manager() -> Result<&'static HistoryManager> {
    unsafe {
        INIT.call_once(|| {
            let home_dir = dirs_next::home_dir().expect("Could not find home directory");
            let db_path = home_dir.join(".commandgpt").join("history.db");
            
            match HistoryManager::new(db_path) {
                Ok(manager) => HISTORY_MANAGER = Some(manager),
                Err(e) => panic!("Failed to initialize history manager: {}", e),
            }
        });

        HISTORY_MANAGER
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("History manager not initialized"))
    }
}

pub async fn record_command(command: &str, stdout: &str, stderr: &str) -> Result<()> {
    let manager = get_history_manager()?;
    let exit_code = if stderr.is_empty() { 0 } else { 1 };
    manager.record_command(command, stdout, stderr, exit_code, 0).await?;
    Ok(())
}

pub async fn get_last_command() -> Result<Option<HistoryEntry>> {
    let manager = get_history_manager()?;
    manager.get_last_entry()
}

pub async fn show_history(count: usize) -> Result<()> {
    use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
    use std::io::Write;

    let manager = get_history_manager()?;
    let entries = manager.get_recent_entries(count)?;

    if entries.is_empty() {
        println!("No command history found.");
        return Ok(());
    }

    let mut stdout = StandardStream::stdout(ColorChoice::Auto);

    for entry in entries {
        // Format timestamp
        let local_time = entry.timestamp.format("%Y-%m-%d %H:%M:%S");
        
        // Color code based on exit status
        let (status_color, status_text) = if entry.exit_code == 0 {
            (Color::Green, "✓")
        } else {
            (Color::Red, "✗")
        };

        // Print entry header
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Cyan)).set_bold(true))?;
        write!(&mut stdout, "[{}] ", entry.id)?;
        
        stdout.set_color(ColorSpec::new().set_fg(Some(status_color)))?;
        write!(&mut stdout, "{} ", status_text)?;
        
        stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow)))?;
        write!(&mut stdout, "{} ", local_time)?;
        
        stdout.reset()?;
        writeln!(&mut stdout, "{}", entry.command)?;

        // Show output if present (truncated)
        if !entry.stdout.is_empty() {
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green)))?;
            let preview = if entry.stdout.len() > 100 {
                format!("{}...", &entry.stdout[..100])
            } else {
                entry.stdout.clone()
            };
            writeln!(&mut stdout, "  → {}", preview.trim())?;
        }

        if !entry.stderr.is_empty() {
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red)))?;
            let preview = if entry.stderr.len() > 100 {
                format!("{}...", &entry.stderr[..100])
            } else {
                entry.stderr.clone()
            };
            writeln!(&mut stdout, "  ✗ {}", preview.trim())?;
        }

        stdout.reset()?;
        writeln!(&mut stdout)?;
    }

    Ok(())
}

pub async fn clear_history() -> Result<()> {
    let manager = get_history_manager()?;
    manager.clear_all()
}

pub async fn search_history(query: &str) -> Result<Vec<HistoryEntry>> {
    let manager = get_history_manager()?;
    manager.search_commands(query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_record_and_retrieve() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let manager = HistoryManager::new(db_path).unwrap();

        let id = manager.record_command("ls -la", "file1\nfile2", "", 0, 100).await.unwrap();
        assert_eq!(id, 1);

        let entry = manager.get_entry(id).unwrap().unwrap();
        assert_eq!(entry.command, "ls -la");
        assert_eq!(entry.stdout, "file1\nfile2");
        assert_eq!(entry.exit_code, 0);
    }

    #[tokio::test]
    async fn test_last_entry() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let manager = HistoryManager::new(db_path).unwrap();

        assert!(manager.get_last_entry().unwrap().is_none());

        manager.record_command("first", "", "", 0, 50).await.unwrap();
        manager.record_command("second", "", "", 0, 75).await.unwrap();

        let last = manager.get_last_entry().unwrap().unwrap();
        assert_eq!(last.command, "second");
        assert_eq!(last.id, 2);
    }

    #[tokio::test]
    async fn test_search() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let manager = HistoryManager::new(db_path).unwrap();

        manager.record_command("ls -la", "", "", 0, 50).await.unwrap();
        manager.record_command("grep pattern file.txt", "", "", 0, 100).await.unwrap();
        manager.record_command("find . -name '*.rs'", "", "", 0, 200).await.unwrap();

        let results = manager.search_commands("ls").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].command, "ls -la");

        let results = manager.search_commands("file").unwrap();
        assert_eq!(results.len(), 2); // grep and find both match
    }
}
