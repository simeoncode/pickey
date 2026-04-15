use std::env;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogLevel {
    Off,
    Normal,
    Debug,
}

pub fn level() -> LogLevel {
    match env::var("PICKEY_LOG").as_deref() {
        Ok("off") | Ok("OFF") => LogLevel::Off,
        Ok("debug") | Ok("DEBUG") => LogLevel::Debug,
        _ => LogLevel::Normal,
    }
}

/// Log a one-line summary (default level).
pub fn info(msg: &str) {
    if level() != LogLevel::Off {
        eprintln!("[🔑🤏] {}", msg);
    }
}

/// Log a warning (default level).
pub fn warn(msg: &str) {
    if level() != LogLevel::Off {
        eprintln!("[🔑🤏] WARN: {}", msg);
    }
}

/// Log an error (always shown unless OFF).
pub fn error(msg: &str) {
    if level() != LogLevel::Off {
        eprintln!("[🔑🤏] ERROR: {}", msg);
    }
}

/// Log a debug message (only in debug mode).
pub fn debug(msg: &str) {
    if level() == LogLevel::Debug {
        eprintln!("[🔑🤏] DEBUG: {}", msg);
    }
}
