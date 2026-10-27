//! Spikes S8 and S11 (build plan M1): Amazon SES, SNS, S3 and SQS called from Rust in wasm, signed with
//! `core::ses::sigv4`, and SNS notifications verified with `core::sns`. The workflow `spikes-ses.yml` runs
//! this Worker in workerd (`wrangler dev --local`) with the credentials of the staging role, and drives it
//! with `run.sh`; see `RESULT.md`.
//!
//! Routes (all `POST`, JSON in and out):
//! - `/s8/send` `{from, to: [...], subject, pad_bytes?}`: SES v2 `SendEmail` with raw content.
//! - `/s8/verify` `{envelope, topic_arn, max_age_seconds}`: verify an SNS envelope (the `Body` of an SQS
//!   message), then a copy with one changed byte and a copy claiming `SignatureVersion` 1.
//! - `/s11/get` `{bucket, key}`: S3 `GetObject`.
//! - `/s11/receive` `{queue_url, delete}`: SQS `ReceiveMessage` (and `DeleteMessage`), the backstop read.
#![forbid(unsafe_code)]

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

/// A raw message from `from` to `to`, padded with a base64 attachment to about `pad_bytes` more bytes.
pub fn raw_message(
    from: &str,
    to: &[String],
    subject: &str,
    message_id: &str,
    pad_bytes: usize,
) -> Vec<u8> {
    let mut out = format!(
        "From: <{from}>\r\nTo: {}\r\nSubject: {subject}\r\nMessage-ID: {message_id}\r\n\
         Date: Tue, 27 Oct 2026 22:00:00 +0000\r\nMIME-Version: 1.0\r\n\
         Content-Type: multipart/mixed; boundary=\"s8\"\r\n\r\n\
         --s8\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nSpike S8/S11: {subject}\r\n",
        to.iter()
            .map(|t| format!("<{t}>"))
            .collect::<Vec<_>>()
            .join(", ")
    )
    .into_bytes();
    if pad_bytes > 0 {
        out.extend_from_slice(
            b"--s8\r\nContent-Type: application/octet-stream\r\n\
              Content-Disposition: attachment; filename=\"pad.bin\"\r\n\
              Content-Transfer-Encoding: base64\r\n\r\n",
        );
        // 57 raw bytes encode to one 76-character line; the bytes are a fixed pattern, not secret.
        let line = STANDARD.encode([0x5a_u8; 57]);
        for _ in 0..pad_bytes / (line.len() + 2) {
            out.extend_from_slice(line.as_bytes());
            out.extend_from_slice(b"\r\n");
        }
    }
    out.extend_from_slice(b"--s8--\r\n");
    out
}

/// The body of SES v2 `SendEmail` with raw content.
pub fn send_email_body(from: &str, to: &[String], raw: &[u8]) -> Value {
    json!({
        "FromEmailAddress": from,
        "Destination": { "ToAddresses": to },
        "Content": { "Raw": { "Data": STANDARD.encode(raw) } },
    })
}

/// Changes one byte of `Message`, as a tampered notification would.
pub fn tamper(envelope: &str) -> String {
    let mut v: Value = serde_json::from_str(envelope).unwrap_or(Value::Null);
    if let Some(m) = v["Message"].as_str() {
        let mut changed = m.to_owned();
        changed.push(' ');
        v["Message"] = Value::String(changed);
    }
    v.to_string()
}

#[cfg(target_arch = "wasm32")]
mod entry {
    use super::*;
    use pylota_identity_core::ses::{Credentials, Request, sha256_hex, sigv4};
    use pylota_identity_core::sns;
    use pylota_identity_platform::cf::glue::{
        CfPlatform, HttpRequestIn, HttpResponseOut, WorkerApp,
    };
    use pylota_identity_platform::cf::spike_s1::{http, now_ms};

    pub struct App {
        platform: CfPlatform,
    }

    struct Aws {
        key_id: String,
        secret: String,
        token: Option<String>,
        region: String,
    }

    impl App {
        fn aws(&self) -> Result<Aws, String> {
            let get = |n: &str| {
                self.platform
                    .var(n)
                    .ok_or_else(|| format!("{n} is not set"))
            };
            Ok(Aws {
                key_id: get("AWS_ACCESS_KEY_ID")?,
                secret: get("AWS_SECRET_ACCESS_KEY")?,
                token: self.platform.var("AWS_SESSION_TOKEN"),
                region: get("AWS_REGION")?,
            })
        }

