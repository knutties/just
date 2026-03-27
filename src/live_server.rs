use std::sync::{Arc, Mutex};

use axum::{
  Router,
  extract::{
    State,
    ws::{Message, WebSocket, WebSocketUpgrade},
  },
  response::Html,
  routing::get,
};
use tokio::sync::broadcast;

struct AppState {
  history: Mutex<Vec<String>>,
  tx: broadcast::Sender<String>,
}

pub(crate) fn start(tx: broadcast::Sender<String>) -> u16 {
  let (port_tx, port_rx) = std::sync::mpsc::channel();

  let mut history_rx = tx.subscribe();
  let state = Arc::new(AppState {
    history: Mutex::new(Vec::new()),
    tx,
  });

  let history_state = Arc::clone(&state);

  std::thread::spawn(move || {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
      // Background task to record event history
      let hs = history_state;
      tokio::spawn(async move {
        while let Ok(msg) = history_rx.recv().await {
          if let Ok(mut history) = hs.history.lock() {
            history.push(msg);
          }
        }
      });

      let app = Router::new()
        .route("/", get(index_handler))
        .route("/ws", get(ws_handler))
        .with_state(state);

      let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind live server");

      let port = listener
        .local_addr()
        .expect("failed to get local addr")
        .port();

      port_tx.send(port).expect("failed to send port");

      axum::serve(listener, app)
        .await
        .expect("live server failed");
    });
  });

  port_rx.recv().expect("failed to receive port")
}

async fn index_handler() -> Html<&'static str> {
  Html(LIVE_HTML)
}

async fn ws_handler(
  ws: WebSocketUpgrade,
  State(state): State<Arc<AppState>>,
) -> axum::response::Response {
  ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
  // Replay history — clone to release the lock before awaiting
  let history_snapshot = state
    .history
    .lock()
    .map(|h| h.clone())
    .unwrap_or_default();

  for msg in history_snapshot {
    if socket.send(Message::Text(msg)).await.is_err() {
      return;
    }
  }

  // Stream live events
  let mut rx = state.tx.subscribe();
  while let Ok(msg) = rx.recv().await {
    if socket.send(Message::Text(msg)).await.is_err() {
      break;
    }
  }
}

