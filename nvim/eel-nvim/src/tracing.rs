use std::sync::Arc;

use tracing::{Level, level_filters::LevelFilter};
use tracing_subscriber::{Layer, filter::Targets, fmt::MakeWriter};

use eel::{
    async_runtime,
    tracing::{ResultExt, TracingLayer},
};

use nvim_oxi::api as nvim_api;

use crate::{editor::NvimEditor, error::IntoNvimResult};

struct NvimIoWriter {
    editor: Arc<NvimEditor>,
}

impl NvimIoWriter {
    fn new(editor: Arc<NvimEditor>) -> Self {
        NvimIoWriter { editor }
    }
}

impl std::io::Write for NvimIoWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let buf = buf.trim_ascii();

        let message = String::from_utf8(buf.to_vec()).map_err(std::io::Error::other)?;
        let len = buf.len();

        let highlight = match message {
            ref s if s.starts_with("ERROR") => Some("DiagnosticError"),
            ref s if s.starts_with("WARN") => Some("DiagnosticWarn"),
            _ => return Ok(len),
        };

        let editor = self.editor.clone();
        async_runtime::spawn(async move {
            editor
                .dispatch(move || {
                    nvim_api::echo([(message, highlight)], false, &Default::default())?;
                    nvim_api::command("redraw")
                })
                .await
                .log_err_msg("Failed to dispatch log echo")?
                .into_nvim()
                .log_err_msg("Log echo failed")?;

            Ok::<_, eel::Error>(())
        });

        Ok(len)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct NvimMakeWriter {
    editor: Arc<NvimEditor>,
}

impl<'a> MakeWriter<'a> for NvimMakeWriter {
    type Writer = tracing_appender::non_blocking::NonBlocking;

    fn make_writer(&self) -> Self::Writer {
        let (writer, guard) = tracing_appender::non_blocking(std::io::LineWriter::new(
            NvimIoWriter::new(self.editor.clone()),
        ));
        Box::leak(Box::new(guard));

        writer
    }
}

impl NvimMakeWriter {
    fn new(editor: Arc<NvimEditor>) -> Self {
        NvimMakeWriter { editor }
    }
}

pub fn nvim_msg_layer(editor: Arc<NvimEditor>) -> TracingLayer {
    let targets = Targets::new()
        .with_default(Level::WARN)
        .with_target("nvim_api_helper::nvim::tracing", LevelFilter::OFF)
        .with_target("nvim_api_helper::nvim::async_dispatch", LevelFilter::OFF);

    let layer = tracing_subscriber::fmt::layer()
        .with_writer(NvimMakeWriter::new(editor))
        .without_time()
        .with_ansi(false)
        .with_filter(targets);

    Box::new(layer)
}
