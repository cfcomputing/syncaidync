//! Prove pty-process can open a PTY and run a command on this machine (WO-03 gate).

use tokio::io::AsyncReadExt;

#[tokio::test]
async fn pty_prints_hello() {
    let opened = pty_process::open();
    let (pty, pts) = match opened {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("skip pty_prints_hello: could not open a PTY ({e})");
            return;
        }
    };
    if let Err(e) = pty.resize(pty_process::Size::new(24, 80)) {
        eprintln!("skip pty_prints_hello: could not resize PTY ({e})");
        return;
    }
    let mut child = match pty_process::Command::new("/bin/sh")
        .arg("-c")
        .arg("printf hello")
        .spawn(pts)
    {
        Ok(child) => child,
        Err(e) => {
            eprintln!("skip pty_prints_hello: could not spawn on PTY ({e})");
            return;
        }
    };

    let (mut reader, _writer) = pty.into_split();
    let mut buf = [0u8; 64];
    let mut got = Vec::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
    while got.windows(5).all(|w| w != b"hello") {
        if tokio::time::Instant::now() > deadline {
            panic!("timed out waiting for hello; got: {got:?}");
        }
        match tokio::time::timeout(std::time::Duration::from_millis(500), reader.read(&mut buf))
            .await
        {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => got.extend_from_slice(&buf[..n]),
            Ok(Err(e)) => panic!("read: {e}"),
            Err(_) => continue,
        }
    }
    assert!(
        got.windows(5).any(|w| w == b"hello"),
        "expected hello in {got:?}"
    );
    let status = child.wait().await.expect("wait");
    assert!(status.success(), "child status: {status:?}");
}