const LIVE_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>just — Live Visualization</title>
<style>
  :root {
    --bg-primary: #0d1117;
    --bg-secondary: #161b22;
    --bg-tertiary: #21262d;
    --bg-hover: #30363d;
    --border: #30363d;
    --text-primary: #e6edf3;
    --text-secondary: #8b949e;
    --text-muted: #484f58;
    --accent-green: #3fb950;
    --accent-red: #f85149;
    --accent-yellow: #d29922;
    --accent-blue: #58a6ff;
    --accent-purple: #bc8cff;
  }

  * { margin: 0; padding: 0; box-sizing: border-box; }

  body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Noto Sans', Helvetica, Arial, sans-serif;
    background: var(--bg-primary);
    color: var(--text-primary);
    height: 100vh;
    overflow: hidden;
  }

  .header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-secondary);
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .header-logo {
    font-size: 20px;
    font-weight: 700;
    color: var(--text-primary);
  }

  .header-logo span {
    color: var(--accent-purple);
  }

  .run-status {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 14px;
    padding: 4px 12px;
    border-radius: 20px;
    background: var(--bg-tertiary);
  }

  .run-status .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-muted);
  }

  .run-status.running .dot {
    background: var(--accent-yellow);
    animation: pulse 1.5s ease-in-out infinite;
  }

  .run-status.success .dot { background: var(--accent-green); }
  .run-status.failed .dot { background: var(--accent-red); }

  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(1.3); }
  }

  .header-timer {
    font-size: 13px;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .container {
    display: flex;
    height: calc(100vh - 57px);
  }

  .sidebar {
    width: 320px;
    min-width: 320px;
    border-right: 1px solid var(--border);
    background: var(--bg-secondary);
    overflow-y: auto;
    padding: 8px 0;
  }

  .sidebar-header {
    padding: 8px 16px 12px;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-secondary);
  }

  .recipe-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 16px;
    cursor: pointer;
    border-left: 2px solid transparent;
    transition: background 0.15s, border-color 0.15s;
  }

  .recipe-item:hover {
    background: var(--bg-hover);
  }

  .recipe-item.selected {
    background: var(--bg-tertiary);
    border-left-color: var(--accent-blue);
  }

  .recipe-item.dependency {
    padding-left: 36px;
  }

  .recipe-icon {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .recipe-icon svg { width: 16px; height: 16px; }

  .recipe-info {
    flex: 1;
    min-width: 0;
  }

  .recipe-name {
    font-size: 14px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .recipe-doc {
    font-size: 12px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 2px;
  }

  .recipe-duration {
    font-size: 12px;
    color: var(--text-secondary);
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
  }

  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }

  .main-header {
    padding: 12px 20px;
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--bg-secondary);
  }

  .main-title {
    font-size: 16px;
    font-weight: 600;
  }

  .log-container {
    flex: 1;
    overflow-y: auto;
    padding: 16px 0;
    font-family: 'SF Mono', 'Cascadia Code', 'Fira Code', 'Menlo', monospace;
    font-size: 13px;
    line-height: 1.6;
  }

  .log-line {
    display: flex;
    padding: 1px 20px;
    transition: background 0.1s;
  }

  .log-line:hover {
    background: var(--bg-tertiary);
  }

  .log-timestamp {
    color: var(--text-muted);
    margin-right: 16px;
    white-space: nowrap;
    user-select: none;
    flex-shrink: 0;
  }

  .log-content {
    white-space: pre-wrap;
    word-break: break-all;
    flex: 1;
  }

  .log-line.command .log-content {
    color: var(--accent-blue);
  }

  .log-line.success .log-content {
    color: var(--accent-green);
  }

  .log-line.error .log-content {
    color: var(--accent-red);
  }

  .log-line.info .log-content {
    color: var(--text-secondary);
    font-style: italic;
  }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-secondary);
    gap: 12px;
  }

  .empty-state svg {
    width: 48px;
    height: 48px;
    opacity: 0.4;
  }

  .empty-state p {
    font-size: 14px;
  }

  .connecting-banner {
    padding: 8px 16px;
    background: var(--bg-tertiary);
    border-bottom: 1px solid var(--border);
    font-size: 13px;
    color: var(--accent-yellow);
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .connecting-banner.connected {
    color: var(--accent-green);
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  .spinner {
    animation: spin 1s linear infinite;
    color: var(--accent-yellow);
  }

  /* Scrollbar */
  ::-webkit-scrollbar { width: 8px; }
  ::-webkit-scrollbar-track { background: transparent; }
  ::-webkit-scrollbar-thumb { background: var(--bg-hover); border-radius: 4px; }
  ::-webkit-scrollbar-thumb:hover { background: var(--text-muted); }
</style>
</head>
<body>
  <div class="header">
    <div class="header-left">
      <div class="header-logo"><span>just</span> live</div>
      <div class="run-status" id="runStatus">
        <div class="dot"></div>
        <span id="runStatusText">Waiting...</span>
      </div>
    </div>
    <div class="header-timer" id="headerTimer">00:00</div>
  </div>
  <div class="container">
    <div class="sidebar">
      <div class="sidebar-header">Recipes</div>
      <div id="recipeList"></div>
    </div>
    <div class="main">
      <div class="main-header">
        <div class="main-title" id="mainTitle">Select a recipe</div>
      </div>
      <div class="log-container" id="logContainer">
        <div class="empty-state" id="emptyState">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
            <path d="M13 10V3L4 14h7v7l9-11h-7z"/>
          </svg>
          <p>Waiting for recipe execution...</p>
        </div>
      </div>
    </div>
  </div>

<script>
const state = {
  recipes: {},
  recipeOrder: [],
  selectedRecipe: null,
  runStarted: false,
  runCompleted: false,
  runSuccess: false,
  runStartMs: 0,
};

const SVG = {
  pending: `<svg viewBox="0 0 16 16" fill="var(--text-muted)"><circle cx="8" cy="8" r="7" fill="none" stroke="var(--text-muted)" stroke-width="1.5"/></svg>`,
  running: `<svg viewBox="0 0 16 16" class="spinner"><circle cx="8" cy="8" r="7" fill="none" stroke="var(--accent-yellow)" stroke-width="1.5" stroke-dasharray="30 14"/></svg>`,
  success: `<svg viewBox="0 0 16 16" fill="var(--accent-green)"><path fill-rule="evenodd" d="M8 16A8 8 0 108 0a8 8 0 000 16zm3.78-9.72a.75.75 0 00-1.06-1.06L7.25 8.69 5.28 6.72a.75.75 0 00-1.06 1.06l2.5 2.5a.75.75 0 001.06 0l4-4z"/></svg>`,
  failed: `<svg viewBox="0 0 16 16" fill="var(--accent-red)"><path fill-rule="evenodd" d="M2.343 13.657A8 8 0 1113.657 2.343 8 8 0 012.343 13.657zM6.03 4.97a.75.75 0 00-1.06 1.06L6.94 8 4.97 9.97a.75.75 0 101.06 1.06L8 9.06l1.97 1.97a.75.75 0 101.06-1.06L9.06 8l1.97-1.97a.75.75 0 10-1.06-1.06L8 6.94 6.03 4.97z"/></svg>`,
};

function formatDuration(ms) {
  if (ms < 1000) return ms + 'ms';
  const secs = Math.floor(ms / 1000);
  const mins = Math.floor(secs / 60);
  if (mins > 0) return mins + 'm ' + (secs % 60) + 's';
  return secs + '.' + Math.floor((ms % 1000) / 100) + 's';
}

function formatTime(ms) {
  const d = new Date(ms);
  return d.toLocaleTimeString('en-US', { hour12: false });
}

function getRecipe(name) {
  if (!state.recipes[name]) {
    state.recipes[name] = {
      name: name,
      doc: null,
      status: 'pending',
      isDependency: false,
      startMs: 0,
      durationMs: 0,
      logs: [],
      errorMessage: null,
    };
    state.recipeOrder.push(name);
  }
  return state.recipes[name];
}

function handleEvent(event) {
  switch (event.type) {
    case 'run_started':
      state.runStarted = true;
      state.runStartMs = event.timestamp_ms;
      updateRunStatus('running', 'Running...');
      break;

    case 'recipe_started': {
      const r = getRecipe(event.name);
      r.status = 'running';
      r.isDependency = event.is_dependency;
      r.doc = event.doc;
      r.startMs = event.timestamp_ms;
      r.logs.push({ type: 'info', text: `Recipe started`, time: event.timestamp_ms });
      if (!state.selectedRecipe) selectRecipe(event.name);
      break;
    }

    case 'recipe_line': {
      const r = getRecipe(event.recipe);
      r.logs.push({ type: 'command', text: `$ ${event.command}`, time: event.timestamp_ms });
      if (state.selectedRecipe === event.recipe) autoScroll();
      break;
    }

    case 'recipe_line_completed': {
      const r = getRecipe(event.recipe);
      const status = event.success ? 'success' : 'error';
      const codeStr = event.code !== null ? ` (exit code ${event.code})` : '';
      const durStr = formatDuration(event.duration_ms);
      r.logs.push({
        type: status,
        text: `${event.success ? '✓' : '✗'} Completed in ${durStr}${codeStr}`,
        time: event.timestamp_ms,
      });
      if (state.selectedRecipe === event.recipe) autoScroll();
      break;
    }

    case 'recipe_completed': {
      const r = getRecipe(event.name);
      r.status = event.success ? 'success' : 'failed';
      r.durationMs = event.duration_ms;
      r.errorMessage = event.error_message;
      const durStr = formatDuration(event.duration_ms);
      if (event.success) {
        r.logs.push({ type: 'success', text: `Recipe completed successfully in ${durStr}`, time: event.timestamp_ms });
      } else {
        r.logs.push({ type: 'error', text: `Recipe failed: ${event.error_message || 'unknown error'} (${durStr})`, time: event.timestamp_ms });
      }
      // Auto-select next running recipe
      if (state.selectedRecipe === event.name) {
        const nextRunning = state.recipeOrder.find(n => state.recipes[n]?.status === 'running');
        if (nextRunning) selectRecipe(nextRunning);
      }
      break;
    }

    case 'run_completed':
      state.runCompleted = true;
      state.runSuccess = event.success;
      updateRunStatus(
        event.success ? 'success' : 'failed',
        event.success ? 'Completed' : 'Failed'
      );
      break;
  }

  renderSidebar();
  renderLogs();
}

function updateRunStatus(cls, text) {
  const el = document.getElementById('runStatus');
  el.className = 'run-status ' + cls;
  document.getElementById('runStatusText').textContent = text;
}

function selectRecipe(name) {
  state.selectedRecipe = name;
  document.getElementById('mainTitle').textContent = name;
  renderSidebar();
  renderLogs();
}

function renderSidebar() {
  const container = document.getElementById('recipeList');
  container.innerHTML = '';
  for (const name of state.recipeOrder) {
    const r = state.recipes[name];
    const item = document.createElement('div');
    item.className = 'recipe-item' +
      (r.isDependency ? ' dependency' : '') +
      (state.selectedRecipe === name ? ' selected' : '');
    item.onclick = () => selectRecipe(name);

    const icon = SVG[r.status] || SVG.pending;
    const dur = r.durationMs > 0 ? formatDuration(r.durationMs) : (r.status === 'running' ? '...' : '');

    item.innerHTML = `
      <div class="recipe-icon">${icon}</div>
      <div class="recipe-info">
        <div class="recipe-name">${escHtml(name)}</div>
        ${r.doc ? `<div class="recipe-doc">${escHtml(r.doc)}</div>` : ''}
      </div>
      ${dur ? `<div class="recipe-duration">${dur}</div>` : ''}
    `;
    container.appendChild(item);
  }
}

function renderLogs() {
  const container = document.getElementById('logContainer');
  const emptyState = document.getElementById('emptyState');
  const recipe = state.selectedRecipe ? state.recipes[state.selectedRecipe] : null;

  if (!recipe || recipe.logs.length === 0) {
    if (emptyState) emptyState.style.display = '';
    return;
  }

  if (emptyState) emptyState.style.display = 'none';

  // Only re-render if content changed
  const logKey = recipe.name + ':' + recipe.logs.length;
  if (container.dataset.logKey === logKey) return;
  container.dataset.logKey = logKey;

  let html = '';
  for (const log of recipe.logs) {
    const time = formatTime(log.time);
    html += `<div class="log-line ${log.type}">
      <span class="log-timestamp">${time}</span>
      <span class="log-content">${escHtml(log.text)}</span>
    </div>`;
  }
  container.innerHTML = html;
  autoScroll();
}

function autoScroll() {
  requestAnimationFrame(() => {
    const el = document.getElementById('logContainer');
    el.scrollTop = el.scrollHeight;
  });
}

function escHtml(s) {
  if (!s) return '';
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

// Timer
setInterval(() => {
  if (!state.runStarted || !state.runStartMs) return;
  const elapsed = state.runCompleted ? 0 : (Date.now() - state.runStartMs);
  if (!state.runCompleted && elapsed >= 0) {
    const secs = Math.floor(elapsed / 1000);
    const mins = Math.floor(secs / 60);
    document.getElementById('headerTimer').textContent =
      String(mins).padStart(2, '0') + ':' + String(secs % 60).padStart(2, '0');
  }
  // Update running recipe durations
  for (const name of state.recipeOrder) {
    const r = state.recipes[name];
    if (r.status === 'running' && r.startMs) {
      r.durationMs = Date.now() - r.startMs;
    }
  }
  renderSidebar();
}, 500);

// WebSocket connection
function connect() {
  const ws = new WebSocket(`ws://${location.host}/ws`);

  ws.onmessage = (e) => {
    try {
      const event = JSON.parse(e.data);
      handleEvent(event);
    } catch (err) {
      console.error('Failed to parse event:', err);
    }
  };

  ws.onclose = () => {
    if (state.runCompleted) return;
    setTimeout(connect, 1000);
  };

  ws.onerror = () => ws.close();
}

connect();
</script>
</body>
</html>
"#;
