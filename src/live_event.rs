use {
  serde::Serialize,
  std::{
    sync::{
      LazyLock, Mutex,
      atomic::{AtomicBool, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
  },
  tokio::sync::broadcast,
};

static LIVE_ENABLED: AtomicBool = AtomicBool::new(false);
static LIVE_SENDER: LazyLock<Mutex<Option<broadcast::Sender<String>>>> =
  LazyLock::new(|| Mutex::new(None));
static RUN_START: LazyLock<Mutex<Option<Instant>>> = LazyLock::new(|| Mutex::new(None));

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum LiveEvent {
  RecipeCompleted {
    name: String,
    success: bool,
    error_message: Option<String>,
    duration_ms: u64,
    timestamp_ms: u64,
  },
  RecipeLine {
    recipe: String,
    command: String,
    timestamp_ms: u64,
  },
  RecipeLineCompleted {
    recipe: String,
    command: String,
    success: bool,
    code: Option<i32>,
    duration_ms: u64,
    timestamp_ms: u64,
  },
  RecipeStarted {
    name: String,
    doc: Option<String>,
    is_dependency: bool,
    timestamp_ms: u64,
  },
  RunCompleted {
    success: bool,
    duration_ms: u64,
    timestamp_ms: u64,
  },
  RunStarted {
    timestamp_ms: u64,
  },
}

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn now_ms() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis() as u64
}

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn run_elapsed_ms() -> u64 {
  RUN_START
    .lock()
    .unwrap()
    .map(|start| start.elapsed().as_millis() as u64)
    .unwrap_or(0)
}

pub(crate) fn set_sender(tx: broadcast::Sender<String>) {
  LIVE_ENABLED.store(true, Ordering::Release);
  *LIVE_SENDER.lock().unwrap() = Some(tx);
  *RUN_START.lock().unwrap() = Some(Instant::now());
}

pub(crate) fn emit(event: LiveEvent) {
  if !LIVE_ENABLED.load(Ordering::Relaxed) {
    return;
  }

  if let Ok(json) = serde_json::to_string(&event) {
    if let Ok(guard) = LIVE_SENDER.lock() {
      if let Some(tx) = guard.as_ref() {
        let _ = tx.send(json);
      }
    }
  }
}
