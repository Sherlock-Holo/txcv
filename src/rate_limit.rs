// copy from leaky-bucket-lite, but use async_std to replace tokio

use std::sync::Arc;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use async_std::task;
use futures_util::lock::Mutex;

#[derive(Debug)]
struct LeakyBucketInner {
    /// How many tokens this bucket can hold.
    max: u32,
    /// Interval at which the bucket gains tokens.
    refill_interval: Duration,
    /// Amount of tokens gained per interval.
    refill_amount: u32,

    /// Current tokens in the bucket.
    tokens: RwLock<u32>,
    /// Last refill of the tokens.
    last_refill: RwLock<Instant>,

    /// To prevent more than one task from acquiring at the same time,
    /// a Semaphore is needed to guard access.
    lock: Mutex<()>,
}

impl LeakyBucketInner {
    fn new(max: u32, tokens: u32, refill_interval: Duration, refill_amount: u32) -> Self {
        Self {
            tokens: RwLock::new(tokens),
            max,
            refill_interval,
            refill_amount,
            last_refill: RwLock::new(Instant::now()),
            lock: Default::default(),
        }
    }

    /// Updates the tokens in the leaky bucket and returns the current amount
    /// of tokens in the bucket.
    #[inline]
    fn update_tokens(&self) -> u32 {
        let mut last_refill = self.last_refill.write().unwrap();
        let mut tokens = self.tokens.write().unwrap();
        let time_passed = Instant::now() - *last_refill;

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let refills_since =
            (time_passed.as_secs_f64() / self.refill_interval.as_secs_f64()).floor() as u32;

        *tokens += self.refill_amount * refills_since;
        *last_refill += self.refill_interval * refills_since;

        *tokens = tokens.min(self.max);

        *tokens
    }

    async fn acquire(&self, amount: u32) {
        // Make sure this is the only task accessing the tokens in a real
        // "write" rather than "update" way.
        let _permit = self.lock.lock().await;
        // let _permit = self.semaphore.acquire().await;

        let current_tokens = self.update_tokens();

        if current_tokens < amount {
            let tokens_needed = amount - current_tokens;
            let mut refills_needed = tokens_needed / self.refill_amount;
            let refills_needed_remainder = tokens_needed % self.refill_amount;

            if refills_needed_remainder > 0 {
                refills_needed += 1;
            }

            let target_time = {
                let last_refill = self.last_refill.read().unwrap();
                *last_refill + self.refill_interval * refills_needed
            };
            let sleep_duration = target_time.duration_since(Instant::now());

            task::sleep(sleep_duration).await;

            self.update_tokens();
        }

        *self.tokens.write().unwrap() -= amount;
    }
}

/// The leaky bucket.
#[derive(Clone, Debug)]
pub struct LeakyBucket {
    inner: Arc<LeakyBucketInner>,
}

impl LeakyBucket {
    fn new(max: u32, tokens: u32, refill_interval: Duration, refill_amount: u32) -> Self {
        let inner = Arc::new(LeakyBucketInner::new(
            max,
            tokens,
            refill_interval,
            refill_amount,
        ));

        Self { inner }
    }

    /// Construct a new leaky bucket through a builder.
    #[must_use]
    pub const fn builder() -> Builder {
        Builder::new()
    }

    /// Get the max number of tokens this rate limiter is configured for.
    #[must_use]
    pub fn max(&self) -> u32 {
        self.inner.max
    }

    #[inline]
    pub async fn acquire_one(&self) {
        self.acquire(1).await;
    }

    pub async fn acquire(&self, amount: u32) {
        assert!(
            amount <= self.max(),
            "Acquiring more tokens than the configured maximum is not possible"
        );

        self.inner.acquire(amount).await;
    }
}

/// Builder for a leaky bucket.
#[derive(Debug)]
pub struct Builder {
    max: Option<u32>,
    tokens: Option<u32>,
    refill_interval: Option<Duration>,
    refill_amount: Option<u32>,
}

impl Builder {
    /// Create a new builder with all defaults.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max: None,
            tokens: None,
            refill_interval: None,
            refill_amount: None,
        }
    }

    /// Set the max value for the builder.
    #[must_use]
    pub const fn max(mut self, max: u32) -> Self {
        self.max = Some(max);
        self
    }

    /// The number of tokens that the bucket should start with.
    ///
    /// If set to larger than `max` at build time, will only saturate to max.
    #[must_use]
    pub const fn tokens(mut self, tokens: u32) -> Self {
        self.tokens = Some(tokens);
        self
    }

    /// Set the max value for the builder.
    #[must_use]
    pub const fn refill_interval(mut self, refill_interval: Duration) -> Self {
        self.refill_interval = Some(refill_interval);
        self
    }

    /// Construct a new leaky bucket.
    #[must_use]
    pub fn build(self) -> LeakyBucket {
        const DEFAULT_MAX: u32 = 120;
        const DEFAULT_TOKENS: u32 = 0;
        const DEFAULT_REFILL_INTERVAL: Duration = Duration::from_secs(1);
        const DEFAULT_REFILL_AMOUNT: u32 = 1;

        let max = self.max.unwrap_or(DEFAULT_MAX);
        let tokens = self.tokens.unwrap_or(DEFAULT_TOKENS);
        let refill_interval = self.refill_interval.unwrap_or(DEFAULT_REFILL_INTERVAL);
        let refill_amount = self.refill_amount.unwrap_or(DEFAULT_REFILL_AMOUNT);

        LeakyBucket::new(max, tokens, refill_interval, refill_amount)
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self::new()
    }
}
