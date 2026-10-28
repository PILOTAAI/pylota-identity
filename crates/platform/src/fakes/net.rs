//! The network: a recording HTTP client with scripted responses, a DNS zone map per resolver, and a
//! scripted SMTP peer behind `TcpConnect`.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use super::FakeClock;
use crate::dns::{Dns, DnsAnswer, DnsError, DnsRecord, DnsStatus, RType, Resolver};
use crate::http::{HttpClient, HttpError, HttpRequest, HttpResponse, take_capped};
use crate::tcp::{SocketError, TcpConn, TcpConnect, TlsMode, check_target};

// ---- HTTP ------------------------------------------------------------------------------------------

/// What the fake server answers for one URL.
#[derive(Clone, Debug)]
pub struct ScriptedResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// How long the server takes; past the request's `timeout_ms` the client gets `Timeout`.
    pub delay_ms: u32,
    /// The size of the chunks the body arrives in.
    pub chunk: usize,
    pub error: Option<HttpError>,
}

impl ScriptedResponse {
    pub fn ok(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body,
            delay_ms: 0,
            chunk: usize::MAX,
            error: None,
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::ok(Vec::new())
        }
    }

    pub fn error(error: HttpError) -> Self {
        Self {
            error: Some(error),
            ..Self::ok(Vec::new())
        }
    }

    pub fn json(body: &serde_json::Value) -> Self {
        Self::ok(body.to_string().into_bytes()).header("content-type", "application/json")
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }

    pub fn after_ms(mut self, ms: u32) -> Self {
        self.delay_ms = ms;
        self
    }

    pub fn chunked(mut self, size: usize) -> Self {
        self.chunk = size.max(1);
        self
    }
}

/// Records every request. A URL without a script fails as an unresolvable host (`HttpError::Dns`).
/// With a clock, each answered request advances it by the response's delay.
#[derive(Default)]
pub struct FakeHttp {
    scripts: RefCell<HashMap<String, ScriptedResponse>>,
    requests: RefCell<Vec<HttpRequest>>,
    clock: Option<FakeClock>,
}

impl FakeHttp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_clock(clock: FakeClock) -> Self {
        Self {
            clock: Some(clock),
            ..Self::default()
        }
    }

    /// Answers every request for exactly `url` with `response` until scripted again.
    pub fn script(&self, url: &str, response: ScriptedResponse) {
        self.scripts.borrow_mut().insert(url.to_owned(), response);
    }

    pub fn requests(&self) -> Vec<HttpRequest> {
        self.requests.borrow().clone()
    }
}

impl HttpClient for FakeHttp {
    async fn send(&self, req: HttpRequest) -> Result<HttpResponse, HttpError> {
        self.requests.borrow_mut().push(req.clone());
        let script = self
            .scripts
            .borrow()
            .get(&req.url)
            .cloned()
            .ok_or(HttpError::Dns)?;
        if script.delay_ms > req.timeout_ms {
            if let Some(c) = &self.clock {
                c.advance(i64::from(req.timeout_ms));
            }
            return Err(HttpError::Timeout);
        }
        if let Some(c) = &self.clock {
            c.advance(i64::from(script.delay_ms));
        }
        if let Some(e) = script.error {
            return Err(e);
        }
        let mut body = Vec::new();
        let mut body_truncated = false;
        for chunk in script
            .body
            .chunks(script.chunk.min(script.body.len().max(1)))
        {
            if take_capped(&mut body, chunk, req.max_body_bytes) {
                body_truncated = true;
                break; // the stream is cancelled
            }
        }
        Ok(HttpResponse {
            status: script.status,
            headers: script.headers,
            body,
            body_truncated,
        })
    }
}

// ---- DNS -------------------------------------------------------------------------------------------

type Zone = HashMap<(Resolver, String, RType), Result<DnsAnswer, DnsError>>;