        /// One signed AWS call. Returns the status, the `x-amzn-ErrorType` header if any, and the body.
        #[allow(clippy::too_many_arguments)]
        async fn call(
            &self,
            service: &str,
            method: &str,
            host: &str,
            path: &str,
            extra: &[(&str, &str)],
            body: Vec<u8>,
            content_sha256_header: bool,
        ) -> Result<(u16, Option<String>, Vec<u8>), String> {
            let aws = self.aws()?;
            let hash = sha256_hex(&body);
            let mut signed_headers: Vec<(&str, &str)> = extra.to_vec();
            if content_sha256_header {
                signed_headers.push(("x-amz-content-sha256", &hash));
            }
            let creds = Credentials {
                key_id: &aws.key_id,
                secret: &aws.secret,
                session_token: aws.token.as_deref(),
            };
            let req = Request {
                method,
                host,
                path,
                query: "",
                headers: &signed_headers,
                payload_sha256: &hash,
            };
            let signed = sigv4(
                &req,
                &creds,
                &aws.region,
                service,
                (now_ms() / 1000.0) as i64,
            );
            let mut headers: Vec<(String, String)> = signed_headers
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect();
            headers.push(("authorization".to_owned(), signed.authorization));
            headers.push(("x-amz-date".to_owned(), signed.x_amz_date));
            if let Some(token) = signed.security_token {
                headers.push(("x-amz-security-token".to_owned(), token));
            }
            let (status, response_headers, bytes) =
                http(method, &format!("https://{host}{path}"), &headers, body).await?;
            let error_type = response_headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("x-amzn-errortype"))
                .map(|(_, v)| v.clone());
            Ok((status, error_type, bytes))
        }

