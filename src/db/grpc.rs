//! Zebra gRPC indexer client
//!
//! Subscribes to Zebra's ChainTipChange stream for instant new-block notifications.
//! Requires Zebra compiled with `--features indexer` and `indexer_listen_addr` configured.

pub mod proto {
    tonic::include_proto!("zebra.indexer.rpc");
}

use proto::indexer_client::IndexerClient;
use proto::Empty;
use tonic::Streaming;

/// Connect to Zebra's gRPC indexer and return a ChainTipChange stream.
/// The stream yields `BlockHashAndHeight` messages whenever a new block is added.
pub async fn connect_chain_tip_stream(
    url: &str,
) -> Result<Streaming<proto::BlockHashAndHeight>, String> {
    let mut client = IndexerClient::connect(url.to_string())
        .await
        .map_err(|e| format!("gRPC connect failed: {}", e))?;

    let stream = client
        .chain_tip_change(Empty {})
        .await
        .map_err(|e| format!("ChainTipChange subscribe failed: {}", e))?
        .into_inner();

    Ok(stream)
}
