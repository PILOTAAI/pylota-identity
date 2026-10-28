//! Email Sending (`EMAIL`): structured mail with `worker`'s `SendEmailBuilder`, raw MIME as an
//! `EmailMessage` over a stream of the bytes (MIME need not be UTF-8). The binding's promise is raced
//! against the deadline.

use wasm_bindgen::JsCast as _;
use wasm_bindgen::JsValue;

use super::js;
use crate::email::{MailSender, SendError, StructuredEmail};

#[derive(Clone)]
pub struct CfMail {
    pub(crate) env: worker::Env,
}

const BINDING: &str = "EMAIL";

fn thrown(e: &JsValue) -> SendError {
    let message = js::message(e).chars().take(300).collect();
    match js::code(e) {
        Some(code) => SendError::Coded { code, message },
        None => SendError::Exception { message },
    }
}

impl CfMail {
    async fn send_js(&self, message: &JsValue, deadline_ms: u32) -> Result<String, SendError> {
        let binding = js::get(&self.env, BINDING).map_err(|e| thrown(&e))?;
        if binding.is_undefined() {
            return Err(SendError::Exception {
                message: "EMAIL is not bound".to_owned(),
            });
        }
        let promise: js_sys::Promise = js::call(&binding, "send", &[message])
            .map_err(|e| thrown(&e))?
            .dyn_into()
            .map_err(|e| thrown(&e))?;
        match js::with_deadline(promise, deadline_ms).await {
            Ok(Some(result)) => Ok(js::get(&result, "messageId")
                .ok()
                .and_then(|m| m.as_string())
                .unwrap_or_default()),
            Ok(None) => Err(SendError::Timeout),
            Err(e) => Err(thrown(&e)),
        }
    }
}

impl MailSender for CfMail {
    async fn send(&self, msg: &StructuredEmail, deadline_ms: u32) -> Result<String, SendError> {
        #[cfg(feature = "itest-hooks")]
        if let Some(r) = super::itest::mail_send(msg).await {
            return r;
        }
        let builder =
            worker::SendEmailBuilder::new_with_str_and_slice(&msg.from.0, &msg.to, &msg.subject);
        if !msg.from.1.is_empty() {
            builder
                .set_from_with_email_address(&worker::EmailAddress::new(&msg.from.1, &msg.from.0));
        }
        if !msg.cc.is_empty() {
            builder.set_cc_with_slice(&msg.cc);
        }
        if !msg.bcc.is_empty() {
            builder.set_bcc_with_slice(&msg.bcc);
        }
        if let Some(r) = &msg.reply_to {
            builder.set_reply_to(r);
        }
        if let Some(t) = &msg.text {
            builder.set_text(t);
        }
        if let Some(h) = &msg.html {
            builder.set_html(h);
        }
        if !msg.headers.is_empty() {
            let headers = js::object();
            for (name, value) in &msg.headers {
                js::set(&headers, name, &JsValue::from_str(value)).map_err(|e| thrown(&e))?;
            }
            builder.set_headers(headers.unchecked_ref());
        }
        if !msg.attachments.is_empty() {
            let attachments: Vec<worker::EmailAttachment> = msg
                .attachments
                .iter()
                .map(|a| {
                    let bytes = js_sys::Uint8Array::from(a.bytes.as_slice());
                    match &a.inline_content_id {
                        Some(cid) => worker::EmailAttachment::new_inline_with_typed_array(
                            cid,
                            &a.filename,
                            &a.content_type,
                            &bytes,
                        ),
                        None => worker::EmailAttachment::new_attachment_with_typed_array(
                            &a.filename,
                            &a.content_type,
                            &bytes,
                        ),
                    }
                })
                .collect();
            builder.set_attachments(&attachments);
        }
        self.send_js(builder.as_ref(), deadline_ms).await
    }

    async fn send_raw(
        &self,
        from: &str,
        to: &str,
        mime: &[u8],
        deadline_ms: u32,
    ) -> Result<String, SendError> {
        let response = js::construct("Response", &[&js_sys::Uint8Array::from(mime).into()])
            .map_err(|e| thrown(&e))?;
        let stream = js::get(&response, "body").map_err(|e| thrown(&e))?;
        let message =
            worker::email::EmailMessage::new_with_readable_stream(from, to, stream.unchecked_ref())
                .map_err(|e| thrown(&e.into()))?;
        self.send_js(message.as_ref(), deadline_ms).await
    }
}
