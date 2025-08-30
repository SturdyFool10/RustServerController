/// Spawns multiple async tasks, each receiving a clone of the provided state.
///
/// # Example
/// ```
/// spawn_tasks!(app_state.clone(), start_servers, start_slave)
/// ```
#[macro_export]
macro_rules! spawn_tasks {
    ($state:expr, $($task:expr),*) => {
        {
            let handles: Vec<_> = vec![
                $(
                    spawn($task($state.clone())),
                )*
            ];

            handles
        }
    };
}

/// Creates an async listener future that waits for a specific key press and then calls `stop()` on the provided app.
///
/// # Example
/// ```
/// async_listener!("t", app_state)
/// ```
#[macro_export]
macro_rules! async_listener {
    ($key:expr, $app:expr) => {{
        use crossterm::event::{poll, read, Event, KeyCode};
        use tokio::task::yield_now;

        // Create a future that waits for the key combination
        let key_future = async move {
            loop {
                yield_now().await;

                if poll(std::time::Duration::from_millis(25)).expect("Failed to poll for events") {
                    if let Event::Key(key_event) = read().expect("Failed to read event") {
                        if key_event.code == KeyCode::Char($key.chars().next().unwrap()) {
                            $app.stop();
                            break;
                        }
                    }
                }
            }
        };
        // Return the key combination future
        key_future
    }};
}
