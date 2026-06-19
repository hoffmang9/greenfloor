use std::io::Write;
use std::sync::{Arc, Mutex};

pub struct TraceCapture {
    buf: Arc<Mutex<Vec<u8>>>,
    _guard: tracing::subscriber::DefaultGuard,
}

struct Writer(Arc<Mutex<Vec<u8>>>);

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("lock").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl TraceCapture {
    pub fn install() -> Self {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer_buf = buf.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .without_time()
            .with_writer(move || Writer(writer_buf.clone()))
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        Self { buf, _guard: guard }
    }

    pub fn logs(&self) -> String {
        String::from_utf8(self.buf.lock().expect("lock").clone()).expect("utf8")
    }

    pub fn count_substr(&self, needle: &str) -> usize {
        self.logs().matches(needle).count()
    }
}
