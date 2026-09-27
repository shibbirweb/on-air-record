//! The database failing underneath a running service, and the service starting against a damaged one.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use tower::ServiceExt;

use super::{Scratch, Store, TEST_BUSY_TIMEOUT};
use crate::app::AppState;
use crate::config::AppConfig;
use crate::models::SettingsPatch;

/// A running application on a scratch data directory, with the busy timeout shortened.
struct Running {
    state: Option<Arc<AppState>>,
    /// Held for its drop, which deletes the directory once the state above has closed the database.
    _scratch: Scratch,
}

impl Running {
    fn start(name: &str) -> Self {
        let scratch = Scratch::new(name);
        let state = bootstrap(&scratch).expect("bootstrap");
        state
            .segments
            .database()
            .with_connection(|conn| Ok(conn.busy_timeout(TEST_BUSY_TIMEOUT)?))
            .expect("busy timeout");
        Self {
            state: Some(state),
            _scratch: scratch,
        }
    }

    fn state(&self) -> &Arc<AppState> {
        self.state.as_ref().expect("running")
    }

    async fn get(&self, path: &str) -> StatusCode {
        let router = crate::routes::build(self.state().clone());
        let request = Request::builder()
            .uri(path)
            .header(header::HOST, "recorder.test")
            .body(Body::empty())
            .expect("request");
        router.oneshot(request).await.expect("response").status()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // The connection closes before the scratch directory is deleted, for Windows.
        drop(self.state.take());
    }
}

fn config_for(scratch: &Scratch) -> AppConfig {
    AppConfig {
        data_dir: scratch.root.join("data"),
        ..AppConfig::default()
    }
}

fn bootstrap(scratch: &Scratch) -> crate::error::AppResult<Arc<AppState>> {
    AppState::bootstrap(config_for(scratch))
}

/// Hide the table every access decision reads, as a damaged or half restored database would.
///
/// An exclusive lock cannot stage this: in WAL mode another program cannot take the database away from
/// a connection that already has it open, which is also why readers here never wait on a writer.
fn hide_access_table(connection: &rusqlite::Connection) {
    connection
        .execute_batch("ALTER TABLE auth_state RENAME TO auth_state_hidden;")
        .expect("hide");
}

fn restore_access_table(connection: &rusqlite::Connection) {
    connection
        .execute_batch("ALTER TABLE auth_state_hidden RENAME TO auth_state;")
        .expect("restore");
}

#[test]
fn a_settings_change_the_locked_database_refused_leaves_the_running_settings_untouched() {
    let store = Store::new("settings-locked");
    let before = store.settings.current();

    let locker = store.second_connection();
    locker.execute_batch("BEGIN EXCLUSIVE;").expect("lock");

    let refused = store.settings.update(&SettingsPatch {
        segment_seconds: Some(before.segment_seconds + 5),
        ..SettingsPatch::default()
    });
    assert!(
        refused.is_err(),
        "a write under another writer's lock fails"
    );
    // Saved first, cached second: had the cache moved first, the service would run on a setting that
    // disappears at the next restart.
    assert_eq!(
        store.settings.current().segment_seconds,
        before.segment_seconds
    );

    locker.execute_batch("COMMIT;").expect("unlock");
    let accepted = store
        .settings
        .update(&SettingsPatch {
            segment_seconds: Some(before.segment_seconds + 5),
            ..SettingsPatch::default()
        })
        .expect("the same change once the lock is gone");
    assert_eq!(accepted.segment_seconds, before.segment_seconds + 5);
}

