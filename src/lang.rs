use clap::ValueEnum;

#[derive(Debug, Copy, Clone, Eq, PartialEq, ValueEnum)]
pub enum Language {
    Chinese,
    English,
    Japanese,
}

impl Language {
    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Chinese => "zh",
            Language::English => "en",
            Language::Japanese => "jp",
        }
    }
}

impl AsRef<str> for Language {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
