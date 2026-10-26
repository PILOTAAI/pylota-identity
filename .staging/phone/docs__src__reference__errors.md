Where this goes: `docs/src/reference/errors.md`, "## Catalogue". Rows go into the existing tables named before each block, in HTTP order within the table; the last block amends existing rows. Source: [Phone numbers § 22](../project/design/phone-numbers.md#22-failure-handling).

## Into "### Validation" (after the `upload_checksum_mismatch` row)

| 400 | <a id="code-phone_number_invalid"></a>`phone_number_invalid` | no | Not a valid E.164 phone number (`details.path`) |

## Into "### Authentication and access" (after the `invalid_signature` row)

| 403 | <a id="code-recipient_opted_out"></a>`recipient_opted_out` | no | The recipient opted out of SMS from this tenant (a STOP, or an opt-out recorded through the API or console). Do not text them again from this business ([Phone numbers](../guides/phone.md#receive-sms-and-handle-stop)) |
| 403 | <a id="code-consent_required"></a>`consent_required` | no | A `kind: "marketing"` SMS to a recipient without a marketing opt-in recorded with evidence |
| 403 | <a id="code-a2p_not_registered"></a>`a2p_not_registered` | no | An SMS from a US number whose 10DLC campaign is not fully provisioned for it, or whose toll-free verification is not `Verified`. `details.registration` (`10dlc` or `toll_free`) and `details.status` |
| 403 | <a id="code-destination_blocked"></a>`destination_blocked` | no | An SMS or call to an emergency, short-code or premium-rate number, which agent numbers never reach |

## Into "### Resources and state"

Add to the combined `404` row's list of codes: <a id="code-phone_number_not_found"></a>`phone_number_not_found`,
<a id="code-phone_request_not_found"></a>`phone_request_not_found`, <a id="code-sms_not_found"></a>`sms_not_found`,
<a id="code-call_not_found"></a>`call_not_found`, <a id="code-phone_document_not_found"></a>`phone_document_not_found`,
<a id="code-port_out_not_found"></a>`port_out_not_found`, <a id="code-voice_connector_not_found"></a>`voice_connector_not_found`.
A `from` number of another identity, a platform number and a released number all answer `phone_number_not_found`.

Then, after the `signup_human_unreachable` row:

| 409 | <a id="code-phone_number_not_ready"></a>`phone_number_not_ready` | yes while `configuring` | The number is not `active`, or lacks the capability asked for (`details.reason`: `status`, with `details.status`; `capability`, with the facet and its `next_step`; or `legacy_unpapered`, an adopted number past its re-papering deadline, which still receives) |
| 409 | <a id="code-phone_number_taken"></a>`phone_number_taken` | no | The exact number asked for was taken by someone else before the order. `details.alternatives[]` holds up to 5 fresh candidates, each with its quote and `quote_token`: pick one and buy again with a new `Idempotency-Key`. A purchase without an exact number never gets this: the service takes the next candidate |
| 409 | <a id="code-phone_request_not_pending"></a>`phone_request_not_pending` | no | Approve, reject or cancel of a phone request in a state that cannot be decided (`details.status`), including one that expired |
| 409 | <a id="code-phone_request_digest_mismatch"></a>`phone_request_digest_mismatch` | no | The approval names a digest that is not the request's current one (`details.current_digest`): the payload or quote changed. Read the request again and approve what it now says |
| 409 | <a id="code-compliance_not_approved"></a>`compliance_not_approved` | no | A port-in for a country, number type and action the tenant's compliance profile is not approved for (`details.country`, `details.number_type`, `details.action`, `details.status`). Purchases do not get this error: they wait on onboarding with `blocked_by` |
| 409 | <a id="code-quiet_hours"></a>`quiet_hours` | yes | A marketing SMS inside the destination country's quiet hours. `details.next_allowed_at`; `Retry-After` is set |
| 409 | <a id="code-identity_has_phone_numbers"></a>`identity_has_phone_numbers` | no | Deleting an identity that still holds a live phone number. Release or move its numbers first (`details.number_ids`) |
| 409 | <a id="code-voice_route_unsupported"></a>`voice_route_unsupported` | no | The number's voice route cannot do what was asked: an outbound call on a route other than `vapi`, or a route that cannot carry the disclosure (`details.reason`) |
| 409 | <a id="code-phone_document_in_use"></a>`phone_document_in_use` | no | Deleting a compliance document that a live provider requirement group still needs |
| 409 | <a id="code-port_in_not_cancelable"></a>`port_in_not_cancelable` | no | Cancelling a port-in within 48 hours of its FOC date |
| 409 | <a id="code-port_out_closed"></a>`port_out_closed` | no | Authorising or rejecting a port-out that is no longer pending (`details.status`) |

Then, after the `upload_expired` row:

| 410 | <a id="code-phone_quote_expired"></a>`phone_quote_expired` | no | A `quote_token` older than 30 minutes. Search again |

## Into "### Policy and limits"

After the `web_bot_auth_disabled` row:

| 422 | <a id="code-country_not_offered"></a>`country_not_offered` | no | The catalogue does not offer this country and number type (`details.country`, `details.number_type`) |
| 422 | <a id="code-local_presence_required"></a>`local_presence_required` | no | The row is offered only to a tenant with a verified address or establishment in the country (`details.local_presence`: `address_in_country`, `address_in_area` or `physical_presence`) |
| 422 | <a id="code-compliance_incomplete"></a>`compliance_incomplete` | no | The compliance profile lacks fields or documents the provider needs (`details.missing[]`, including `company_registry`), describes an entity the deployment refuses (`details.reason = "entity_type"`), names a sole proprietor's mobile outside the US and Canada (`otp_mobile_country`), or asks for a US texting registration for a business registered where it is not yet supported (`brand_country_not_supported`) |
| 422 | <a id="code-phone_registration_otp_invalid"></a>`phone_registration_otp_invalid` | no | The one-time code typed for a US sole-proprietor brand is wrong or expired. Ask for a new one (`…/otp/resend`, at most 5 a day) |
| 422 | <a id="code-phone_number_limit_reached"></a>`phone_number_limit_reached` | no | The identity or tenant already holds its limit of live numbers, counting pending purchases (`details.scope`, `details.limit`); or a US sole-proprietor registration already texts from its one number (`details.limit = "sole_proprietor_sms"`) |
| 422 | <a id="code-phone_request_limit_reached"></a>`phone_request_limit_reached` | no | The tenant already has 20 pending phone requests, or 10 running (`details.limit`, `details.status`) |

After the `billing_limit` row:

| 402 | <a id="code-spend_cap_reached"></a>`spend_cap_reached` | no | A monthly phone spend cap would be exceeded. Nothing was stored: ask a workspace owner to raise the cap, then retry with the same `Idempotency-Key`. `details.scope` (`tenant`, `identity`, or `registration` for one-off registration fees), `cap_pence`, `used_pence`, `resets_at` |

## Amendments to existing rows

- `403 invalid_signature`: add "or a request to `POST /hooks/telnyx` whose `telnyx-signature-ed25519` does not
  verify over `telnyx-timestamp|raw body`, or whose timestamp is more than 5 minutes from the server's clock
  (`details.reason = "timestamp"`)".
- `403 policy_denied`: add the `details.reason` values `phone_not_allowed` (the tenant's `phone.allowed` is
  false), `plan_required` (phone needs a paid plan or a partner), `destination_country` (an SMS or call to a
  country the policy does not allow), `destination_untested` and `destination_not_supported` (the sending
  number's country has not been shown to deliver to that destination, or is known not to), `calls_off`
  (`phone.calls.outbound` is `off`), and, on `GET …/sms/wait`, `account_not_approved` and
  `codes_into_fills_only`.
- `400 invalid_request`: add `details.reason = "location_not_supported"` (an area code or locality on a number
  type that has none, such as UK mobile numbers).
- `422 compliance_address_invalid` (from the phone design's address validation): add
  `details.reason = "emergency_address_country"` (an emergency address outside the number's country).
- `429 daily_cap_reached`: add the `details.cap` values `phone_search`, `sms_identity`, `sms_tenant` and
  `sms_tenant_month`.
- `502 upstream_error` and `503 unavailable`: add the `details.reason` values `provider` (the telephony
  provider), `phone_paused` (the operator paused phone purchases or sends), `provider_balance` (the provider
  account's balance is too low for new purchases) and, on `/hooks/telnyx`, `not_configured`.
- "## Send failures after `202`": add "SMS failures after `202` are statuses and reasons on the SMS
  (`sms.failed`, `reason`), never HTTP errors, as for mail: `opted_out`, `destination_not_allowed`,
  `a2p_not_registered`, `provider_spend_limit`, `unhealthy_sender`, `delivery_failed`, `sending_failed`."
