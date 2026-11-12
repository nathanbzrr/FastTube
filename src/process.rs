use std::io::{self, Read};
use std::process::{Command, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

fn read_output(mut reader: impl Read) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut buffer = [0; 8192];
    let mut oversized = false;
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(data.len());
        data.extend_from_slice(&buffer[..count.min(remaining)]);
        oversized |= count > remaining;
    }
    if oversized {
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "saída da consulta excede o limite",
        ))
    } else {
        Ok(data)
    }
}

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

pub fn output(
    command: &mut Command,
    cancellation: &Cancellation,
    timeout: Duration,
) -> io::Result<Output> {
    if cancellation.cancelled() {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "consulta cancelada",
        ));
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = out_tx.send(read_output(stdout));
    });
    std::thread::spawn(move || {
        let _ = err_tx.send(read_output(stderr));
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if cancellation.cancelled() || start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(io::Error::new(
                    if cancellation.cancelled() {
                        io::ErrorKind::Interrupted
                    } else {
                        io::ErrorKind::TimedOut
                    },
                    if cancellation.cancelled() {
                        "consulta cancelada"
                    } else {
                        "tempo limite da consulta excedido"
                    },
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(error);
            }
        }
    };
    // Descendants may inherit the pipes. Do not let them delay timeout/cancellation.
    let status = status?;
    let receive = |receiver: std::sync::mpsc::Receiver<io::Result<Vec<u8>>>| loop {
        if cancellation.cancelled() {
            break Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "consulta cancelada",
            ));
        }
        if start.elapsed() >= timeout {
            break Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "tempo limite da consulta excedido",
            ));
        }
        match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(result) => break result,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(_) => break Err(io::Error::other("falha ao ler saída")),
        }
    };
    let stdout = receive(out_rx)?;
    let stderr = receive(err_rx)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn timeout_and_cancellation_stop_the_child() {
        let mut slow = Command::new("ping");
        slow.args(["-n", "10", "127.0.0.1"]);
        let started = Instant::now();
        assert_eq!(
            output(
                &mut slow,
                &Cancellation::default(),
                Duration::from_millis(150)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(started.elapsed() < Duration::from_secs(5));
        let cancellation = Cancellation::default();
        let cancel_from_thread = cancellation.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            cancel_from_thread.cancel();
        });
        let mut slow = Command::new("ping");
        slow.args(["-n", "10", "127.0.0.1"]);
        assert_eq!(
            output(&mut slow, &cancellation, Duration::from_secs(5))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        canceller.join().unwrap();
        let cancellation = Cancellation::default();
        cancellation.cancel();
        assert_eq!(
            output(
                &mut Command::new("ping"),
                &cancellation,
                Duration::from_secs(1)
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::Interrupted
        );
    }
}
