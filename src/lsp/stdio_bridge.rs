//! A blocking-thread bridge for stdio, replacing `tokio::io::stdin/stdout`.
//!
//! The reader thread does plain blocking `read()`s on fd 0 and forwards chunks
//! over an mpsc channel; `poll_recv` delivers them to the async side with
//! correct waker semantics (no lost wakeups). The writer mirrors it: an
//! unbounded channel feeds a thread that writes + flushes fd 1 in order.

use std::io::{Read, Write};
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc;

const CHUNK: usize = 64 * 1024;

pub struct ChannelReader {
    rx: mpsc::Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

pub fn reader() -> ChannelReader {
    let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
    std::thread::Builder::new()
        .name("lsp-stdin".into())
        .spawn(move || {
            let mut stdin = std::io::stdin().lock();
            let mut buf = vec![0u8; CHUNK];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        if tx.blocking_send(buf[..n].to_vec()).is_err() {
                            break; // server gone
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        })
        .expect("spawn lsp-stdin thread");
    ChannelReader { rx, buf: Vec::new(), pos: 0 }
}

impl AsyncRead for ChannelReader {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let me = self.get_mut();
        if me.pos >= me.buf.len() {
            match me.rx.poll_recv(cx) {
                Poll::Ready(Some(chunk)) => {
                    me.buf = chunk;
                    me.pos = 0;
                }
                Poll::Ready(None) => return Poll::Ready(Ok(())), // EOF
                Poll::Pending => return Poll::Pending,
            }
        }
        let n = std::cmp::min(out.remaining(), me.buf.len() - me.pos);
        out.put_slice(&me.buf[me.pos..me.pos + n]);
        me.pos += n;
        Poll::Ready(Ok(()))
    }
}

pub struct ChannelWriter {
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

pub fn writer() -> ChannelWriter {
    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    std::thread::Builder::new()
        .name("lsp-stdout".into())
        .spawn(move || {
            let mut stdout = std::io::stdout().lock();
            while let Some(chunk) = rx.blocking_recv() {
                if stdout.write_all(&chunk).is_err() || stdout.flush().is_err() {
                    break;
                }
            }
        })
        .expect("spawn lsp-stdout thread");
    ChannelWriter { tx }
}

impl AsyncWrite for ChannelWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.tx.send(buf.to_vec()) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "stdout thread gone",
            ))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(())) // writer thread flushes each chunk
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
