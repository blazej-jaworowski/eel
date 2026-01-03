use tracing::{error, level_filters::LevelFilter};
use tracing_subscriber::{
    EnvFilter, Layer, Registry, layer::SubscriberExt, util::SubscriberInitExt,
};

pub type TracingLayer = Box<dyn Layer<Registry> + Send + Sync>;

pub fn file_log_layer(log_dir: impl Into<String>) -> TracingLayer {
    let file_appender = tracing_appender::rolling::daily(log_dir.into(), "log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);

    Box::leak(Box::new(guard));
    Box::new(tracing_subscriber::fmt::layer().with_writer(writer))
}

pub fn init_tracing(layers: impl Into<Vec<TracingLayer>>) {
    tracing_subscriber::registry()
        .with(layers.into())
        .with(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .init();
}

pub trait ResultExt {
    fn log_err(self) -> Self;
    fn log_err_msg(self, message: &str) -> Self;
}

impl<R, E: std::error::Error + Sized> ResultExt for std::result::Result<R, E> {
    fn log_err(self) -> Self {
        self.log_err_msg("Error occured")
    }

    fn log_err_msg(self, message: &str) -> Self {
        self.inspect_err(|e| error!("{message}: {e}"))
    }
}
