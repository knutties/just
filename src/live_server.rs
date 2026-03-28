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

#[derive(Clone)]
struct AppState {
  sessions: Arc<Mutex<Vec<SessionInfo>>>,
  web_tx: broadcast::Sender<String>,
}

struct SessionInfo {
  events: Vec<String>,
  project: String,
  session_id: String,
  status: String,
}

/// Start the central server (for `just --serve`). Blocks forever.
pub(crate) fn start_central(port: u16) {
  let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
  rt.block_on(async {
    let (web_tx, _) = broadcast::channel(4096);
    let state = AppState {
      sessions: Arc::new(Mutex::new(Vec::new())),
      web_tx,
    };

    let app = Router::new()
      .route("/", get(index_handler))
      .route("/ws", get(ws_handler))
      .route("/api/publish", get(publish_handler))
      .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
      .await
      .unwrap_or_else(|e| panic!("failed to bind live server on {addr}: {e}"));

    let port = listener
      .local_addr()
      .expect("failed to get local addr")
      .port();

    eprintln!("Just live server running at http://0.0.0.0:{port}");
    eprintln!("Set JUST_LIVE_URL=http://127.0.0.1:{port} in your projects to publish here.");

    axum::serve(listener, app)
      .await
      .expect("live server failed");
  });
}

