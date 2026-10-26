Where this goes: `docs/src/concepts.md`. A new section after "## Addresses". Source: [Phone numbers](project/design/phone-numbers.md).

## Phone numbers

A **phone number** is another address of an identity: it receives and sends SMS and carries calls to a route
the workspace chooses. It is registered in the workspace's own legal name, so before the first number in a
country the workspace **sets up the country once** (its business details and, in the UK, documents the provider
reviews; in the US, a texting registration). After that an agent gets a number with **one call**, inside a
**standing budget** a person set; outside it, the purchase becomes a **phone request** a person approves. Each
number says what it can do now in four **capability facets** (`outbound_sms`, `inbound_sms`, `inbound_calls`,
`outbound_calls`), each `enabled`, `activating`, `registration_required` or `not_available`, with the next step
and who must take it. Ports, releases, compliance submissions, registration fees, voice routes and port-outs are
always decided by people. See the [phone guide](guides/phone.md).
