use serde::Serialize;
use tauri::Emitter;

/// Event sink used by networking services.
/// In GUI mode it forwards to Tauri, in headless mode it's a no-op.
#[derive(Clone)]
pub enum EventSink {
    Tauri(tauri::AppHandle),
    Noop,
}

impl EventSink {
    pub fn tauri(app: tauri::AppHandle) -> Self {
        Self::Tauri(app)
    }

    pub fn noop() -> Self {
        Self::Noop
    }

    pub fn emit<T>(&self, event: &str, payload: T)
    where
        T: Serialize + Clone,
    {
        if let Self::Tauri(app) = self {
            let _ = app.emit(event, payload);
        }
    }
}
