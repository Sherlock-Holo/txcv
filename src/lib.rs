use std::io;
use std::io::IsTerminal;

use clap::builder::styling::AnsiColor;
use clap::builder::Styles;
use clap::Parser;

use self::color::Color;
use self::lang::Language;
use self::translate::{Mode, Translate};

mod api;
mod color;
mod lang;
mod rate_limit;
mod translate;

#[derive(Debug, Parser)]
#[command(version, about,
styles = Styles::styled()
.header(AnsiColor::Yellow.on_default())
.usage(AnsiColor::Green.on_default())
.literal(AnsiColor::Green.on_default())
.placeholder(AnsiColor::Green.on_default()))]
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

    /// translate output color
    #[arg(long, default_value_t)]
    color: Color,

    /// if specifies, only print the translated result
    #[arg(long)]
    concise: bool,
}

pub async fn run() -> anyhow::Result<()> {
    let args = Args::parse();
    if args.clear {
        Translate::clear_authentication()?;

        return Ok(());
    }

    let from_stdin = !io::stdin().is_terminal();
    let mut translate = Translate::new(from_stdin, args.color, args.concise).await?;
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
