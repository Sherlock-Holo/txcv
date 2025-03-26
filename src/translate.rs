use std::future::{Future, ready};
use std::io::IsTerminal;
use std::time::Duration;

use async_std::{io, task};
use colored::Colorize;
use crossterm::terminal;
use futures_util::stream::FuturesOrdered;
use futures_util::{AsyncReadExt, TryStreamExt};
use keyring::{Entry, Error};
use requestty::{OnEsc, Question};
use tencentcloud::{Auth, Client};

use crate::api::language_detect::{LanguageDetect, LanguageDetectRequest};
use crate::api::text_translate::{TextTranslate, TextTranslateRequest};
use crate::color::Color;
use crate::lang::Language;
use crate::rate_limit::LeakyBucket;

const SERVICE: &str = "txcv";
const MAX_RESPONSE_SIZE: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub enum Mode {
    Batch(Vec<String>),
    Interact,
    FromStdin,
}

#[derive(Debug, Clone)]
pub struct Translate {
    api_client: Client,
    color: Color,
    concise: bool,
}

impl Translate {
    pub async fn new(from_stdin: bool, color: Color, concise: bool) -> anyhow::Result<Translate> {
        let secret_id = Self::get_secret_id(from_stdin).await?;
        let secret_key = Self::get_secret_key(from_stdin).await?;
        let region = Self::get_region(from_stdin).await?;

        let client = Client::new(region, Auth::new(secret_key, secret_id), MAX_RESPONSE_SIZE);

        Ok(Self {
            api_client: client,
            color,
            concise,
        })
    }

    pub fn clear_authentication() -> anyhow::Result<()> {
        for secret in ["secret_id", "secret_key", "region"] {
            match Entry::new(SERVICE, secret)?.delete_password() {
                Err(Error::NoEntry) | Ok(_) => {}
                Err(err) => return Err(err.into()),
            }
        }

        Ok(())
    }

    pub async fn run(
        &mut self,
        mode: Mode,
        source: Option<Language>,
        target: Option<Language>,
    ) -> anyhow::Result<()> {
        match mode {
            Mode::Batch(words) => self.run_batch(words, source, target).await,
            Mode::Interact => self.run_interact(source, target).await,
            Mode::FromStdin => self.run_from_stdin(source, target).await,
        }
    }

    async fn run_batch(
        &self,
        words: Vec<String>,
        source: Option<Language>,
        target: Option<Language>,
    ) -> anyhow::Result<()> {
        // translate api rate limit is 5/s
        const MAX_CONCURRENT: u32 = 5;
        const REFILL_INTERVAL: Duration = Duration::from_millis(100);

        let bucket = LeakyBucket::builder()
            .max(MAX_CONCURRENT)
            .refill_interval(REFILL_INTERVAL)
            .tokens(MAX_CONCURRENT)
            .build();

        FuturesOrdered::from_iter(
            words
                .into_iter()
                .map(|word| ready(Ok::<_, anyhow::Error>(word))),
        )
        .and_then(|word| async {
            let translated_word = tencentcloud_api_retry(|| async {
                bucket.acquire_one().await;

                let translated_word = self.translate_word(word.clone(), source, target).await?;

                Ok(translated_word)
            })
            .await?;

            Ok((word, translated_word))
        })
        .try_for_each(|(word, translated_word)| {
            self.print(&word, &translated_word);

            ready(Ok(()))
        })
        .await?;

        Ok(())
    }

    async fn run_from_stdin(
        &self,
        source: Option<Language>,
        target: Option<Language>,
    ) -> anyhow::Result<()> {
        let mut buf = String::new();
        io::stdin().read_to_string(&mut buf).await?;

        self.translate_and_print(buf, source, target).await
    }

    async fn run_interact(
        &self,
        source: Option<Language>,
        target: Option<Language>,
    ) -> anyhow::Result<()> {
        loop {
            let word = task::spawn_blocking(|| {
                let question = Question::input("word").on_esc(OnEsc::Terminate).build();
                let answer = requestty::prompt_one(question)?;
                let word = answer.as_string().unwrap_or("");
                if word.is_empty() {
                    Ok::<_, anyhow::Error>(None)
                } else {
                    Ok(Some(word.to_string()))
                }
            })
            .await?;

            match word {
                None => return Ok(()),
                Some(word) => {
                    self.translate_and_print(word, source, target).await?;
                }
            }
        }
    }