/// A zone map per resolver (H1, H7: the two resolvers can disagree). An unknown name is NXDOMAIN.
#[derive(Default)]
pub struct FakeDns {
    zone: RefCell<Zone>,
    /// Answers with DNSSEC validation off (`cd=1`).
    cd_zone: RefCell<Zone>,
    queries: RefCell<Vec<(Resolver, String, RType)>>,
}

fn key(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

impl FakeDns {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records for `name` on one resolver, or both with `None`. TXT data is the concatenated text.
    pub fn set(&self, resolver: Option<Resolver>, name: &str, rtype: RType, data: &[&str]) {
        let answer = DnsAnswer {
            status: DnsStatus::NoError,
            ad: false,
            records: data
                .iter()
                .map(|d| DnsRecord {
                    name: key(name),
                    rtype,
                    ttl: 300,
                    data: (*d).to_owned(),
                })
                .collect(),
        };
        self.answer(resolver, name, rtype, Ok(answer));
    }

    /// Any answer or failure for `name` on one resolver, or both with `None`.
    pub fn answer(
        &self,
        resolver: Option<Resolver>,
        name: &str,
        rtype: RType,
        answer: Result<DnsAnswer, DnsError>,
    ) {
        let resolvers = match resolver {
            Some(r) => vec![r],
            None => vec![Resolver::First, Resolver::Second],
        };
        let mut zone = self.zone.borrow_mut();
        for r in resolvers {
            zone.insert((r, key(name), rtype), answer.clone());
        }
    }

    /// The answer to a `cd=1` query of `name` on both resolvers.
    pub fn set_cd(&self, name: &str, rtype: RType, answer: Result<DnsAnswer, DnsError>) {
        let mut zone = self.cd_zone.borrow_mut();
        for r in [Resolver::First, Resolver::Second] {
            zone.insert((r, key(name), rtype), answer.clone());
        }
    }

    pub fn remove(&self, name: &str, rtype: RType) {
        self.zone
            .borrow_mut()
            .retain(|(_, n, t), _| !(n == &key(name) && *t == rtype));
    }

    pub fn queries(&self) -> Vec<(Resolver, String, RType)> {
        self.queries.borrow().clone()
    }
}

impl Dns for FakeDns {
    /// Answers from the zone set with `set_cd`, else as without `cd`.
    async fn query_cd(&self, r: Resolver, name: &str, rtype: RType) -> Result<DnsAnswer, DnsError> {
        if let Some(a) = self.cd_zone.borrow().get(&(r, key(name), rtype)) {
            return a.clone();
        }
        self.query(r, name, rtype).await
    }

    async fn query(&self, r: Resolver, name: &str, rtype: RType) -> Result<DnsAnswer, DnsError> {
        self.queries.borrow_mut().push((r, key(name), rtype));
        self.zone
            .borrow()
            .get(&(r, key(name), rtype))
            .cloned()
            .unwrap_or(Ok(DnsAnswer {
                status: DnsStatus::NxDomain,
                ad: false,
                records: Vec::new(),
            }))
    }
}

// ---- TCP: a scripted SMTP peer ---------------------------------------------------------------------

/// The client bytes of each connection, by host and port.
type Transcripts = Rc<RefCell<Vec<((String, u16), Vec<u8>)>>>;

/// What the peer at one host and port says: its greeting, then one reply per client command (a line,
/// or a whole `DATA` body after a `354`). Replies are given without the final CRLF.
#[derive(Clone, Debug, Default)]
pub struct SmtpPeer {
    pub greeting: String,
    pub replies: Vec<String>,
    /// Fail the connect with this error instead.
    pub refuse: Option<SocketError>,
}

impl SmtpPeer {
    pub fn new(greeting: &str, replies: &[&str]) -> Self {
        Self {
            greeting: greeting.to_owned(),
            replies: replies.iter().map(|r| (*r).to_owned()).collect(),
            refuse: None,
        }
    }

    pub fn refusing(error: SocketError) -> Self {
        Self {
            refuse: Some(error),
            ..Self::default()
        }
    }
}

/// Connections to scripted peers; each connection's transcript (client bytes, and `-- TLS --` where
/// the upgrade happened) is kept by host and port.
#[derive(Default)]
pub struct FakeTcp {
    peers: RefCell<HashMap<(String, u16), SmtpPeer>>,
    transcripts: Transcripts,
}

impl FakeTcp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn peer(&self, host: &str, port: u16, peer: SmtpPeer) {
        self.peers
            .borrow_mut()
            .insert((host.to_owned(), port), peer);
    }

    /// What clients sent, per connection, in order.
    pub fn transcripts(&self) -> Vec<((String, u16), String)> {
        self.transcripts
            .borrow()
            .iter()
            .map(|(k, b)| (k.clone(), String::from_utf8_lossy(b).into_owned()))
            .collect()
    }
}

