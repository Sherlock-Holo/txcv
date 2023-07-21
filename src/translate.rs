use colored::Colorize;
use keyring::{Entry, Error};
use requestty::Question;
use tencentcloud::{Auth, Client};
use tokio::task;

use crate::api::language_detect::{LanguageDetect, LanguageDetectRequest};
use crate::api::text_translate::{TextTranslate, TextTranslateRequest};

const SERVICE: &str = "txcv";

#[derive(Debug)]
pub enum Mode {
    Batch(Vec<String>),
    Interact,
}

#[derive(Debug, Clone)]
pub struct Translate {
    api_client: Client,
}

impl Translate {
    pub async fn new() -> anyhow::Result<Translate> {
        let secret_id = Self::get_secret_id().await?;
        let secret_key = Self::get_secret_key().await?;
        let region = Self::get_region().await?;

        let client = Client::new(region, Auth::new(secret_key, secret_id));

        Ok(Self { api_client: client })
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

    pub async fn run(&mut self, mode: Mode) -> anyhow::Result<()> {
        match mode {
            Mode::Batch(words) => {
                for word in words {
                    let translated_word = self.translate_word(word.clone()).await?;

                    println!(
                        "{} {} {}",
                        word.blue(),
                        "->".white(),
                        translated_word.green()
                    );
                }
            }

            Mode::Interact => loop {
                let word = task::spawn_blocking(|| {
                    let question = Question::input("word").build();
                    let answer = requestty::prompt_one(question)?;
                    let word = answer.as_string().unwrap_or("");
                    if word.is_empty() {
                        Ok::<_, anyhow::Error>(None)
                    } else {
                        Ok(Some(word.to_string()))
                    }
                })
                .await
                .unwrap()?;

                match word {
                    None => return Ok(()),
                    Some(word) => {
                        let translated_word = self.translate_word(word.clone()).await?;

                        println!(
                            "{} {} {}",
                            word.blue(),
                            "->".white(),
                            translated_word.green()
                        );
                    }
                }
            },
        }

        Ok(())
    }

    async fn translate_word(&mut self, word: String) -> anyhow::Result<String> {
        let source_lang = self.get_source_lang(&word).await?;
        let target_lang = get_target_lang(&source_lang).unwrap_or("en");

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

    async fn get_source_lang(&mut self, word: &str) -> anyhow::Result<String> {
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

            Err(err) => Err(err.into()),
            Ok((resp, _)) => Ok(resp.lang),
        }
    }

    async fn get_secret_id() -> anyhow::Result<String> {
        let secret_id_entry = Entry::new(SERVICE, "secret_id")?;
        let secret_id = match secret_id_entry.get_password() {
            Err(Error::NoEntry) => {
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

    async fn get_secret_key() -> anyhow::Result<String> {
        let secret_key_entry = Entry::new(SERVICE, "secret_key")?;
        let secret_key = match secret_key_entry.get_password() {
            Err(Error::NoEntry) => {
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

    async fn get_region() -> anyhow::Result<String> {
        let region_entry = Entry::new(SERVICE, "region")?;
        let region = match region_entry.get_password() {
            Err(Error::NoEntry) => {
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
        .unwrap()
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
        .unwrap()
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
        .unwrap()
    }
}

fn get_target_lang(source: &str) -> Option<&'static str> {
    match source {
        "zh" => Some("en"),
        "en" | "jp" => Some("zh"),
        _ => None,
    }
}
