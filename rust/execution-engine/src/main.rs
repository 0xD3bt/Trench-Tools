use execution_engine::extension_api;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = dotenvy::dotenv();
    // Build the router first so we fail fast with a helpful message if
    // the data root is unwritable or the auth tokens file is corrupt.
    // `try_router` returns `Err(String)` instead of panicking in
    // `AppState::new`, which previously aborted the process without a
    // clear way for operators to diagnose.
    let router = match extension_api::try_router() {
        Ok(router) => router,
        Err(error) => {
            eprintln!("execution-engine startup failed: {error}");
            return Err(error.into());
        }
    };
    let bind_address = extension_api::host_bind_address();
    let listener = tokio::net::TcpListener::bind(&bind_address).await?;
    println!("execution-engine listening on http://{bind_address}");
    axum::serve(listener, router).await?;
    Ok(())
}
