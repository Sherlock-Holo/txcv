fn main() -> anyhow::Result<()> {
    async_global_executor::block_on(txcv::run())
}
