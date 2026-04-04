mod prompts;
mod logging;
mod tools;
mod engine;
mod app;
mod constants;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    app::run().await
}