        async fn route(&self, path: &str, body: &Value) -> Result<Value, String> {
            let region = self.aws()?.region;
            match path {
                "/s8/send" => {
                    let from = body["from"].as_str().ok_or("from")?.to_owned();
                    let to: Vec<String> = body["to"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect();
                    let subject = body["subject"].as_str().unwrap_or("S8").to_owned();
                    let pad = body["pad_bytes"].as_u64().unwrap_or(0) as usize;
                    let message_id = format!(
                        "<s8-{}@{}>",
                        now_ms() as u64,
                        from.split('@').nth(1).unwrap_or("x")
                    );
                    let raw = raw_message(&from, &to, &subject, &message_id, pad);
                    let raw_len = raw.len();
                    let payload = send_email_body(&from, &to, &raw).to_string().into_bytes();
                    let (status, error_type, bytes) = self
                        .call(
                            "ses",
                            "POST",
                            &format!("email.{region}.amazonaws.com"),
                            "/v2/email/outbound-emails",
                            &[("content-type", "application/json")],
                            payload,
                            false,
                        )
                        .await?;
                    Ok(json!({
                        "status": status, "error_type": error_type, "raw_len": raw_len,
                        "composed_message_id": message_id,
                        "response": serde_json::from_slice::<Value>(&bytes)
                            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned())),
                    }))
                }
                "/s8/verify" => {
                    let envelope = body["envelope"].as_str().ok_or("envelope")?;
                    let topic = body["topic_arn"].as_str().ok_or("topic_arn")?;
                    let expect = sns::Expect {
                        region: &region,
                        topic_arn: topic,
                        max_age_seconds: body["max_age_seconds"].as_i64().unwrap_or(14 * 86_400),
                    };
                    let now = (now_ms() / 1000.0) as i64;
                    let parsed = sns::parse_envelope(envelope).map_err(|e| format!("{e:?}"))?;
                    let url = sns::precheck(&parsed, &expect, now)
                        .map_err(|e| format!("{e:?}"))?
                        .to_owned();
                    let (status, _, cert) = http("GET", &url, &[], Vec::new()).await?;
                    let pem = String::from_utf8_lossy(&cert).into_owned();
                    let genuine = sns::verify(&parsed, &expect, now, &pem);
                    let tampered = sns::parse_envelope(&tamper(envelope))
                        .and_then(|t| sns::verify(&t, &expect, now, &pem));
                    let mut v1 = parsed.clone();
                    v1.signature_version = "1".to_owned();
                    let version1 = sns::verify(&v1, &expect, now, &pem);
                    Ok(json!({
                        "cert_url": url, "cert_status": status,
                        "genuine": format!("{genuine:?}"), "tampered": format!("{tampered:?}"),
                        "version_1": format!("{version1:?}"),
                        "type": parsed.kind, "subject": parsed.subject,
                    }))
                }
                "/s11/get" => {
                    let bucket = body["bucket"].as_str().ok_or("bucket")?;
                    let key = body["key"].as_str().ok_or("key")?;
                    let (status, error_type, bytes) = self
                        .call(
                            "s3",
                            "GET",
                            &format!("{bucket}.s3.{region}.amazonaws.com"),
                            &format!("/{key}"),
                            &[],
                            Vec::new(),
                            true,
                        )
                        .await?;
                    let head =
                        String::from_utf8_lossy(&bytes[..bytes.len().min(4000)]).into_owned();
                    let pick = |name: &str| {
                        head.lines()
                            .find(|l| {
                                l.len() > name.len() && l[..name.len()].eq_ignore_ascii_case(name)
                            })
                            .map(str::to_owned)
                    };
                    Ok(json!({
                        "status": status, "error_type": error_type, "bytes": bytes.len(),
                        "sha256": sha256_hex(&bytes),
                        "subject": pick("Subject:"), "to": pick("To:"), "from": pick("From:"),
                        "x_failed": pick("X-Failed-Recipients:"),
                        "dsn_status": head.lines().filter(|l| l.starts_with("Status:") || l.starts_with("Diagnostic-Code:")).collect::<Vec<_>>(),
                    }))
                }
                "/s11/receive" => {
                    let queue_url = body["queue_url"].as_str().ok_or("queue_url")?;
                    let request = json!({ "QueueUrl": queue_url, "MaxNumberOfMessages": 10, "WaitTimeSeconds": 5 });
                    let host = format!("sqs.{region}.amazonaws.com");
                    let headers = [
                        ("content-type", "application/x-amz-json-1.0"),
                        ("x-amz-target", "AmazonSQS.ReceiveMessage"),
                    ];
                    let (status, error_type, bytes) = self
                        .call(
                            "sqs",
                            "POST",
                            &host,
                            "/",
                            &headers,
                            request.to_string().into_bytes(),
                            false,
                        )
                        .await?;
                    let reply: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                    let mut bodies = Vec::new();
                    for message in reply["Messages"].as_array().into_iter().flatten() {
                        bodies.push(message["Body"].clone());
                        if body["delete"] == true {
                            let delete = json!({ "QueueUrl": queue_url, "ReceiptHandle": message["ReceiptHandle"] });
                            let headers = [
                                ("content-type", "application/x-amz-json-1.0"),
                                ("x-amz-target", "AmazonSQS.DeleteMessage"),
                            ];
                            self.call(
                                "sqs",
                                "POST",
                                &host,
                                "/",
                                &headers,
                                delete.to_string().into_bytes(),
                                false,
                            )
                            .await?;
                        }
                    }
                    Ok(json!({ "status": status, "error_type": error_type, "bodies": bodies }))
                }
                other => Err(format!("no route {other}")),
            }
        }
    }

    impl WorkerApp for App {
        fn new(platform: CfPlatform) -> Self {
            App { platform }
        }

        async fn fetch(&self, req: HttpRequestIn) -> HttpResponseOut {
            let path = req
                .url
                .split("://")
                .nth(1)
                .and_then(|r| r.find('/').map(|i| r[i..].to_owned()));
            let path = path.unwrap_or_default();
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            let (status, value) = match self.route(&path, &body).await {
                Ok(v) => (200, v),
                Err(e) => (500, json!({ "error": e })),
            };
            HttpResponseOut::Http {
                status,
                headers: vec![("content-type".to_owned(), "application/json".to_owned())],
                body: value.to_string().into_bytes(),
            }
        }
    }

    pylota_identity_platform::export_worker! {
        app: crate::entry::App,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_message_pads_to_size() {
        let raw = raw_message(
            "a@x.example",
            &["b@x.example".into()],
            "s",
            "<m@x>",
            39 * 1024 * 1024,
        );
        assert!(raw.len() > 39 * 1024 * 1024 - 200 && raw.len() < 39 * 1024 * 1024 + 2000);
        assert!(raw.ends_with(b"--s8--\r\n"));
    }

    #[test]
    fn tamper_changes_the_message() {
        let t = tamper(r#"{"Message":"abc","Type":"Notification"}"#);
        assert!(t.contains("\"abc \""));
    }
}