    async fn translate_and_print(
        &self,
        word: String,
        source: Option<Language>,
        target: Option<Language>,
    ) -> anyhow::Result<()> {
        let translated_word = self.translate_word(word.clone(), source, target).await?;
        self.print(&word, &translated_word);

        Ok(())
    }

    fn print(&self, word: &str, translated_word: &str) {
        if translated_word.contains('\n') {
            self.print_newline(word, translated_word);

            return;
        } else if let Ok((_, rows)) = terminal::size() {
            let word_count = word.chars().count();
            let translated_word_count = translated_word.chars().count();
            if word_count + translated_word_count > rows as usize {
                self.print_newline(word, translated_word);

                return;
            }
        }

        self.print_one_line(word, translated_word);
    }

    fn print_newline(&self, word: &str, translated_word: &str) {
        let color_output = match self.color {
            Color::Always => true,
            Color::Auto => std::io::stdout().is_terminal(),
            Color::Disable => false,
        };

        if !color_output {
            if !self.concise {
                println!("{word}\n↓\n{translated_word}");
            } else {
                println!("{translated_word}");
            }
        } else if !self.concise {
            println!(
                "{}\n{}\n{}",
                word.blue(),
                "↓".white(),
                translated_word.green()
            );
        } else {
            println!("{}", translated_word.green());
        }
    }

    fn print_one_line(&self, word: &str, translated_word: &str) {
        let color_output = match self.color {
            Color::Always => true,
            Color::Auto => std::io::stdout().is_terminal(),
            Color::Disable => false,
        };

        if !color_output {
            if !self.concise {
                println!("{word} -> {translated_word}");
            } else {
                println!("{translated_word}");
            }
        } else if !self.concise {
            println!(
                "{} {} {}",
                word.blue(),
                "->".white(),
                translated_word.green()
            );
        } else {
            println!("{}", translated_word.green());
        }
    }

    async fn translate_word(
        &self,
        word: String,
        source: Option<Language>,
        target: Option<Language>,
    ) -> Result<String, tencentcloud::Error> {
        let source_lang = match source {
            None => self.get_source_lang(&word).await?,
            Some(source) => source.as_str().to_string(),
        };
        let target_lang = match target {
            None => get_target_lang(&source_lang).unwrap_or("en"),
            Some(target) => target.as_str(),
        };

        Ok(self
            .api_client
            .send::<TextTranslate>(&TextTranslateRequest {
                source_text: word,
                source: source_lang,
                target: target_lang.to_string(),
                project_id: 0,
            })
            .await?
            .0
            .target_text)
    }

    async fn get_source_lang(&self, word: &str) -> Result<String, tencentcloud::Error> {
        match self
            .api_client
            .send::<LanguageDetect>(&LanguageDetectRequest {
                text: word.to_string(),
                project_id: 0,
            })
            .await
        {
            Err(tencentcloud::Error::Api { err, .. })
                if err.code == "FailedOperation.LanguageRecognitionErr" =>
            {
                Ok("zh".to_string())
            }

            Err(err) => Err(err),
            Ok((resp, _)) => Ok(resp.lang),
        }
    }

    async fn get_secret_id(from_stdin: bool) -> anyhow::Result<String> {
        let secret_id_entry = Entry::new(SERVICE, "secret_id")?;
        let secret_id = match secret_id_entry.get_password() {
            Err(Error::NoEntry) => {
                if from_stdin {
                    return Err(anyhow::anyhow!(
                        "read from stdin must set secret_id, secret_key and region at first, please just run txcv to set"
                    ));
                }

                let secret_id = Self::ask_secret_id().await?;
                secret_id_entry.set_password(&secret_id)?;

                secret_id
            }

            Ok(secret_id) if secret_id.is_empty() => {
                let secret_id = Self::ask_secret_id().await?;
                secret_id_entry.set_password(&secret_id)?;

                secret_id
            }

            Err(err) => return Err(err.into()),

            Ok(secret_id) => secret_id,
        };

        Ok(secret_id)
    }

