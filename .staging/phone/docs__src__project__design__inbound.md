Where this goes: `docs/src/project/design/inbound.md`. One paragraph at the end of "### Verification codes and unsolicited OTP (E5)". Source: [Phone numbers § 11](phone-numbers.md#11-verification-codes-over-sms).

SMS uses the same trigger keywords and code patterns (`core::phone::verify::find_code` calls the shared
patterns of `core::classify::find_verification`) without the link rule: a link in an SMS is never a verification
object. Solicitation, the ledger gate, vault withholding and redaction for SMS are specified in
[Phone numbers § 11](phone-numbers.md#11-verification-codes-over-sms).
