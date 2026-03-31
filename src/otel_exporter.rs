use {
  crate::live_event::LiveEvent,
  opentelemetry::{
    InstrumentationScope, KeyValue,
    trace::{SpanContext, SpanId, SpanKind, Status, TraceFlags, TraceId, TraceState},
  },
  opentelemetry_sdk::trace::{SpanData, SpanEvents, SpanExporter, SpanLinks},
  std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
  },
  tokio::sync::broadcast,
};

struct RecipeSpan {
  command_spans: Vec<CommandSpan>,
  span_id: SpanId,
  start: SystemTime,
}

struct CommandSpan {
  command: String,
  span_id: SpanId,
  start: SystemTime,
}

struct OtelState {
  pending_spans: Vec<SpanData>,
  recipe_spans: HashMap<String, RecipeSpan>,
  root_span_id: SpanId,
  run_start: SystemTime,
  scope: InstrumentationScope,
  session_attrs: Vec<KeyValue>,
  trace_id: TraceId,
}

fn ts_to_systime(ms: u64) -> SystemTime {
  UNIX_EPOCH + Duration::from_millis(ms)
}

fn new_span_id() -> SpanId {
  SpanId::from_bytes(rand::random::<[u8; 8]>())
}

pub(crate) fn start(event_tx: broadcast::Sender<String>, service_name: &str) {
  let mut event_rx = event_tx.subscribe();

  let trace_id = TraceId::from_bytes(rand::random::<[u8; 16]>());
  let root_span_id = new_span_id();

  let scope = InstrumentationScope::builder("just")
    .with_version(env!("CARGO_PKG_VERSION"))
    .build();

  let service_name = service_name.to_string();
  let resource = opentelemetry_sdk::Resource::builder()
    .with_service_name(service_name)
    .build();

  let state = Arc::new(Mutex::new(OtelState {
    pending_spans: Vec::new(),
    recipe_spans: HashMap::new(),
    root_span_id,
    run_start: SystemTime::now(),
    scope,
    session_attrs: Vec::new(),
    trace_id,
  }));

  // Single thread with its own tokio runtime for receiving events and exporting spans
  std::thread::spawn(move || {
    let rt = tokio::runtime::Builder::new_current_thread()
      .enable_all()
      .build()
      .expect("failed to create otel runtime");

    rt.block_on(async {
      let mut exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
      {
        Ok(e) => e,
        Err(err) => {
          eprintln!("Warning: failed to create OTLP exporter: {err}");
          return;
        }
      };

      exporter.set_resource(&resource);

      loop {
        let event_json = match event_rx.recv().await {
          Ok(json) => json,
          Err(broadcast::error::RecvError::Closed) => break,
          Err(broadcast::error::RecvError::Lagged(_)) => continue,
        };

        let Ok(event) = serde_json::from_str::<LiveEvent>(&event_json) else {
          continue;
        };

        let is_run_completed = matches!(event, LiveEvent::RunCompleted { .. });
        handle_event(&state, event);

        let spans: Vec<SpanData> = state.lock().unwrap().pending_spans.drain(..).collect();
        if !spans.is_empty() {
          if let Err(err) = exporter.export(spans).await {
            eprintln!("Warning: failed to export OTel span: {err}");
          }
        }

        if is_run_completed {
          let _ = exporter.force_flush();
          break;
        }
      }
    });
  });
}

