use futures_util::SinkExt;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite;

pub(crate) fn start(
  event_tx: broadcast::Sender<String>,
  server_url: &str,
  session_id: &str,
  project: &str,
) {
  let mut event_rx = event_tx.subscribe();

  // Convert http:// to ws:// and append the publish endpoint
  let ws_url = server_url
    .trim_end_matches('/')
    .replacen("http://", "ws://", 1)
    .replacen("https://", "wss://", 1)
    + "/api/publish";

  let session_id = session_id.to_string();
  let project = project.to_string();
  let ws_url_clone = ws_url.clone();

  std::thread::spawn(move || {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    rt.block_on(async {
      let conn = tokio_tungstenite::connect_async(&ws_url_clone).await;

      let Ok((ws_stream, _)) = conn else {
        eprintln!("Warning: failed to connect to live server at {ws_url_clone}");
        return;
      };

      let (mut write, _read) = futures_util::StreamExt::split(ws_stream);

      while let Ok(event_json) = event_rx.recv().await {
        let msg = serde_json::json!({
          "session_id": session_id,
          "project": project,
          "event": serde_json::from_str::<serde_json::Value>(&event_json)
            .unwrap_or_default()
        });

        let send_result = write
          .send(tungstenite::Message::Text(msg.to_string()))
          .await;

        if send_result.is_err() {
          eprintln!("Warning: lost connection to live server");
          break;
        }
      }
    });
  });
}
