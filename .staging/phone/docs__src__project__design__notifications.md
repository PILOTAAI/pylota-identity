Where this goes: `docs/src/project/design/notifications.md`. (1) Additions to the `needs_person` and `account` rows of "## 1. Kinds"; (2) a paragraph in "## 5. The emails". Source: [Phone numbers § 27](phone-numbers.md#27-console-and-notifications).

## 1. Kinds

- `needs_person` gains: pending phone requests (blocked ones counted apart), open port-outs, numbers with
  requirements pending, onboarding steps waiting for a person, numbers expiring within a day that received SMS
  in the last day, and `uncertain` phone requests.
- `account` gains the `AccountEvent` values `phone_port_out` (urgent, to owners, admins and the identity's
  accountable human, not subject to the daily cap), `phone_action_needed`, `phone_requirements_deadline` (72 and
  24 hours), `phone_compliance_expiring` (30 and 7 days), `phone_compliance_legacy` (60, 30 and 7 days before a
  legacy number's deadline), `phone_spend` (80% and 100% of a cap), `phone_request_uncertain` (after 24 hours),
  `phone_setup_action_needed` (at most once a day per country) and `phone_number_ready` (a number became able to
  do everything its country promises).

## 2. The emails

Phone emails carry no SMS text, no code and no counterparty number: a port-out names the workspace's own number
and the time left; an onboarding email names the country and the step.
