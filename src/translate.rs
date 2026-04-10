use std::borrow::Cow;
use std::future::{Future, ready};
use std::io;
use std::io::{IsTerminal, Read, Write};
use std::time::Duration;

use colored::Colorize;
use crossterm::ExecutableCommand;
use crossterm::cursor;
use crossterm::terminal::{self, ClearType};
use futures_util::TryStreamExt;
use futures_util::stream::FuturesOrdered;
use keyring::{Entry, Error};
use rustyline::completion::Completer;
use rustyline::config::Configurer;
use rustyline::error::ReadlineError;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{ColorMode, CompletionType, Helper};
use tencentcloud::{Auth, Client};

use crate::api::language_detect::{LanguageDetect, LanguageDetectRequest};
use crate::api::text_translate::{TextTranslate, TextTranslateRequest};
use crate::color::Color;
use crate::lang::Language;
use crate::rate_limit::LeakyBucket;

const SERVICE: &str = "txcv";
const MAX_RESPONSE_SIZE: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct MaskingHelper {
    masking: bool,
    prompt_style: Option<Cow<'static, str>>,
}

impl Completer for MaskingHelper {
    type Candidate = String;
}

impl Hinter for MaskingHelper {
    type Hint = String;
}

impl Validator for MaskingHelper {}

impl Helper for MaskingHelper {}

impl Highlighter for MaskingHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if self.masking {
            let width = line.chars().count();
            Cow::Owned("*".repeat(width))
        } else {
            Cow::Borrowed(line)
        }
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        match &self.prompt_style {
            Some(prompt_style) => prompt_style.clone(),
            None => Cow::Borrowed(prompt),
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize, kind: CmdKind) -> bool {
        !matches!(kind, CmdKind::MoveCursor) && self.masking
    }
}

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
            match Entry::new(SERVICE, secret)?.delete_credential() {
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

                let translated_word = self.translate_word(&word, source, target).await?;

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
        let buf = async_global_executor::spawn_blocking(|| {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf)?;

            Ok::<_, io::Error>(buf)
        })
        .await?;

        self.translate_and_print(&buf, source, target).await
    }

    async fn run_interact(
        &self,
        source: Option<Language>,
        target: Option<Language>,
    ) -> anyhow::Result<()> {
        let mut editor = rustyline::Editor::<MaskingHelper, DefaultHistory>::new()?;
        let prompt = interact_prompt(self.color_enabled());
        editor.set_helper(Some(MaskingHelper {
            masking: false,
            prompt_style: Some(prompt.1.clone()),
        }));
        if self.color_enabled() {
            editor.set_color_mode(ColorMode::Forced);
        }

        loop {
            let prompt = prompt.clone();
            let (ret_editor, res) = async_global_executor::spawn_blocking(move || {
                let res = editor.readline(&prompt);

                (editor, res)
            })
            .await;
            editor = ret_editor;

            match res {
                Ok(word) => {
                    let word = word.trim();

                    self.render_interact_submission(word)?;

                    if word.is_empty() {
                        continue;
                    }

                    let _ = editor.add_history_entry(word);
                    self.translate_and_print(word, source, target).await?;
                }

                Err(ReadlineError::Interrupted) => {
                    println!();
                    continue;
                }

                Err(ReadlineError::Eof) => return Ok(()),
                Err(err) => return Err(err.into()),
            }
        }
    }

    async fn translate_and_print(
        &self,
        word: &str,
        source: Option<Language>,
        target: Option<Language>,
    ) -> anyhow::Result<()> {
        let translated_word = self.translate_word(word, source, target).await?;
        self.print(word, &translated_word);

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

    fn color_enabled(&self) -> bool {
        match self.color {
            Color::Always => true,
            Color::Auto => io::stdout().is_terminal(),
            Color::Disable => false,
        }
    }

    fn render_interact_submission(&self, word: &str) -> anyhow::Result<()> {
        if !io::stdout().is_terminal() {
            return Ok(());
        }

        let mut stdout = io::stdout();
        stdout.execute(cursor::MoveUp(1))?;
        stdout.execute(terminal::Clear(ClearType::CurrentLine))?;

        if word.is_empty() {
            stdout.flush()?;
            return Ok(());
        }

        writeln!(
            stdout,
            "{}",
            interact_submission(word, self.color_enabled())
        )?;
        stdout.flush()?;

        Ok(())
    }

    fn print_newline(&self, word: &str, translated_word: &str) {
        let color_output = self.color_enabled();

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
        let color_output = self.color_enabled();

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
        word: &str,
        source: Option<Language>,
        target: Option<Language>,
    ) -> Result<String, tencentcloud::Error> {
        let source_lang = match source {
            None => self.get_source_lang(word).await?,
            Some(source) => Cow::Borrowed(source.as_str()),
        };
        let target_lang = match target {
            None => get_target_lang(&source_lang).unwrap_or("en"),
            Some(target) => target.as_str(),
        };

        Ok(self
            .api_client
            .send::<TextTranslate>(&TextTranslateRequest {
                source_text: word,
                source: source_lang.as_ref(),
                target: target_lang,
                project_id: 0,
            })
            .await?
            .0
            .target_text)
    }

    async fn get_source_lang(&self, word: &str) -> Result<Cow<'_, str>, tencentcloud::Error> {
        match self
            .api_client
            .send::<LanguageDetect>(&LanguageDetectRequest {
                text: word,
                project_id: 0,
            })
            .await
        {
            Err(tencentcloud::Error::Api { err, .. })
                if err.code == "FailedOperation.LanguageRecognitionErr" =>
            {
                Ok(Cow::Borrowed("zh"))
            }

            Err(err) => Err(err),

            Ok((resp, _)) => Ok(Cow::Owned(resp.lang)),
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
        async_global_executor::spawn_blocking(|| read_input("secret id: ", false)).await
    }

    async fn ask_secret_key() -> anyhow::Result<String> {
        async_global_executor::spawn_blocking(|| read_input("secret key: ", true)).await
    }

    async fn ask_region() -> anyhow::Result<String> {
        async_global_executor::spawn_blocking(|| read_input("region: ", false)).await
    }
}

fn read_input(prompt: &str, masked: bool) -> anyhow::Result<String> {
    let mut editor = rustyline::Editor::<MaskingHelper, DefaultHistory>::new()?;
    editor.set_completion_type(CompletionType::List);
    editor.set_auto_add_history(false);

    if masked {
        editor.set_helper(Some(MaskingHelper {
            masking: true,
            prompt_style: None,
        }));
        editor.set_color_mode(ColorMode::Forced);
    }

    match editor.readline(prompt) {
        Ok(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                Err(anyhow::anyhow!("input is empty"))
            } else {
                Ok(value)
            }
        }

        Err(ReadlineError::Interrupted | ReadlineError::Eof) => {
            Err(anyhow::anyhow!("input cancelled"))
        }

        Err(err) => Err(err.into()),
    }
}

fn interact_prompt(color_output: bool) -> (String, Cow<'static, str>) {
    let raw = format!("? word: {} ", '›');
    if !color_output {
        return (raw.clone(), Cow::Owned(raw));
    }

    let styled = "\x1b[1;32m?\x1b[0m word: \x1b[34m›\x1b[0m ";

    (raw, Cow::Borrowed(styled))
}

fn interact_submission(word: &str, color_output: bool) -> String {
    if !color_output {
        return format!("✔ word: · {word}");
    }

    format!("{} {} · {word}", "✔".green(), "word:".green().bold())
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