pub struct FakeConn {
    replies: VecDeque<String>,
    out: Vec<u8>,
    input: Vec<u8>,
    in_data: bool,
    tls: TlsMode,
    tls_started: bool,
    index: usize,
    transcripts: Transcripts,
}

impl FakeConn {
    fn reply(&mut self) {
        if let Some(r) = self.replies.pop_front() {
            self.in_data = r.starts_with("354");
            self.out.extend_from_slice(r.as_bytes());
            self.out.extend_from_slice(b"\r\n");
        }
    }

    fn log(&self, bytes: &[u8]) {
        self.transcripts.borrow_mut()[self.index]
            .1
            .extend_from_slice(bytes);
    }
}

impl TcpConnect for FakeTcp {
    type Conn = FakeConn;

    async fn connect(
        &self,
        host: &str,
        port: u16,
        tls: TlsMode,
        _timeout_ms: u32,
    ) -> Result<FakeConn, SocketError> {
        check_target(port)?;
        let peer = self
            .peers
            .borrow()
            .get(&(host.to_owned(), port))
            .cloned()
            .ok_or(SocketError::Connect)?;
        if let Some(e) = peer.refuse {
            return Err(e);
        }
        let mut transcripts = self.transcripts.borrow_mut();
        transcripts.push(((host.to_owned(), port), Vec::new()));
        let mut out = peer.greeting.into_bytes();
        out.extend_from_slice(b"\r\n");
        Ok(FakeConn {
            replies: peer.replies.into(),
            out,
            input: Vec::new(),
            in_data: false,
            tls,
            tls_started: false,
            index: transcripts.len() - 1,
            transcripts: self.transcripts.clone(),
        })
    }
}

impl TcpConn for FakeConn {
    async fn read(&mut self, buf: &mut [u8], _timeout_ms: u32) -> Result<usize, SocketError> {
        if self.out.is_empty() {
            return if self.replies.is_empty() {
                Ok(0)
            } else {
                Err(SocketError::Timeout) // the peer waits for a command
            };
        }
        let n = buf.len().min(self.out.len());
        buf[..n].copy_from_slice(&self.out[..n]);
        self.out.drain(..n);
        Ok(n)
    }

    async fn write_all(&mut self, bytes: &[u8], _timeout_ms: u32) -> Result<(), SocketError> {
        self.log(bytes);
        self.input.extend_from_slice(bytes);
        loop {
            if self.in_data {
                let Some(end) = self.input.windows(5).position(|w| w == b"\r\n.\r\n") else {
                    return Ok(());
                };
                self.input.drain(..end + 5);
                self.in_data = false;
                self.reply();
            } else {
                let Some(end) = self.input.windows(2).position(|w| w == b"\r\n") else {
                    return Ok(());
                };
                self.input.drain(..end + 2);
                self.reply();
            }
        }
    }