#[tokio::test]
async fn an_unreadable_access_mode_fails_closed_on_every_protected_route() {
    let running = Running::start("auth-unreadable");
    assert_eq!(running.get("/api/status").await, StatusCode::OK);

    let other = rusqlite::Connection::open(running.state().config.database_path()).expect("other");
    hide_access_table(&other);

    // The access mode itself cannot be read, so nobody can be let in: an error, never an open door.
    assert!(running.state().auth.mode().is_err());
    assert_eq!(
        running.get("/api/status").await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        running.get("/api/timeline/range").await,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    // The health probe needs no database, which is what lets a container's health check tell a live
    // process from a dead one while the database is in trouble.
    assert_eq!(running.get("/api/health").await, StatusCode::OK);

    restore_access_table(&other);
    assert_eq!(
        running.get("/api/status").await,
        StatusCode::OK,
        "the service recovers by itself once the database is whole again"
    );
}

#[test]
fn startup_refuses_a_recordings_path_that_is_a_file_and_says_which() {
    let scratch = Scratch::new("startup-recordings-file");
    let config = config_for(&scratch);
    std::fs::create_dir_all(&config.data_dir).expect("data dir");
    std::fs::write(config.recordings_dir(), b"not a directory").expect("file");

    let error = match bootstrap(&scratch) {
        Ok(_) => panic!("started with nowhere to record"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains(&config.recordings_dir().display().to_string()),
        "the error names the path to fix, got: {error}"
    );
}

#[cfg(unix)]
#[test]
fn startup_refuses_a_data_directory_it_cannot_write() {
    let mut scratch = Scratch::new("startup-read-only");
    let config = config_for(&scratch);
    std::fs::create_dir_all(&config.data_dir).expect("data dir");
    if !scratch.make_read_only(&config.data_dir) {
        return;
    }

    let error = match bootstrap(&scratch) {
        Ok(_) => panic!("started in a directory it cannot write"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains(&config.data_dir.display().to_string()),
        "the error names the directory, got: {error}"
    );
}

#[test]
fn startup_refuses_a_corrupt_database_without_touching_it() {
    let scratch = Scratch::new("startup-corrupt");
    let config = config_for(&scratch);
    std::fs::create_dir_all(config.recordings_dir()).expect("dirs");
    let garbage: Vec<u8> = (0..16_384u32).map(|index| (index * 7 + 3) as u8).collect();
    std::fs::write(config.database_path(), &garbage).expect("garbage");

    let error = match bootstrap(&scratch) {
        Ok(_) => panic!("started on a database that is not one"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains(&config.database_path().display().to_string()),
        "the error names the database file, got: {error}"
    );
    // Whatever is in that file may be somebody's only copy of their index, so a failed start must leave
    // it byte for byte as it was, for them to restore or repair.
    assert_eq!(
        std::fs::read(config.database_path()).expect("still there"),
        garbage
    );
}

#[cfg(unix)]
#[test]
fn startup_refuses_a_read_only_database_rather_than_running_unable_to_save() {
    use std::os::unix::fs::PermissionsExt;

    let scratch = Scratch::new("startup-read-only-db");
    let config = config_for(&scratch);
    drop(bootstrap(&scratch).expect("first start"));

    let database_path = config.database_path();
    std::fs::set_permissions(&database_path, std::fs::Permissions::from_mode(0o444))
        .expect("chmod");
    if std::fs::OpenOptions::new()
        .write(true)
        .open(&database_path)
        .is_ok()
    {
        eprintln!("skipping: permissions are not enforced here, probably running as root");
        return;
    }

    let outcome = bootstrap(&scratch);
    std::fs::set_permissions(&database_path, std::fs::Permissions::from_mode(0o644))
        .expect("chmod back");

    let error = match outcome {
        Ok(_) => panic!("started on a database it cannot write"),
        Err(error) => error.to_string(),
    };
    assert!(
        error.contains(&database_path.display().to_string()),
        "the error names the database file, got: {error}"
    );
}

/// Read the stream until a control message of type `kind`, failing if it closes first. Reading also lets
/// the client library answer the server's pings, as a browser does, so the stream is not dropped as idle.
async fn until_control(
    client: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    kind: &str,
) -> serde_json::Value {
    use futures_util::StreamExt;
    use tokio_tungstenite::tungstenite::Message;

    loop {
        let message = tokio::time::timeout(std::time::Duration::from_secs(60), client.next())
            .await
            .expect("the server said something in time");
        match message {
            Some(Ok(Message::Text(text))) => {
                let value: serde_json::Value = serde_json::from_str(text.as_str()).expect("JSON");
                if value["type"] == kind {
                    return value;
                }
            }
            None | Some(Err(_)) | Some(Ok(Message::Close(_))) => {
                panic!("the stream closed while waiting for {kind}")
            }
            Some(Ok(_)) => {}
        }
    }
}

/// `ws/session.rs` re-checks access every 15 seconds and treats a database error two ways on purpose: the
/// stream stays up, because a listener should not be cut off over something that is not their fault, and
/// the listener list is withheld, because it names people and where they connect from. The functions are
/// private to the session, so this drives them the only way from outside: a real socket, a paused clock,
/// and a database that stops answering the access question.
#[tokio::test(start_paused = true)]
async fn a_database_error_keeps_an_open_stream_playing_but_withholds_the_listener_list() {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::HeaderValue;
    use tokio_tungstenite::tungstenite::Message;

    let running = Running::start("stream-db-error");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a free port");
    let address = listener.local_addr().expect("address");
    let service = crate::routes::build(running.state().clone())
        .into_make_service_with_connect_info::<std::net::SocketAddr>();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, service).await;
    });

    let mut request = format!("ws://{address}/api/ws/stream")
        .into_client_request()
        .expect("request");
    request.headers_mut().insert(
        "origin",
        HeaderValue::from_str(&format!("http://{address}")).expect("origin"),
    );
    let (mut client, _) = tokio_tungstenite::connect_async(request)
        .await
        .expect("the stream opens");
    // An open recorder shows everybody the list.
    until_control(&mut client, "listeners").await;

    let other = rusqlite::Connection::open(running.state().config.database_path()).expect("other");
    hide_access_table(&other);

    until_control(&mut client, "listeners-hidden").await;
    // And the stream itself is still up: a ping is answered.
    client
        .send(Message::Text(
            serde_json::json!({ "type": "ping", "clientTimeMs": 1 })
                .to_string()
                .into(),
        ))
        .await
        .expect("send");
    until_control(&mut client, "pong").await;
    assert_eq!(running.state().listeners.count(), 1);

    restore_access_table(&other);
    until_control(&mut client, "listeners").await;

    drop(client);
    server.abort();
}
