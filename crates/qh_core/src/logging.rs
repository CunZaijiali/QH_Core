use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt;

pub struct Logger{
    guard: WorkerGuard
}

impl Logger{
    pub fn new()->Self{
        let file_appender = tracing_appender::rolling::daily(".logs/", "qh_logs.log");
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
        fmt()
            .with_writer(std::io::stdout)
            .with_writer(non_blocking)
            .init();
        Self { 
            guard: _guard
        }
    }
}