use ferogram::session_backend::PersistedSession;
use ferogram::{Client, DcEntry, DcFlags, TransportKind};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;

fn fixture_config(
    path: &std::path::Path,
    addr: std::net::SocketAddr,
) -> anyhow::Result<ferogram::Config> {
    let session = PersistedSession {
        home_dc_id: 2,
        dcs: vec![DcEntry {
            dc_id: 2,
            addr: addr.to_string(),
            auth_key: Some([42; 256]),
            first_salt: 0,
            time_offset: 0,
            flags: DcFlags::NONE,
        }],
        updates_state: Default::default(),
        peers: Vec::new(),
        min_peers: Vec::new(),
        future_auth_token: None,
    };
    std::fs::write(path, session.to_bytes())?;
    Ok(Client::builder()
        .api_id(1)
        .api_hash("local-test")
        .session(path)
        .transport(TransportKind::Full)
        .process_updates(false)
        .build()?)
}

#[tokio::test]
async fn rejected_startup_does_not_leave_background_reconnects() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let config = fixture_config(&dir.path().join("test.session"), listener.local_addr()?)?;
    let session_before = std::fs::read(dir.path().join("test.session"))?;
    let reject = async {
        let (mut stream, _) = listener.accept().await?;
        let mut request = [0; 1024];
        assert!(stream.read(&mut request).await? > 0);
        stream.write_all(&(-429_i32).to_le_bytes()).await?;
        anyhow::Ok(())
    };
    let (connected, rejected) =
        timeout(Duration::from_secs(3), async { tokio::join!(Client::connect(config), reject) })
            .await?;
    rejected?;
    match connected {
        Err(error) => assert!(error.to_string().contains("-429"), "{error}"),
        Ok((_, shutdown)) => {
            shutdown.cancel();
            anyhow::bail!("a rejected startup unexpectedly connected");
        },
    }
    assert!(
        timeout(Duration::from_millis(1200), listener.accept()).await.is_err(),
        "a failed connect left a detached task reconnecting without an owner"
    );
    assert_eq!(std::fs::read(dir.path().join("test.session"))?, session_before);
    Ok(())
}

#[tokio::test]
async fn cancelled_startup_closes_its_transport() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let config = fixture_config(&dir.path().join("test.session"), listener.local_addr()?)?;
    let session_before = std::fs::read(dir.path().join("test.session"))?;
    let mut connect = Box::pin(Client::connect(config));
    let mut stream = tokio::select! {
        accepted = listener.accept() => accepted?.0,
        _ = &mut connect => anyhow::bail!("connect ended before the fixture accepted it"),
    };
    assert!(timeout(Duration::from_millis(100), &mut connect).await.is_err());
    // Drop the entire startup future while its initialization RPC is pending.
    drop(connect);
    let mut received = Vec::new();
    timeout(Duration::from_secs(2), stream.read_to_end(&mut received)).await??;
    assert!(!received.is_empty(), "the initialization request was never sent");
    assert_eq!(std::fs::read(dir.path().join("test.session"))?, session_before);
    Ok(())
}
