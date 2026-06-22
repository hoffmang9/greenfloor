/// Dispatch `tracing::event!` when the level is chosen at runtime.
#[macro_export]
macro_rules! event_at_level {
    ($level:expr, $($fields:tt)* ) => {
        match $level {
            ::tracing::Level::ERROR => ::tracing::event!(::tracing::Level::ERROR, $($fields)*),
            ::tracing::Level::WARN => ::tracing::event!(::tracing::Level::WARN, $($fields)*),
            ::tracing::Level::INFO => ::tracing::event!(::tracing::Level::INFO, $($fields)*),
            ::tracing::Level::DEBUG | ::tracing::Level::TRACE => {
                ::tracing::event!(::tracing::Level::DEBUG, $($fields)*)
            }
        }
    };
}

/// Emit a structured operator trace event (`service`, `event`, and `phase` are always set).
#[macro_export]
macro_rules! trace_event {
    ($level:ident, $ctx:expr, $event:expr, { $($fields:tt)* } ; $msg:literal) => {
        tracing::event!(
            tracing::Level::$level,
            service = ($ctx).service,
            event = $event,
            phase = ($ctx).phase,
            $($fields)*
            $msg
        );
    };
}

/// Like [`trace_event!`], but the tracing level is chosen at runtime.
#[macro_export]
macro_rules! trace_event_at_level {
    ($level:expr, $ctx:expr, $event:expr, { $($fields:tt)* } ; $msg:literal) => {
        $crate::event_at_level!(
            $level,
            service = ($ctx).service,
            event = $event,
            phase = ($ctx).phase,
            $($fields)*
            $msg
        )
    };
}
