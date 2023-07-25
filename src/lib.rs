use std::io;
use std::io::IsTerminal;

use clap::Parser;

use self::lang::Language;
use self::translate::{Mode, Translate};

mod api;
mod lang;
mod rate_limit;
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

    /// disable color
    #[arg(long)]
    no_color: bool,
}

pub async fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.clear {
        Translate::clear_authentication()?;

        return Ok(());
    }

    let from_stdin = !io::stdin().is_terminal();
    let mut translate = Translate::new(from_stdin, args.no_color).await?;
    if from_stdin {
        return translate
            .run(Mode::FromStdin, args.source, args.target)
            .await;
    }

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
