use std::{env, fs};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let path = env::var("WHATSAPP_ARCHIVE_DB").map_err(|_| "WHATSAPP_ARCHIVE_DB is required")?;
    let token_file = env::var("WHATSAPP_ARCHIVE_TOKEN_FILE")
        .map_err(|_| "WHATSAPP_ARCHIVE_TOKEN_FILE is required")?;
    let token = fs::read_to_string(token_file).map_err(|_| "cannot read archive token file")?;
    let app = ferret_whatsapp_archive::router(path, &token).await?;
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
