use clap::Parser;

use crate::translate::{Mode, Translate};

mod api;
mod translate;

#[derive(Debug, Parser)]
struct Args {
    words: Vec<String>,

    /// clear authentication
    #[arg(short, long)]
    clear: bool,
}

pub async fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.clear {
        Translate::clear_authentication()?;

        return Ok(());
    }

    let mut translate = Translate::new().await?;

    if args.words.is_empty() {
        translate.run(Mode::Interact).await
    } else {
        translate.run(Mode::Batch(args.words)).await
    }
}