fn handle_event(state: &Arc<Mutex<OtelState>>, event: LiveEvent) {
  match event {
    LiveEvent::RunStarted { timestamp_ms } => {
      state.lock().unwrap().run_start = ts_to_systime(timestamp_ms);
    }

    LiveEvent::SessionMetadata {
      command_line,
      git_branch,
      git_commit,
      git_dirty,
      working_dir,
      ..
    } => {
      let mut s = state.lock().unwrap();
      s.session_attrs = vec![
        KeyValue::new("just.command", command_line),
        KeyValue::new("just.working_dir", working_dir),
      ];
      if let Some(branch) = git_branch {
        s.session_attrs.push(KeyValue::new("vcs.branch", branch));
      }
      if let Some(commit) = git_commit {
        s.session_attrs.push(KeyValue::new("vcs.commit", commit));
      }
      s.session_attrs
        .push(KeyValue::new("vcs.dirty", git_dirty));
    }

    LiveEvent::RecipeStarted {
      name, timestamp_ms, ..
    } => {
      let mut s = state.lock().unwrap();
      let span_id = new_span_id();
      s.recipe_spans.insert(
        name,
        RecipeSpan {
          command_spans: Vec::new(),
          span_id,
          start: ts_to_systime(timestamp_ms),
        },
      );
    }

    LiveEvent::RecipeLine {
      recipe,
      command,
      timestamp_ms,
    } => {
      let mut s = state.lock().unwrap();
      if let Some(rs) = s.recipe_spans.get_mut(&recipe) {
        rs.command_spans.push(CommandSpan {
          command,
          span_id: new_span_id(),
          start: ts_to_systime(timestamp_ms),
        });
      }
    }

    LiveEvent::RecipeLineCompleted {
      recipe,
      success,
      code,
      duration_ms,
      ..
    } => {
      let mut s = state.lock().unwrap();

      let span_data = if let Some(rs) = s.recipe_spans.get(&recipe) {
        rs.command_spans.last().map(|cs| {
          let end = cs.start + Duration::from_millis(duration_ms);
          let status = if success {
            Status::Ok
          } else {
            Status::error(format!("exit code {}", code.unwrap_or(-1)))
          };

          let mut attrs = vec![KeyValue::new("just.recipe", recipe)];
          if let Some(c) = code {
            attrs.push(KeyValue::new("process.exit.code", i64::from(c)));
          }

          build_span(
            &s,
            &format!("$ {}", cs.command),
            cs.span_id,
            rs.span_id,
            cs.start,
            end,
            status,
            attrs,
            SpanKind::Internal,
          )
        })
      } else {
        None
      };

      if let Some(data) = span_data {
        s.pending_spans.push(data);
      }
    }

    LiveEvent::RecipeCompleted {
      name,
      success,
      error_message,
      duration_ms,
      ..
    } => {
      let mut s = state.lock().unwrap();

      let span_data = s.recipe_spans.get(&name).map(|rs| {
        let end = rs.start + Duration::from_millis(duration_ms);
        let status = if success {
          Status::Ok
        } else {
          Status::error(error_message.unwrap_or_else(|| "failed".into()))
        };

        let attrs = vec![KeyValue::new("just.recipe", name.clone())];

        build_span(
          &s,
          &format!("recipe {name}"),
          rs.span_id,
          s.root_span_id,
          rs.start,
          end,
          status,
          attrs,
          SpanKind::Internal,
        )
      });

      if let Some(data) = span_data {
        s.pending_spans.push(data);
      }

      s.recipe_spans.remove(&name);
    }

    LiveEvent::RunCompleted {
      success,
      duration_ms,
      ..
    } => {
      let mut s = state.lock().unwrap();
      let end = s.run_start + Duration::from_millis(duration_ms);
      let status = if success {
        Status::Ok
      } else {
        Status::error("run failed")
      };

      let mut attrs = s.session_attrs.clone();
      attrs.push(KeyValue::new("just.success", success));

      let data = build_span(
        &s,
        "just run",
        s.root_span_id,
        SpanId::INVALID,
        s.run_start,
        end,
        status,
        attrs,
        SpanKind::Server,
      );

      s.pending_spans.push(data);
    }

    // All LiveEvent variants are handled above; this arm exists for
    // forward-compatibility if new variants are added.
    #[allow(unreachable_patterns)]
    _ => {}
  }
}

fn build_span(
  state: &OtelState,
  name: &str,
  span_id: SpanId,
  parent_span_id: SpanId,
  start: SystemTime,
  end: SystemTime,
  status: Status,
  attrs: Vec<KeyValue>,
  kind: SpanKind,
) -> SpanData {
  let span_context = SpanContext::new(
    state.trace_id,
    span_id,
    TraceFlags::SAMPLED,
    false,
    TraceState::default(),
  );

  SpanData {
    span_context,
    parent_span_id,
    parent_span_is_remote: false,
    span_kind: kind,
    name: Cow::Owned(name.to_string()),
    start_time: start,
    end_time: end,
    attributes: attrs,
    dropped_attributes_count: 0,
    events: SpanEvents::default(),
    links: SpanLinks::default(),
    status,
    instrumentation_scope: state.scope.clone(),
  }
}
