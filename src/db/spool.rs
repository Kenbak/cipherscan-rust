//! Bounded on-disk spool for recent canonical block bytes.
//!
//! The node remains the source of truth. This spool exists only to preserve
//! recent block bytes across a reorg long enough to attach actual orphan
//! evidence after the node stops serving the old branch.

use std::path::{Path, PathBuf};

pub struct BlockSpool {
    directory: PathBuf,
    retain_blocks: u32,
}

impl BlockSpool {
    pub fn open(directory: PathBuf, retain_blocks: u32) -> Result<Self, String> {
        if retain_blocks == 0 {
            return Err("block spool retention must be non-zero".to_string());
        }
        std::fs::create_dir_all(&directory)
            .map_err(|e| format!("Failed to create block spool at {directory:?}: {e}"))?;
        Ok(Self {
            directory,
            retain_blocks,
        })
    }

    pub async fn store(&self, height: u32, hash: &str, bytes: &[u8]) -> Result<(), String> {
        validate_hash(hash)?;
        let final_path = self.block_path(height, hash);
        let temporary_path = self.directory.join(format!(".{height}-{hash}.tmp"));
        tokio::fs::write(&temporary_path, bytes)
            .await
            .map_err(|e| format!("Failed to write block spool file {temporary_path:?}: {e}"))?;
        tokio::fs::rename(&temporary_path, &final_path)
            .await
            .map_err(|e| format!("Failed to commit block spool file {final_path:?}: {e}"))?;
        self.prune(height).await
    }

    pub async fn read(&self, height: u32, hash: &str) -> Result<Option<Vec<u8>>, String> {
        validate_hash(hash)?;
        let path = self.block_path(height, hash);
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("Failed to read block spool file {path:?}: {error}")),
        }
    }

    async fn prune(&self, current_height: u32) -> Result<(), String> {
        let minimum_height = current_height.saturating_sub(self.retain_blocks.saturating_sub(1));
        let mut entries = tokio::fs::read_dir(&self.directory)
            .await
            .map_err(|e| format!("Failed to list block spool {:?}: {e}", self.directory))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| format!("Failed to iterate block spool {:?}: {e}", self.directory))?
        {
            let file_name = entry.file_name();
            let Some(file_name) = file_name.to_str() else {
                continue;
            };
            let Some(height) = parse_height(file_name) else {
                continue;
            };
            if height < minimum_height {
                tokio::fs::remove_file(entry.path()).await.map_err(|e| {
                    format!("Failed to prune block spool file {:?}: {e}", entry.path())
                })?;
            }
        }
        Ok(())
    }

    fn block_path(&self, height: u32, hash: &str) -> PathBuf {
        self.directory.join(format!("{height:010}-{hash}.block"))
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

fn validate_hash(hash: &str) -> Result<(), String> {
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("block spool hash must be 64 hexadecimal characters".to_string());
    }
    Ok(())
}

fn parse_height(file_name: &str) -> Option<u32> {
    let (height, _) = file_name.split_once('-')?;
    height.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{parse_height, validate_hash};

    #[test]
    fn spool_file_height_is_parsed() {
        assert_eq!(
            parse_height(
                "0000000042-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.block"
            ),
            Some(42)
        );
        assert_eq!(parse_height(".partial.tmp"), None);
    }

    #[test]
    fn spool_hash_rejects_paths_and_wrong_lengths() {
        assert!(validate_hash(&"a".repeat(64)).is_ok());
        assert!(validate_hash("../block").is_err());
        assert!(validate_hash(&"g".repeat(64)).is_err());
    }
}
