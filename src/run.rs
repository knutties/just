use super::*;

/// Main entry point into `just`. Parse arguments from `args` and run.
#[allow(clippy::missing_errors_doc)]
pub fn run(args: impl Iterator<Item = impl Into<OsString> + Clone>) -> Result<(), i32> {
  #[cfg(windows)]
  ansi_term::enable_ansi_support().ok();

  let arguments = Arguments::try_parse_from(args).map_err(|err| {
    err.print().ok();
    err.exit_code()
  })?;

  let config = Config::from_arguments(arguments).map_err(Error::from);

  let (color, verbosity) = config
    .as_ref()
    .map(|config| (config.color, config.verbosity))
    .unwrap_or_default();

  let live = config.as_ref().map(|c| c.live).unwrap_or(false);

  if live {
    let (tx, _rx) = tokio::sync::broadcast::channel(256);
    live_event::set_sender(tx.clone());

    let project = std::env::current_dir()
      .ok()
      .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
      .unwrap_or_else(|| "unknown".into());

    if let Ok(url) = std::env::var("JUST_LIVE_URL") {
      let session_id = uuid::Uuid::new_v4().to_string();
      live_client::start(tx, &url, &session_id, &project);
      eprintln!("Publishing live events to {url}");
    } else {
      let port = live_server::start_embedded(tx, &project);
      eprintln!("Live visualization at http://127.0.0.1:{port}");
    }
  }

  let loader = Loader::new();

  let result = config
    .and_then(|config| {
      SignalHandler::install(config.verbosity)?;
      config.subcommand.execute(&config, &loader)
    })
    .map_err(|error| {
      if !verbosity.quiet() && error.print_message() {
        eprintln!("{}", error.color_display(color.stderr()));
      }
      error.code().unwrap_or(EXIT_FAILURE)
    });

  if live {
    let success = result.is_ok();
    live_event::emit(live_event::LiveEvent::RunCompleted {
      success,
      duration_ms: live_event::run_elapsed_ms(),
      timestamp_ms: live_event::now_ms(),
    });

    // Give the client/server a moment to deliver the final event
    std::thread::sleep(std::time::Duration::from_millis(500));

    // If running with embedded server, keep alive for inspection
    if std::env::var("JUST_LIVE_URL").is_err() {
      if success {
        eprintln!("Recipes complete. Visualization server still running. Press Ctrl+C to exit.");
      } else {
        eprintln!("Recipes failed. Visualization server still running. Press Ctrl+C to exit.");
      }
      loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
      }
    }
  }

  result
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn run_can_be_called_more_than_once() {
    let tmp = testing::tempdir();
    fs::write(tmp.path().join("justfile"), "foo:").unwrap();
    let search_directory = format!("{}/", tmp.path().to_str().unwrap());
    run(["just", &search_directory].iter()).unwrap();
    run(["just", &search_directory].iter()).unwrap();
  }
}
