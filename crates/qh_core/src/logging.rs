use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;

pub struct Logger{
    guard: WorkerGuard
}

impl Logger{
    pub fn new()->Result<Self,()>{
        let file_appender = tracing_appender::rolling::daily(".logs/", "qh_logs.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        fmt()
            .with_writer(std::io::stdout)
            .with_writer(non_blocking)
            .init();
        Ok(Self {
            guard: _guard
        })
    }

    pub fn init()->Result<Self,()>{
        Self::new()
    }
}