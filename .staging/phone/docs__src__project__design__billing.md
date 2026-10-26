Where this goes: `docs/src/project/design/billing.md`. (1) A paragraph at the end of "## What the Worker meters"; (2) a row in "### Hold"; (3) a paragraph in "## Stripe integration" after "### Stripe objects". Source: [Phone numbers § 18](phone-numbers.md#18-money).

## 1. What the Worker meters

**Phone numbers** are priced outside plan allowances, from a GBP rate card built from the provider's USD list
prices ([Phone numbers § 18.1](phone-numbers.md#181-prices-currency-and-the-rate-card)): each number's setup and
monthly price fixed in its quote, SMS per segment, voice per minute, and regulatory pass-through fees at cost.
Charges are rows of `phone_charges`, unique by source, so a repeated provider event charges once. Phone needs a
paid plan, or a partner that pays for its tenants.

## 2. Hold

| `phone_spend` | Pence | Every billable phone action, against the tenant's and the identity's monthly caps, or against the registration cap for one-off registration fees; a purchase covered by the standing budget also takes the month's budget slot (`phone_budget:t:{YYYY-MM}`) in the same transaction | Settled at the provider's charge; released on refusal; recurring monthly charges are never refused |

## 3. Stripe

On the 1st of each month the phone invoicing job creates one invoice item per tenant and category (numbers,
SMS, voice, registration fees, emergency) in GBP pence on the tenant's Stripe customer, with the idempotency key
`pi-phone-{tenant_id}-{YYYY-MM}-{category}`; partner tenants appear on the partner's monthly statement instead.
The restricted Stripe key gains invoice-item write.
