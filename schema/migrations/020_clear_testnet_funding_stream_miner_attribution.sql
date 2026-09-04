-- The ZIP-214 Testnet FPF/ZCG payout is a funding-stream output, not a miner
-- address. Older indexer versions treated the first transparent coinbase
-- output as the miner, even where the miner reward was shielded.
UPDATE blocks
SET miner_address = NULL
WHERE miner_address = 't2HifwjUj9uyxr9bknR8LFuQbc98c3vkXtu';

UPDATE orphaned_blocks
SET miner_address = NULL
WHERE miner_address = 't2HifwjUj9uyxr9bknR8LFuQbc98c3vkXtu';
