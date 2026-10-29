use pylota_identity::types::{Recipient, SendRequest};
use pylota_identity::{Client, IdempotencyKey};

pub async fn confirm_booking(key: String) -> Result<(), pylota_identity::Error> {
    let client = Client::builder()
        .base_url("https://mail.example.com")
        .api_key(key)
        .build()?;
    let request = SendRequest {
        to: vec![Recipient::from("renter@example.org")],
        subject: "Your booking BK-2291".into(),
        text: Some("Your car is ready at 9:00.".into()),
        ..Default::default()
    };
    let key = IdempotencyKey::new("bk-2291-confirm")?;
    // a retry with the same key returns this same message
    let sent = client
        .send_message("idn_01J9Z3K8V4", &request, &key)
        .await?;
    println!("{} deduplicated={}", sent.message.id, sent.deduplicated);
    Ok(())
}
