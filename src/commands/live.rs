use crate::config::Config;

/// Run live indexer (with PostgreSQL writes)
pub(crate) async fn run_live(config: &Config) -> Result<(), String> {
    use crate::indexer::Indexer;

    // Check if DATABASE_URL is configured
    if config.database_url.is_empty() {
        return Err(
            "DATABASE_URL not configured. Set it in .env or pass --database-url".to_string(),
        );
    }

    println!("🔗 Connecting to PostgreSQL...");

    let indexer = Indexer::new(config.clone()).await?;

    println!("✅ Connected to PostgreSQL");
    println!();

    indexer.live().await
}