/// Start the embedded server (for `just --live` without `JUST_LIVE_URL`).
/// Returns the port.
pub(crate) fn start_embedded(
  event_tx: broadcast::Sender<String>,
  project: &str,
) -> u16 {
  let (port_tx, port_rx) = std::sync::mpsc::channel();

  let mut event_rx = event_tx.subscribe();
  let session_id = uuid::Uuid::new_v4().to_string();
  let project = project.to_string();

  let (web_tx, _) = broadcast::channel(4096);
  let state = AppState {
    sessions: Arc::new(Mutex::new(vec![SessionInfo {
      events: Vec::new(),
      project: project.clone(),
      session_id: session_id.clone(),
      status: "running".into(),
    }])),
    web_tx: web_tx.clone(),
  };

  let sessions = Arc::clone(&state.sessions);

  std::thread::spawn(move || {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
      // Forward local events to web broadcast as LiveMessages
      tokio::spawn(async move {
        while let Ok(event_json) = event_rx.recv().await {
          let wrapped = wrap_event(&session_id, &project, &event_json);

          if let Ok(mut s) = sessions.lock() {
            if let Some(session) = s.first_mut() {
              session.events.push(wrapped.clone());
              update_session_status(session, &event_json);
            }
          }

          let _ = web_tx.send(wrapped);
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

fn wrap_event(session_id: &str, project: &str, event_json: &str) -> String {
  format!(
    r#"{{"session_id":{},"project":{},"event":{}}}"#,
    serde_json::to_string(session_id).unwrap_or_default(),
    serde_json::to_string(project).unwrap_or_default(),
    event_json
  )
}

fn update_session_status(session: &mut SessionInfo, event_json: &str) {
  if let Ok(val) = serde_json::from_str::<serde_json::Value>(event_json) {
    if let Some(event_type) = val.get("type").and_then(|t| t.as_str()) {
      match event_type {
        "run_started" => session.status = "running".into(),
        "run_completed" => {
          let success = val.get("success").and_then(serde_json::Value::as_bool).unwrap_or(false);
          session.status = if success { "completed" } else { "failed" }.into();
        }
        _ => {}
      }
    }
  }
}

async fn index_handler() -> Html<&'static str> {
  Html(LIVE_HTML)
}

async fn ws_handler(
  ws: WebSocketUpgrade,
  State(state): State<AppState>,
) -> axum::response::Response {
  ws.on_upgrade(move |socket| handle_web_socket(socket, state))
}

async fn handle_web_socket(mut socket: WebSocket, state: AppState) {
  // Replay all session events
  let all_events: Vec<String> = state
    .sessions
    .lock()
    .map(|sessions| {
      sessions
        .iter()
        .flat_map(|s| s.events.clone())
        .collect()
    })
    .unwrap_or_default();

  for event in all_events {
    if socket.send(Message::Text(event)).await.is_err() {
      return;
    }
  }

  // Stream live events
  let mut rx = state.web_tx.subscribe();
  while let Ok(msg) = rx.recv().await {
    if socket.send(Message::Text(msg)).await.is_err() {
      break;
    }
  }
}

async fn publish_handler(
  ws: WebSocketUpgrade,
  State(state): State<AppState>,
) -> axum::response::Response {
  ws.on_upgrade(move |socket| handle_publish(socket, state))
}

async fn handle_publish(mut socket: WebSocket, state: AppState) {
  let mut session_id: Option<String> = None;

  while let Some(Ok(msg)) = socket.recv().await {
    let Message::Text(text) = msg else {
      continue;
    };

    // Parse the LiveMessage wrapper
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
      continue;
    };

    let sid = parsed
      .get("session_id")
      .and_then(|v| v.as_str())
      .unwrap_or("unknown")
      .to_string();

    let project = parsed
      .get("project")
      .and_then(|v| v.as_str())
      .unwrap_or("unknown")
      .to_string();

    // Create session on first message
    if session_id.is_none() {
      session_id = Some(sid.clone());
      if let Ok(mut sessions) = state.sessions.lock() {
        sessions.push(SessionInfo {
          events: Vec::new(),
          project,
          session_id: sid.clone(),
          status: "running".into(),
        });
      }
    }

    // Store event and update status
    if let Ok(mut sessions) = state.sessions.lock() {
      if let Some(session) = sessions.iter_mut().find(|s| s.session_id == sid) {
        session.events.push(text.clone());
        if let Some(event) = parsed.get("event") {
          if let Ok(event_str) = serde_json::to_string(event) {
            update_session_status(session, &event_str);
          }
        }
      }
    }

    // Broadcast to web UI clients
    let _ = state.web_tx.send(text);
  }

  // Connection closed — mark session as disconnected if still running
  if let Some(sid) = session_id {
    if let Ok(mut sessions) = state.sessions.lock() {
      if let Some(session) = sessions.iter_mut().find(|s| s.session_id == sid) {
        if session.status == "running" {
          session.status = "disconnected".into();

          // Broadcast a synthetic disconnected event
          let wrapped = wrap_event(
            &sid,
            &session.project,
            r#"{"type":"run_completed","success":false,"duration_ms":0,"timestamp_ms":0}"#,
          );
          let _ = state.web_tx.send(wrapped);
        }
      }
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
    padding: 12px 24px;
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

  .header-logo span { color: var(--accent-purple); }

  .session-tabs {
    display: flex;
    gap: 2px;
    padding: 0 24px;
    background: var(--bg-secondary);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    min-height: 38px;
    align-items: flex-end;
  }

  .session-tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 16px;
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
    cursor: pointer;
    border-bottom: 2px solid transparent;
    white-space: nowrap;
    transition: color 0.15s, border-color 0.15s;
  }

  .session-tab:hover { color: var(--text-primary); }

  .session-tab.active {
    color: var(--text-primary);
    border-bottom-color: var(--accent-blue);
  }

  .session-tab .tab-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-muted);
    flex-shrink: 0;
  }

  .session-tab .tab-dot.running {
    background: var(--accent-yellow);
    animation: pulse 1.5s ease-in-out infinite;
  }
  .session-tab .tab-dot.completed { background: var(--accent-green); }
  .session-tab .tab-dot.failed, .session-tab .tab-dot.disconnected { background: var(--accent-red); }

  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(1.3); }
  }

  .header-info {
    font-size: 13px;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .session-count {
    padding: 2px 8px;
    border-radius: 10px;
    background: var(--bg-tertiary);
    font-size: 12px;
  }

  .container {
    display: flex;
    height: calc(100vh - 95px);
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

  .recipe-item:hover { background: var(--bg-hover); }

  .recipe-item.selected {
    background: var(--bg-tertiary);
    border-left-color: var(--accent-blue);
  }

  .recipe-item.dependency { padding-left: 36px; }

  .recipe-icon {
    width: 20px;
    height: 20px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }

  .recipe-icon svg { width: 16px; height: 16px; }

  .recipe-info { flex: 1; min-width: 0; }

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

  .main-title { font-size: 16px; font-weight: 600; }

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

  .log-line:hover { background: var(--bg-tertiary); }

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

  .log-line.command .log-content { color: var(--accent-blue); }
  .log-line.success .log-content { color: var(--accent-green); }
  .log-line.error .log-content { color: var(--accent-red); }
  .log-line.info .log-content { color: var(--text-secondary); font-style: italic; }

  .empty-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-secondary);
    gap: 12px;
  }

  .empty-state svg { width: 48px; height: 48px; opacity: 0.4; }
  .empty-state p { font-size: 14px; }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
  .spinner { animation: spin 1s linear infinite; color: var(--accent-yellow); }

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
    </div>
    <div class="header-info">
      <span class="session-count" id="sessionCount">0 sessions</span>
    </div>
  </div>
  <div class="session-tabs" id="sessionTabs"></div>
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
// Multi-session state
const sessions = {};    // session_id -> session state
const sessionOrder = [];
let activeSession = null;

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

function getSession(sessionId, project) {
  if (!sessions[sessionId]) {
    sessions[sessionId] = {
      sessionId,
      project: project || 'unknown',
      status: 'running',
      recipes: {},
      recipeOrder: [],
      selectedRecipe: null,
      runStartMs: 0,
    };
    sessionOrder.push(sessionId);
  }
  return sessions[sessionId];
}

function getRecipe(session, name) {
  if (!session.recipes[name]) {
    session.recipes[name] = {
      name,
      doc: null,
      status: 'pending',
      isDependency: false,
      startMs: 0,
      durationMs: 0,
      logs: [],
    };
    session.recipeOrder.push(name);
  }
  return session.recipes[name];
}

function handleMessage(raw) {
  const msg = JSON.parse(raw);

  // LiveMessage format: { session_id, project, event }
  const sessionId = msg.session_id;
  const project = msg.project;
  const event = msg.event;

  if (!sessionId || !event) return;

  const session = getSession(sessionId, project);

  // Auto-activate first session
  if (!activeSession) switchSession(sessionId);

  switch (event.type) {
    case 'run_started':
      session.status = 'running';
      session.runStartMs = event.timestamp_ms;
      break;

    case 'recipe_started': {
      const r = getRecipe(session, event.name);
      r.status = 'running';
      r.isDependency = event.is_dependency;
      r.doc = event.doc;
      r.startMs = event.timestamp_ms;
      r.logs.push({ type: 'info', text: 'Recipe started', time: event.timestamp_ms });
      if (activeSession === sessionId && !session.selectedRecipe) {
        session.selectedRecipe = event.name;
      }
      break;
    }

    case 'recipe_line': {
      const r = getRecipe(session, event.recipe);
      r.logs.push({ type: 'command', text: '$ ' + event.command, time: event.timestamp_ms });
      break;
    }

    case 'recipe_line_completed': {
      const r = getRecipe(session, event.recipe);
      const status = event.success ? 'success' : 'error';
      const codeStr = event.code !== null ? ' (exit code ' + event.code + ')' : '';
      r.logs.push({
        type: status,
        text: (event.success ? '\u2713' : '\u2717') + ' Completed in ' + formatDuration(event.duration_ms) + codeStr,
        time: event.timestamp_ms,
      });
      break;
    }

    case 'recipe_completed': {
      const r = getRecipe(session, event.name);
      r.status = event.success ? 'success' : 'failed';
      r.durationMs = event.duration_ms;
      const durStr = formatDuration(event.duration_ms);
      if (event.success) {
        r.logs.push({ type: 'success', text: 'Recipe completed successfully in ' + durStr, time: event.timestamp_ms });
      } else {
        r.logs.push({ type: 'error', text: 'Recipe failed: ' + (event.error_message || 'unknown error') + ' (' + durStr + ')', time: event.timestamp_ms });
      }
      // Auto-select next running recipe
      if (activeSession === sessionId && session.selectedRecipe === event.name) {
        const next = session.recipeOrder.find(n => session.recipes[n]?.status === 'running');
        if (next) session.selectedRecipe = next;
      }
      break;
    }

    case 'run_completed':
      session.status = event.success ? 'completed' : 'failed';
      break;
  }

  render();
}

function switchSession(sessionId) {
  activeSession = sessionId;
  render();
}

function render() {
  renderSessionTabs();
  renderSidebar();
  renderLogs();
  renderSessionCount();
}

function renderSessionCount() {
  const n = sessionOrder.length;
  document.getElementById('sessionCount').textContent = n + (n === 1 ? ' session' : ' sessions');
}

function renderSessionTabs() {
  const container = document.getElementById('sessionTabs');
  container.innerHTML = '';
  for (const sid of sessionOrder) {
    const s = sessions[sid];
    const tab = document.createElement('div');
    tab.className = 'session-tab' + (activeSession === sid ? ' active' : '');
    tab.onclick = () => switchSession(sid);
    tab.innerHTML = '<div class="tab-dot ' + s.status + '"></div>' + escHtml(s.project);
    container.appendChild(tab);
  }
}

function renderSidebar() {
  const container = document.getElementById('recipeList');
  container.innerHTML = '';

  if (!activeSession || !sessions[activeSession]) return;
  const session = sessions[activeSession];

  for (const name of session.recipeOrder) {
    const r = session.recipes[name];
    const item = document.createElement('div');
    item.className = 'recipe-item' +
      (r.isDependency ? ' dependency' : '') +
      (session.selectedRecipe === name ? ' selected' : '');
    item.onclick = () => { session.selectedRecipe = name; render(); };

    const icon = SVG[r.status] || SVG.pending;
    const dur = r.durationMs > 0 ? formatDuration(r.durationMs) : (r.status === 'running' ? '...' : '');

    item.innerHTML =
      '<div class="recipe-icon">' + icon + '</div>' +
      '<div class="recipe-info">' +
        '<div class="recipe-name">' + escHtml(name) + '</div>' +
        (r.doc ? '<div class="recipe-doc">' + escHtml(r.doc) + '</div>' : '') +
      '</div>' +
      (dur ? '<div class="recipe-duration">' + dur + '</div>' : '');
    container.appendChild(item);
  }
}

function renderLogs() {
  const container = document.getElementById('logContainer');
  const emptyState = document.getElementById('emptyState');

  if (!activeSession || !sessions[activeSession]) {
    if (emptyState) emptyState.style.display = '';
    return;
  }

  const session = sessions[activeSession];
  const recipe = session.selectedRecipe ? session.recipes[session.selectedRecipe] : null;

  if (!recipe || recipe.logs.length === 0) {
    if (emptyState) emptyState.style.display = '';
    document.getElementById('mainTitle').textContent = 'Select a recipe';
    return;
  }

  if (emptyState) emptyState.style.display = 'none';
  document.getElementById('mainTitle').textContent = session.project + ' > ' + recipe.name;

  const logKey = activeSession + ':' + recipe.name + ':' + recipe.logs.length;
  if (container.dataset.logKey === logKey) return;
  container.dataset.logKey = logKey;

  let html = '';
  for (const log of recipe.logs) {
    const time = formatTime(log.time);
    html += '<div class="log-line ' + log.type + '">' +
      '<span class="log-timestamp">' + time + '</span>' +
      '<span class="log-content">' + escHtml(log.text) + '</span>' +
    '</div>';
  }
  container.innerHTML = html;

  requestAnimationFrame(() => { container.scrollTop = container.scrollHeight; });
}

function escHtml(s) {
  if (!s) return '';
  return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
}

// Timer: update running recipe durations
setInterval(() => {
  for (const sid of sessionOrder) {
    const s = sessions[sid];
    for (const name of s.recipeOrder) {
      const r = s.recipes[name];
      if (r.status === 'running' && r.startMs) {
        r.durationMs = Date.now() - r.startMs;
      }
    }
  }
  renderSidebar();
}, 500);

// WebSocket connection
function connect() {
  const ws = new WebSocket('ws://' + location.host + '/ws');

  ws.onmessage = (e) => {
    try { handleMessage(e.data); }
    catch (err) { console.error('Failed to parse event:', err); }
  };

  ws.onclose = () => { setTimeout(connect, 1000); };
  ws.onerror = () => ws.close();
}

connect();
</script>
</body>
</html>
"#;
