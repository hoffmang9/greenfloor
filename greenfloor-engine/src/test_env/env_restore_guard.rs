//! RAII guard that restores process environment variables on drop.

pub struct EnvRestoreGuard {
    saved: Vec<(String, Option<String>)>,
}

impl EnvRestoreGuard {
    #[must_use]
    pub fn set(vars: &[(&str, &str)]) -> Self {
        let mut saved = Vec::new();
        for (key, value) in vars {
            saved.push(((*key).to_string(), std::env::var(key).ok()));
            std::env::set_var(key, value);
        }
        Self { saved }
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