    async fn get_secret_key(from_stdin: bool) -> anyhow::Result<String> {
        let secret_key_entry = Entry::new(SERVICE, "secret_key")?;
        let secret_key = match secret_key_entry.get_password() {
            Err(Error::NoEntry) => {
                if from_stdin {
                    return Err(anyhow::anyhow!(
                        "read from stdin must set secret_id, secret_key and region at first, please just run txcv to set"
                    ));
                }

                let secret_key = Self::ask_secret_key().await?;
                secret_key_entry.set_password(&secret_key)?;

                secret_key
            }

            Ok(secret_key) if secret_key.is_empty() => {
                let secret_key = Self::ask_secret_key().await?;
                secret_key_entry.set_password(&secret_key)?;

                secret_key
            }

            Err(err) => return Err(err.into()),

            Ok(secret_key) => secret_key,
        };

        Ok(secret_key)
    }

    async fn get_region(from_stdin: bool) -> anyhow::Result<String> {
        let region_entry = Entry::new(SERVICE, "region")?;
        let region = match region_entry.get_password() {
            Err(Error::NoEntry) => {
                if from_stdin {
                    return Err(anyhow::anyhow!(
                        "read from stdin must set secret_id, secret_key and region at first, please just run txcv to set"
                    ));
                }

                let region = Self::ask_region().await?;
                region_entry.set_password(&region)?;

                region
            }

            Ok(region) if region.is_empty() => {
                let region = Self::ask_region().await?;
                region_entry.set_password(&region)?;

                region
            }

            Err(err) => return Err(err.into()),

            Ok(secret_key) => secret_key,
        };

        Ok(region)
    }

    async fn ask_secret_id() -> anyhow::Result<String> {
        task::spawn_blocking(|| {
            let question = Question::input("secret_id").message("secret id").build();
            let secret_id = requestty::prompt_one(question)?;
            let secret_id = secret_id
                .as_string()
                .ok_or_else(|| anyhow::anyhow!("secret id is not string"))?;

            if secret_id.is_empty() {
                return Err(anyhow::anyhow!("secret id is empty"));
            }

            Ok(secret_id.to_string())
        })
        .await
    }

    async fn ask_secret_key() -> anyhow::Result<String> {
        task::spawn_blocking(|| {
            let question = Question::password("secret_key")
                .message("secret key")
                .build();
            let secret_key = requestty::prompt_one(question)?;
            let secret_key = secret_key
                .as_string()
                .ok_or_else(|| anyhow::anyhow!("secret_key is not string"))?;

            if secret_key.is_empty() {
                return Err(anyhow::anyhow!("secret_key is empty"));
            }

            Ok(secret_key.to_string())
        })
        .await
    }

    async fn ask_region() -> anyhow::Result<String> {
        task::spawn_blocking(|| {
            let question = Question::input("region").message("region").build();
            let region = requestty::prompt_one(question)?;
            let region = region
                .as_string()
                .ok_or_else(|| anyhow::anyhow!("region is not string"))?;

            if region.is_empty() {
                return Err(anyhow::anyhow!("region is empty"));
            }

            Ok(region.to_string())
        })
        .await
    }
}

async fn tencentcloud_api_retry<
    Fut: Future<Output = Result<T, tencentcloud::Error>>,
    T,
    F: FnMut() -> Fut,
>(
    mut f: F,
) -> Result<T, tencentcloud::Error> {
    const RATE_LIMIT_CODE: &str = "RequestLimitExceeded";

    loop {
        match f().await {
            Err(tencentcloud::Error::Api { err, .. }) if err.code == RATE_LIMIT_CODE => continue,
            Err(err) => return Err(err),
            Ok(result) => return Ok(result),
        }
    }
}

fn get_target_lang(source: &str) -> Option<&'static str> {
    match source {
        "zh" => Some("en"),
        "en" | "jp" => Some("zh"),
        _ => None,
    }
}
