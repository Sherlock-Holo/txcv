use std::fmt::{Display, Formatter};

use clap::ValueEnum;

#[derive(Debug, Eq, PartialEq, Copy, Clone, Default, ValueEnum)]
pub enum Color {
    Always,
    #[default]
    Auto,
    Disable,
}

impl Display for Color {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Color::Always => f.write_str("always"),
            Color::Auto => f.write_str("auto"),
            Color::Disable => f.write_str("disable"),
        }
    }
}
