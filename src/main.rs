#[async_std::main]
async fn main() -> anyhow::Result<()> {
    txcv::run().await
}