    async fn start_tls(mut self) -> Result<Self, SocketError> {
        if self.tls != TlsMode::StartTls || self.tls_started {
            return Err(SocketError::Tls);
        }
        self.tls_started = true;
        self.log(b"-- TLS --\r\n");
        Ok(self)
    }

    async fn close(self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fakes::block_on;

    #[test]
    fn dns_zone_per_resolver() {
        let dns = FakeDns::new();
        dns.set(None, "Example.TEST.", RType::Txt, &["v=spf1 -all"]);
        dns.set(
            Some(Resolver::Second),
            "example.test",
            RType::Txt,
            &["v=spf1 ~all"],
        );
        let a = block_on(dns.query(Resolver::First, "example.test", RType::Txt)).unwrap();
        let b = block_on(dns.query(Resolver::Second, "example.test", RType::Txt)).unwrap();
        assert_eq!(a.records[0].data, "v=spf1 -all");
        assert_eq!(b.records[0].data, "v=spf1 ~all");
        let nx = block_on(dns.query(Resolver::First, "missing.test", RType::Mx)).unwrap();
        assert_eq!(nx.status, DnsStatus::NxDomain);
        dns.answer(
            Some(Resolver::First),
            "slow.test",
            RType::Ns,
            Err(DnsError::Timeout),
        );
        assert_eq!(
            block_on(dns.query(Resolver::First, "slow.test", RType::Ns)),
            Err(DnsError::Timeout)
        );
        assert_eq!(dns.queries().len(), 4);
    }

    /// A STARTTLS session with the scripted peer, as the `smtp_relay` transport drives it.
    #[test]
    fn smtp_peer_session() {
        let tcp = FakeTcp::new();
        tcp.peer(
            "smtp.example.test",
            587,
            SmtpPeer::new(
                "220 smtp.example.test ESMTP",
                &[
                    "250-smtp.example.test\r\n250 STARTTLS",
                    "220 go ahead",
                    "250 ok",
                    "354 send",
                    "250 queued",
                    "221 bye",
                ],
            ),
        );
        block_on(async {
            let read = |c: &mut FakeConn| {
                let mut buf = [0u8; 512];
                let n = block_on(c.read(&mut buf, 1_000)).unwrap();
                String::from_utf8_lossy(&buf[..n]).into_owned()
            };
            let mut c = tcp
                .connect("smtp.example.test", 587, TlsMode::StartTls, 1_000)
                .await
                .unwrap();
            assert!(read(&mut c).starts_with("220 "));
            assert_eq!(c.read(&mut [0; 8], 1).await, Err(SocketError::Timeout));
            c.write_all(b"EHLO me\r\n", 1_000).await.unwrap();
            assert!(read(&mut c).contains("STARTTLS"));
            c.write_all(b"STARTTLS\r\n", 1_000).await.unwrap();
            assert!(read(&mut c).starts_with("220"));
            let mut c = c.start_tls().await.unwrap();
            c.write_all(b"EHLO me\r\n", 1_000).await.unwrap();
            read(&mut c);
            c.write_all(b"DATA\r\n", 1_000).await.unwrap();
            assert!(read(&mut c).starts_with("354"));
            c.write_all(b"Subject: x\r\n\r\nbody\r\n", 1_000)
                .await
                .unwrap();
            c.write_all(b".\r\n", 1_000).await.unwrap();
            assert!(read(&mut c).starts_with("250 queued"));
            assert!(c.start_tls().await.is_err(), "at most once");
        });
        let t = tcp.transcripts();
        assert!(t[0].1.contains("STARTTLS\r\n-- TLS --\r\nEHLO me"));
        assert_eq!(
            block_on(tcp.connect("smtp.example.test", 25, TlsMode::Implicit, 1)).err(),
            Some(SocketError::Other("port 25 is refused".into()))
        );
        assert_eq!(
            block_on(tcp.connect("other.test", 465, TlsMode::Implicit, 1)).err(),
            Some(SocketError::Connect)
        );
    }
}
