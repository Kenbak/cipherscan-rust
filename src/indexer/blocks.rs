//! Block indexing logic

use crate::db::ZebraState;
use crate::models::Block;

/// Block indexer - reads and parses blocks from Zebra state
pub struct BlockIndexer<'a> {
    zebra: &'a ZebraState,
}

impl<'a> BlockIndexer<'a> {
    pub fn new(zebra: &'a ZebraState) -> Self {
        Self { zebra }
    }

    /// Index a single block by height
    pub fn index_block(&self, height: u32) -> Result<Block, String> {
        let hash = self.zebra.get_block_hash(height)?;

        // TODO: Get header data and parse
        // For now, return minimal block
        let mut hash_reversed = hash;
        hash_reversed.reverse();

        Ok(Block {
            height,
            hash: hex::encode(&hash_reversed),
            version: 4,
            merkle_root: String::new(),
            time: 0,
            difficulty: String::new(),
            nonce: String::new(),
            solution: String::new(),
            previous_block_hash: String::new(),
            tx_count: 0,
            size: 0,
            sapling_tree_size: None,
            orchard_tree_size: None,
        })
    }

    /// Index a range of blocks
    pub fn index_range(&self, start: u32, end: u32) -> Vec<Result<Block, String>> {
        self.zebra.iter_blocks(start, end)
            .map(|result| {
                match result {
                    Ok((height, hash)) => {
                        let mut hash_reversed = hash;
                        hash_reversed.reverse();

                        Ok(Block {
                            height,
                            hash: hex::encode(&hash_reversed),
                            version: 4,
                            merkle_root: String::new(),
                            time: 0,
                            difficulty: String::new(),
                            nonce: String::new(),
                            solution: String::new(),
                            previous_block_hash: String::new(),
                            tx_count: 0,
                            size: 0,
                            sapling_tree_size: None,
                            orchard_tree_size: None,
                        })
                    }
                    Err(e) => Err(e),
                }
            })
            .collect()
    }
}
