//! Outbound TCP: the Workers `connect()` API, used only by the `smtp_relay` transport. Implicit TLS on
//! 465, STARTTLS on 587, never plain text; port 25 is refused, and the host passes the SSRF rules,
//! before any connect. `start_tls` is called at most once, after the server advertises `STARTTLS`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TlsMode {
    /// Port 465.
    Implicit,
    /// Port 587.
    StartTls,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SocketError {
    Timeout,
    Connect,
    Tls,
    Closed,
    Other(String),
}

pub trait TcpConn: Sized {
    /// `0` once the peer has closed.
    async fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<usize, SocketError>;
    async fn write_all(&mut self, bytes: &[u8], timeout_ms: u32) -> Result<(), SocketError>;
    /// [`TlsMode::StartTls`] only, at most once.
    async fn start_tls(self) -> Result<Self, SocketError>;
    async fn close(self);
}

pub trait TcpConnect {
    type Conn: TcpConn;
    async fn connect(
        &self,
        host: &str,
        port: u16,
        tls: TlsMode,
        timeout_ms: u32,
    ) -> Result<Self::Conn, SocketError>;
}

/// The checks every implementation makes before connecting.
pub fn check_target(port: u16) -> Result<(), SocketError> {
    if port == 25 {
        return Err(SocketError::Other("port 25 is refused".to_owned()));
    }
    Ok(())
}
