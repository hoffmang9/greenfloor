//! RAII guard that restores process environment variables on drop.

static ENV_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[doc(hidden)]
pub struct EnvRestoreGuard {
    saved: Vec<(String, Option<String>)>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvRestoreGuard {
    #[must_use]
    pub fn set(vars: &[(&str, &str)]) -> Self {
        let lock = ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut saved = Vec::new();
        for (key, value) in vars {
            saved.push(((*key).to_string(), std::env::var(key).ok()));
            std::env::set_var(key, value);
        }
        Self { saved, _lock: lock }
    }
}

impl Drop for EnvRestoreGuard {
    fn drop(&mut self) {
        for (key, previous) in self.saved.drain(..) {
            match previous {
                Some(value) => std::env::set_var(&key, value),
                None => std::env::remove_var(&key),
            }
        }
    }
}
