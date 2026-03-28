use {
  serde::Serialize,
  std::{
    process::Command,
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
static COMMAND_LINE: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RecipeNode {
  pub(crate) dependencies: Vec<String>,
  pub(crate) doc: Option<String>,
  pub(crate) name: String,
}

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
  SessionMetadata {
    command_line: String,
    git_branch: Option<String>,
    git_commit: Option<String>,
    git_dirty: bool,
    recipes: Vec<RecipeNode>,
    timestamp_ms: u64,
    working_dir: String,
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

pub(crate) fn set_command_line(cmd: String) {
  *COMMAND_LINE.lock().unwrap() = Some(cmd);
}

pub(crate) fn command_line() -> String {
  COMMAND_LINE
    .lock()
    .unwrap()
    .clone()
    .unwrap_or_default()
}

pub(crate) fn is_enabled() -> bool {
  LIVE_ENABLED.load(Ordering::Relaxed)
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

fn git_cmd(working_dir: &std::path::Path, args: &[&str]) -> Option<String> {
  Command::new("git")
    .args(args)
    .current_dir(working_dir)
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::null())
    .output()
    .ok()
    .filter(|o| o.status.success())
    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

pub(crate) fn git_info(working_dir: &std::path::Path) -> (Option<String>, Option<String>, bool) {
  let branch = git_cmd(working_dir, &["rev-parse", "--abbrev-ref", "HEAD"]);
  let commit = git_cmd(working_dir, &["rev-parse", "--short", "HEAD"]);
  let dirty = git_cmd(working_dir, &["status", "--porcelain", "--untracked-files=no"])
    .is_some_and(|s| !s.is_empty());
  (branch, commit, dirty)
}
