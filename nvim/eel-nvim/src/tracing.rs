use std::io::{self, Write};
use std::sync::{Arc, mpsc};

use tracing::{Level, level_filters::LevelFilter};
use tracing_subscriber::{
    Layer,
    filter::{FilterExt, Targets, filter_fn},
    fmt::MakeWriter,
};

use eel::tracing::TracingLayer;
use nvim_oxi::api as nvim_api;

use crate::editor::NvimEditor;

const QUEUE_CAPACITY: usize = 64;

struct NvimWriter(mpsc::SyncSender<String>);

impl Write for &NvimWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let message = std::str::from_utf8(buf).map_err(io::Error::other)?;
        // Never wait for the UI thread, including when it is the caller.
        let _ = self.0.try_send(message.trim_ascii().to_owned());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for NvimWriter {
    type Writer = &'a Self;

    fn make_writer(&'a self) -> Self::Writer {
        self
    }
}

fn echo_messages<D: std::fmt::Display, E: std::fmt::Display>(
    messages: mpsc::Receiver<String>,
    mut echo: impl FnMut(String, &'static str) -> Result<Result<(), E>, D>,
) {
    while let Ok(message) = messages.recv() {
        let highlight = if message.starts_with("ERROR") {
            "DiagnosticError"
        } else if message.starts_with("WARN") {
            "DiagnosticWarn"
        } else {
            continue;
        };
        // Wait for each dispatch before starting the next. If the editor sink
        // breaks, stop feeding it; a failed wakeup can leave a dispatch queued.
        let error = match echo(message, highlight) {
            Ok(Ok(())) => continue,
            Ok(Err(error)) => format!("echo failed: {error}"),
            Err(error) => format!("dispatch failed: {error}"),
        };
        drop(messages);
        tracing::error!(%error, "Neovim log echo disabled; file logging continues");
        return;
    }
}

fn message_targets() -> Targets {
    Targets::new()
        .with_default(Level::WARN)
        .with_target("eel_nvim::tracing", LevelFilter::OFF)
        .with_target("eel_nvim::dispatcher", LevelFilter::OFF)
}

pub fn nvim_msg_layer(editor: Arc<NvimEditor>) -> TracingLayer {
    let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
    let worker = std::thread::Builder::new()
        .name("eel-nvim-log-echo".into())
        .spawn(move || {
            echo_messages(receiver, |message, highlight| {
                editor.dispatch(move || {
                    nvim_api::echo([(message, Some(highlight))], false, &Default::default())?;
                    nvim_api::command("redraw")
                })
            });
        });
    if let Err(error) = worker {
        tracing::error!(%error, "Could not start Neovim log echo worker");
    }

    message_layer(NvimWriter(sender))
}

fn message_layer(writer: NvimWriter) -> TracingLayer {
    let layer = tracing_subscriber::fmt::layer()
        .with_writer(writer)
        .without_time()
        .with_ansi(false)
        // A file-only event keeps its original target for RUST_LOG filtering.
        .with_filter(
            message_targets().and(filter_fn(|meta| meta.fields().field("file_only").is_none())),
        );
    Box::new(layer)
}

#[cfg(test)]
mod tests {
    use std::{sync::Mutex, time::Duration};

    use tracing_subscriber::{EnvFilter, layer::SubscriberExt};

    use super::*;

    #[derive(Clone, Default)]
    struct LogCapture(Arc<Mutex<Vec<u8>>>);

    impl Write for LogCapture {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn full_echo_queue_does_not_block_the_producer() {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (done, finished) = mpsc::channel();
        let producer = std::thread::spawn(move || {
            let writer = NvimWriter(sender);
            for _ in 0..1_000 {
                (&writer).write_all(b"ERROR test\n").unwrap();
            }
            done.send(()).unwrap();
        });
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        producer.join().unwrap();
        assert_eq!(receiver.into_iter().count(), QUEUE_CAPACITY);
    }

    #[test]
    fn editor_filter_excludes_disk_only_and_internal_events() {
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let subscriber = tracing_subscriber::registry()
            .with(message_layer(NvimWriter(sender)))
            .with(EnvFilter::new("info,suppressed=off"));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "app", "info");
            tracing::warn!(target: "app", "warning");
            tracing::error!(target: "app", "error");
            tracing::error!(target: "app", file_only = true, "repeat summary");
            tracing::error!(target: "eel_nvim::dispatcher", "dispatch failure");
            tracing::error!(target: "eel_nvim::tracing", "echo failure");
            tracing::warn!(target: "suppressed", "filtered by environment");
        });
        let messages: Vec<_> = receiver.try_iter().collect();
        assert_eq!(messages.len(), 2);
        assert!(messages[0].starts_with("WARN"));
        assert!(messages[1].starts_with("ERROR"));
    }

    #[test]
    fn echo_and_dispatch_failures_stop_echoes_but_leave_file_logging_intact() {
        for (result, diagnostic) in [
            (Ok(Err("injected")), "echo failed: injected"),
            (Err("injected"), "dispatch failed: injected"),
        ] {
            let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
            let file_output = LogCapture::default();
            let writer = file_output.clone();
            let file_layer: TracingLayer =
                Box::new(tracing_subscriber::fmt::layer().with_writer(move || writer.clone()));
            let subscriber = tracing_subscriber::registry()
                .with(vec![file_layer, message_layer(NvimWriter(sender.clone()))]);
            tracing::subscriber::with_default(subscriber, || {
                tracing::error!(target: "app", "first event");
                tracing::warn!(target: "app", "queued event");
                sender.try_send("ERROR unexpected echo".into()).unwrap();
                let mut attempts = 0;
                echo_messages(receiver, |message, highlight| {
                    attempts += 1;
                    assert!(message.contains("first event"));
                    assert_eq!(highlight, "DiagnosticError");
                    result
                });
                assert_eq!(attempts, 1);
                tracing::error!(target: "app", "later event");
            });
            let output = file_output.0.lock().unwrap();
            let output = std::str::from_utf8(&output).unwrap();
            assert_eq!(output.matches(diagnostic).count(), 1);
            for event in ["first event", "queued event", "later event"] {
                assert!(output.contains(event));
            }
        }
    }
}
