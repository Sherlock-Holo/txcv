use clap::Parser;

use self::lang::Language;
use self::translate::{Mode, Translate};

mod api;
mod lang;
mod translate;

#[derive(Debug, Parser)]
struct Args {
    words: Vec<String>,

    /// clear authentication
    #[arg(short, long)]
    clear: bool,

    /// source language, default is auto detect
    #[arg(short, long)]
    source: Option<Language>,

    /// target language, default is auto detect
    #[arg(short, long)]
    target: Option<Language>,
}

pub async fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.clear {
        Translate::clear_authentication()?;

        return Ok(());
    }

    let mut translate = Translate::new().await?;

    if args.words.is_empty() {
        translate
            .run(Mode::Interact, args.source, args.target)
            .await
    } else {
        translate
            .run(Mode::Batch(args.words), args.source, args.target)
            .await
    }
}
