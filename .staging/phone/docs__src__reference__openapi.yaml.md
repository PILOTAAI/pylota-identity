Where this goes: `docs/src/reference/openapi.yaml`. (1) Two tags in `tags:` after "Service accounts"; (2) the paths below at the end of `paths:` before `webhooks:` (the hook path beside `/hooks/ses`); (3) the parameters at the end of `components.parameters`; (4) the schemas at the end of `components.schemas`, under a `# ── Phone numbers ──` comment. Generated from [Phone numbers § 28](../project/design/phone-numbers.md#28-mcp-and-cli); the route table there and this file must agree, which `xtask openapi` checks once M38 lands.

## 1. Tags

```yaml
  - name: Phone numbers
    description: >-
      Catalogue, onboarding, the standing budget, one-call purchase, numbers, requests, compliance, registrations, voice connectors, port-outs, money ([Phone numbers](https://pylotamail.com/docs/project/design/phone-numbers.html)).
  - name: SMS and calls
    description: >-
      Sending and receiving SMS, waiting for codes, calls, consent.
```

## 2. Paths

```yaml
  /phone/countries:
    get:
      operationId: listPhoneCountries
      tags: [Phone numbers]
      summary: "List the phone catalogue"
      description: >-
        The rows the deployment offers, each with the tenant's own standing (`eligible`, or `reason`).
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/TenantIdQuery'
      responses:
        '200':
          description: "The rows."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneCountryList'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-setup:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: getPhoneSetup
      tags: [Phone numbers]
      summary: "Phone onboarding status, every country"
      description: >-
        One entry per country the tenant's policy allows: status, steps, fees, waits and the next step. Holds no personal data, so identity keys may read it.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The setup status per country."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneSetupList'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-setup/{country}:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
      - $ref: '#/components/parameters/CountryPath'
    get:
      operationId: getPhoneSetupCountry
      tags: [Phone numbers]
      summary: "Phone onboarding status, one country"
      description: >-
        As `getPhoneSetup` for one country.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The setup status."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneSetup'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-setup/{country}/draft:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
      - $ref: '#/components/parameters/CountryPath'
    put:
      operationId: putPhoneSetupDraft
      tags: [Phone numbers]
      summary: "Draft the text of a registration"
      description: >-
        An agent drafts the text a registration needs (descriptions, sample messages, opt-in flow, URLs). Never tax identifiers, personal data or documents: such fields are `400 invalid_request`. A person reviews and submits it in the console. Audit `phone_setup.draft`.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneSetupDraftRequest'}
      responses:
        '200':
          description: "The draft."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneSetupDraft'}
        '400':
          description: "The draft holds a field outside its schema, or a field breaks its length rule."
          x-error-codes: [invalid_request]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: getPhoneSetupDraft
      tags: [Phone numbers]
      summary: "Read the draft of a registration"
      description: >-
        The draft, if any.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The draft."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneSetupDraft'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    delete:
      operationId: deletePhoneSetupDraft
      tags: [Phone numbers]
      summary: "Delete the draft of a registration"
      description: >-
        Deletes the draft.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '204':
          description: "Deleted."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-budget:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: getPhoneBudget
      tags: [Phone numbers]
      summary: "Read the standing purchase budget"
      description: >-
        The budget and this month's use.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The budget."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneBudget'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    put:
      operationId: putPhoneBudget
      tags: [Phone numbers]
      summary: "Set the standing purchase budget"
      description: >-
        Replaces the budget. Tightening any field, or `enabled: false`, needs `phone:manage`. Setting it or loosening any field is a decision reserved for people: from an API key only a platform key or the tenant's partner key where `phone.key_approval` is `true`, with `on_behalf_of`. Emits `phone_budget.changed`; audit `phone_budget.set`.
      x-required-permission: ['phone:manage']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneBudgetRequest'}
      responses:
        '200':
          description: "The budget, with its new version."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneBudget'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403':
          description: "The key lacks `phone:manage`, or loosening needs a person (`permission_denied`, `details.reason = \"person_required\"`)."
          x-error-codes: [permission_denied, scope_denied, partner_suspended]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-numbers:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    post:
      operationId: buyPhoneNumber
      tags: [Phone numbers]
      summary: "Buy a phone number in one call"
      description: >-
        Creates a `purchase_number` request and, when the tenant's standing budget covers it and the country is onboarded, executes it at once. `201` with the number once the provider accepted the order; `202` with the request when a person must act (`details.outside_budget`), when it waits on onboarding (`blocked_by`), or when the run continues in the queue. A taken number is replaced by the next candidate when no exact number was asked for; an exact one answers `409 phone_number_taken` with alternatives.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneBuyRequest'}
      responses:
        '201':
          description: "The number."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneNumber'}
        '202':
          description: "The request, waiting on a person, on onboarding, or on the queue."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRequest'}
        '400':
          description: "Invalid fields, or a location filter on a type without one (`details.reason = \"location_not_supported\"`)."
          x-error-codes: [invalid_request, invalid_idempotency_key, idempotency_key_required]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '402':
          description: "A spend cap would be exceeded."
          x-error-codes: [spend_cap_reached]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "The exact number was taken (`details.alternatives`), or the idempotency key is in use."
          x-error-codes: [phone_number_taken, identity_paused, idempotency_conflict, request_in_progress]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: "The row is not offered, needs local presence, the entity type is refused, or a limit is reached."
          x-error-codes: [country_not_offered, local_presence_required, compliance_incomplete, phone_number_limit_reached, phone_request_limit_reached]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: listIdentityPhoneNumbers
      tags: [Phone numbers]
      summary: "List an identity's phone numbers"
      description: >-
        Live numbers by default; `status` filters.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
        - name: status
          in: query
          schema: {"$ref": "#/components/schemas/PhoneNumberStatus"}
      responses:
        '200':
          description: "A page of numbers."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneNumberPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-numbers/available:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    get:
      operationId: searchPhoneNumbers
      tags: [Phone numbers]
      summary: "Search numbers to buy"
      description: >-
        Candidates with every capability of the row, a GBP quote (with `follow_on`) and a 30-minute quote token. Nothing is reserved. 300 searches a day per tenant.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api, search]
      x-idempotency: none
      parameters:
        - name: country
          in: query
          required: true
          schema: {"type": "string", "pattern": "^[A-Z]{2}$"}
        - name: type
          in: query
          required: true
          schema: {"$ref": "#/components/schemas/PhoneNumberType"}
        - name: contains
          in: query
          schema: {"type": "string", "pattern": "^[0-9]{1,15}$"}
        - name: starts_with
          in: query
          schema: {"type": "string", "pattern": "^[0-9]{1,15}$"}
        - name: ends_with
          in: query
          schema: {"type": "string", "pattern": "^[0-9]{1,15}$"}
        - name: locality
          in: query
          schema: {"type": "string", "maxLength": 100}
        - name: region
          in: query
          schema: {"type": "string", "maxLength": 100}
        - name: limit
          in: query
          schema: {"type": "integer", "minimum": 1, "maximum": 20, "default": 10}
      responses:
        '200':
          description: "Candidates."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneCandidateList'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '422':
          description: "The row is not offered or needs local presence."
          x-error-codes: [country_not_offered, local_presence_required]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-numbers/portability:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    post:
      operationId: checkPhonePortability
      tags: [Phone numbers]
      summary: "Check whether numbers can be ported in"
      description: >-
        The provider's portability check. Free and read-only.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhonePortabilityRequest'}
      responses:
        '200':
          description: "One result per number."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhonePortabilityList'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-numbers/{number_id}:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/NumberIdPath'
    get:
      operationId: getPhoneNumber
      tags: [Phone numbers]
      summary: "Get a phone number"
      description: >-
        The number with its capability facets.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The number."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneNumber'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    patch:
      operationId: updatePhoneNumber
      tags: [Phone numbers]
      summary: "Update a phone number"
      description: >-
        `primary_for` needs `phone:manage`. An earlier `expires_at` is allowed to `phone:request`; a later one, or `null`, needs `phone:manage`.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-permission-notes: "`phone:manage` for `primary_for` and for extending or removing `expires_at`."
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneNumberUpdateRequest'}
      responses:
        '200':
          description: "The number."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneNumber'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-numbers/{number_id}/move:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/NumberIdPath'
    post:
      operationId: movePhoneNumber
      tags: [Phone numbers]
      summary: "Move a number to another identity"
      description: >-
        Within the tenant; no provider call; history stays with the old identity. Audit `phone_number.move`.
      x-required-permission: ['phone:manage']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneNumberMoveRequest'}
      responses:
        '200':
          description: "The number."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneNumber'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-numbers/{number_id}/voice-route:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/NumberIdPath'
    get:
      operationId: getPhoneVoiceRoute
      tags: [Phone numbers]
      summary: "Get a number's voice route"
      description: >-
        The route without secrets.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The route."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneVoiceRoute'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-numbers/{number_id}/port-outs:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/NumberIdPath'
    get:
      operationId: listPhonePortOuts
      tags: [Phone numbers]
      summary: "List a number's port-outs"
      description: >-
        Open first.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The port-outs."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhonePortOutList'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-numbers:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: listTenantPhoneNumbers
      tags: [Phone numbers]
      summary: "List a tenant's phone numbers"
      description: >-
        Every identity's numbers.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: "A page of numbers."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneNumberPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /phone-numbers/lookup:
    get:
      operationId: lookupPhoneNumber
      tags: [Phone numbers]
      summary: "Find the identity of a live number"
      description: >-
        A number of another tenant answers like a missing one.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - name: number
          in: query
          required: true
          schema: {"$ref": "#/components/schemas/E164"}
      responses:
        '200':
          description: "The identity and number IDs."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneNumberLookup'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-requests:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    post:
      operationId: createPhoneRequest
      tags: [Phone numbers]
      summary: "Create a phone request"
      description: >-
        A purchase, port-in, release, compliance submission, US registration, toll-free verification, emergency address, voice route or outbound call, for a person to approve. A `purchase_number` the standing budget covers is created approved and executes at once; one waiting on onboarding carries `blocked_by`.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-permission-notes: "Identity keys create only `purchase_number`, `port_in` (without secrets), `release_number` and `outbound_call`; the other kinds need `phone:compliance` or `phone:manage`."
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneRequestCreate'}
      responses:
        '201':
          description: "The request."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRequest'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '402':
          description: "A spend cap would be exceeded."
          x-error-codes: [spend_cap_reached]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "Port-in without an approved profile, or the key is in use."
          x-error-codes: [compliance_not_approved, identity_paused, idempotency_conflict, request_in_progress]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: "Catalogue, entity or limit refusals."
          x-error-codes: [country_not_offered, local_presence_required, compliance_incomplete, phone_number_limit_reached, phone_request_limit_reached]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: listIdentityPhoneRequests
      tags: [Phone numbers]
      summary: "List an identity's phone requests"
      description: >-
        Newest first.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
        - name: status
          in: query
          schema: {"$ref": "#/components/schemas/PhoneRequestStatus"}
        - name: kind
          in: query
          schema: {"$ref": "#/components/schemas/PhoneRequestKind"}
      responses:
        '200':
          description: "A page of requests."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRequestPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-requests/{request_id}:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/PhoneRequestIdPath'
    get:
      operationId: getPhoneRequest
      tags: [Phone numbers]
      summary: "Get a phone request"
      description: >-
        The request, its quote, digest and result.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The request."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-requests:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: listTenantPhoneRequests
      tags: [Phone numbers]
      summary: "List a tenant's phone requests"
      description: >-
        The approval queue, oldest pending first.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
        - name: status
          in: query
          schema: {"$ref": "#/components/schemas/PhoneRequestStatus"}
      responses:
        '200':
          description: "A page of requests."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRequestPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-requests/{request_id}/approve:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/PhoneRequestIdPath'
    post:
      operationId: approvePhoneRequest
      tags: [Phone numbers]
      summary: "Approve a phone request"
      description: >-
        A decision reserved for people: an API key may approve only where `phone.key_approval` is `true` and the key is a platform key or the tenant's partner key (with `on_behalf_of`), or where `PM_QUARANTINE_KEY_RELEASE` is `on` or `PM_CONSOLE` is `off`. The requesting key never approves (`self_approval`). The digest must be current. Emits `phone_request.approved`; audit `phone_request.approve`.
      x-required-permission: ['phone:approve']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneRequestApproveRequest'}
      responses:
        '200':
          description: "The request."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRequest'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "The request cannot be decided in its state, or the digest is not current."
          x-error-codes: [phone_request_not_pending, phone_request_digest_mismatch, idempotency_conflict]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-requests/{request_id}/reject:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/PhoneRequestIdPath'
    post:
      operationId: rejectPhoneRequest
      tags: [Phone numbers]
      summary: "Reject a phone request"
      description: >-
        Always allowed for a key with `phone:approve`. Emits `phone_request.rejected`.
      x-required-permission: ['phone:approve']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneRequestRejectRequest'}
      responses:
        '200':
          description: "The request."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRequest'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "The request cannot be decided in its state, or the digest is not current."
          x-error-codes: [phone_request_not_pending, phone_request_digest_mismatch, idempotency_conflict]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-requests/{request_id}/cancel:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/PhoneRequestIdPath'
    post:
      operationId: cancelPhoneRequest
      tags: [Phone numbers]
      summary: "Cancel a phone request"
      description: >-
        The requester, or a key with `phone:approve`, while `pending_approval` or `approved`. Emits `phone_request.canceled`.
      x-required-permission: ['phone:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      responses:
        '200':
          description: "The request."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRequest'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "The request cannot be decided in its state, or the digest is not current."
          x-error-codes: [phone_request_not_pending, phone_request_digest_mismatch, idempotency_conflict]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /platform/phone-requests/{request_id}/resolve:
    parameters:
      - $ref: '#/components/parameters/PhoneRequestIdPath'
    post:
      operationId: resolvePhoneRequest
      tags: [Phone numbers]
      summary: "Resolve an uncertain order"
      description: >-
        Ends an attempt left `uncertain` for 24 hours, after the operator checked the provider portal.
      x-required-permission: ['platform:ops']
      x-key-levels: [platform]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneRequestResolveRequest'}
      responses:
        '200':
          description: "The request."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRequest'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-compliance:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: getPhoneCompliance
      tags: [Phone numbers]
      summary: "Get the compliance profile"
      description: >-
        Sealed fields shown masked.
      x-required-permission: ['phone:compliance']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The profile."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneComplianceProfile'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    patch:
      operationId: updatePhoneCompliance
      tags: [Phone numbers]
      summary: "Update the compliance profile"
      description: >-
        Deep-merges; sealed fields are write-only. Audit `phone_compliance.update`.
      x-required-permission: ['phone:compliance']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneComplianceProfileUpdate'}
      responses:
        '200':
          description: "The profile."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneComplianceProfile'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '422':
          description: "A missing field, an entity type the deployment refuses, or a sole proprietor's mobile outside the US and Canada."
          x-error-codes: [compliance_incomplete]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-compliance/documents:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    post:
      operationId: createPhoneDocument
      tags: [Phone numbers]
      summary: "Upload a compliance document"
      description: >-
        `201` with the document and a one-hour `upload_url`; `PUT` the bytes there. Write-only.
      x-required-permission: ['phone:compliance']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneDocumentCreate'}
      responses:
        '201':
          description: "The document and its upload link."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneDocumentUpload'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: listPhoneDocuments
      tags: [Phone numbers]
      summary: "List compliance documents"
      description: >-
        Metadata only.
      x-required-permission: ['phone:compliance']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The documents."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneDocumentList'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-compliance/documents/{document_id}:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
      - $ref: '#/components/parameters/PhoneDocumentIdPath'
    get:
      operationId: getPhoneDocument
      tags: [Phone numbers]
      summary: "Get a compliance document's metadata"
      description: >-
        Never its bytes.
      x-required-permission: ['phone:compliance']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The document."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneDocument'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    delete:
      operationId: deletePhoneDocument
      tags: [Phone numbers]
      summary: "Delete a compliance document"
      description: >-
        Crypto-shred, then delete the object.
      x-required-permission: ['phone:compliance']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '204':
          description: "Deleted."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "A live provider group still needs it."
          x-error-codes: [phone_document_in_use]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /platform/phone-compliance/{profile_id}/export:
    parameters:
      - $ref: '#/components/parameters/PhoneProfileIdPath'
    post:
      operationId: exportPhoneCompliance
      tags: [Phone numbers]
      summary: "Export a compliance profile for a regulator"
      description: >-
        A single-use link valid for one hour; audited with the reason.
      x-required-permission: ['platform:ops']
      x-key-levels: [platform]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneExportRequest'}
      responses:
        '200':
          description: "The link."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneExportLink'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-registrations:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: getPhoneRegistrations
      tags: [Phone numbers]
      summary: "US registrations"
      description: >-
        Brand, campaign and number assignment status.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The registrations."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRegistrations'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-registrations/{registration_id}/otp:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
      - $ref: '#/components/parameters/PhoneRegistrationIdPath'
    post:
      operationId: verifyPhoneRegistrationOtp
      tags: [Phone numbers]
      summary: "Verify a sole-proprietor brand's code"
      description: >-
        A person types the code sent to the proprietor's own mobile. Never logged or stored. Audit `phone_registration.otp`.
      x-required-permission: ['phone:compliance']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneOtpRequest'}
      responses:
        '200':
          description: "The registration."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneRegistrations'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403':
          description: "Only a person decides this (`person_required`)."
          x-error-codes: [permission_denied, scope_denied]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404': {$ref: '#/components/responses/NotFound'}
        '422':
          description: "The code is wrong or expired."
          x-error-codes: [phone_registration_otp_invalid]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-registrations/{registration_id}/otp/resend:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
      - $ref: '#/components/parameters/PhoneRegistrationIdPath'
    post:
      operationId: resendPhoneRegistrationOtp
      tags: [Phone numbers]
      summary: "Send a new sole-proprietor code"
      description: >-
        At most 5 a day.
      x-required-permission: ['phone:compliance']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '202':
          description: "Sent."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-connectors:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    post:
      operationId: createPhoneConnector
      tags: [Phone numbers]
      summary: "Register a voice connector"
      description: >-
        A tenant's Vapi key, sealed and never shown again.
      x-required-permission: ['phone:manage']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneConnectorCreate'}
      responses:
        '201':
          description: "The connector."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneConnector'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: listPhoneConnectors
      tags: [Phone numbers]
      summary: "List voice connectors"
      description: >-
        Without keys.
      x-required-permission: ['phone:manage']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The connectors."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneConnectorList'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-connectors/{connector_id}:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
      - $ref: '#/components/parameters/PhoneConnectorIdPath'
    delete:
      operationId: deletePhoneConnector
      tags: [Phone numbers]
      summary: "Delete a voice connector"
      description: >-
        Routes using it fail with `voice_configuration`.
      x-required-permission: ['phone:manage']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '204':
          description: "Deleted."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-connectors/{connector_id}/rotate:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
      - $ref: '#/components/parameters/PhoneConnectorIdPath'
    post:
      operationId: rotatePhoneConnector
      tags: [Phone numbers]
      summary: "Rotate a voice connector's key"
      description: >-
        Replaces the sealed key.
      x-required-permission: ['phone:manage']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneConnectorRotate'}
      responses:
        '200':
          description: "The connector."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneConnector'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/sms:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    post:
      operationId: sendSms
      tags: [SMS and calls]
      summary: "Send an SMS"
      description: >-
        One send path: the ordered policy pipeline of the phone design. `202` with the SMS; a dry run answers `200` with `would_send`.
      x-required-permission: ['phone:send']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api, send]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeySms'
        - $ref: '#/components/parameters/DryRun'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/SmsSendRequest'}
      responses:
        '202':
          description: "The SMS, `queued`."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Sms'}
        '200':
          description: "A dry run's result."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/SmsDryRun'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '402':
          description: "A spend cap would be exceeded."
          x-error-codes: [spend_cap_reached]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: "Policy refusals."
          x-error-codes: [tenant_suspended, policy_denied, destination_blocked, recipient_opted_out, consent_required, a2p_not_registered]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "Not ready, quiet hours, or the key is in use."
          x-error-codes: [phone_number_not_ready, quiet_hours, identity_paused, idempotency_conflict, request_in_progress]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: listSms
      tags: [SMS and calls]
      summary: "List SMS"
      description: >-
        Newest first; held SMS only with `quarantine:review` and a `status` filter.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: "A page of SMS."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/SmsPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/sms/wait:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    get:
      operationId: waitSms
      tags: [SMS and calls]
      summary: "Wait for an SMS or a code"
      description: >-
        Long poll up to 60 seconds.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - name: number_id
          in: query
          schema: {"$ref": "#/components/schemas/PhoneNumberId"}
        - name: from
          in: query
          schema: {"type": "string", "maxLength": 32}
        - name: account_id
          in: query
          schema: {"type": "string"}
        - name: kind
          in: query
          schema: {"type": "string", "enum": ["any", "verification"], "default": "any"}
        - name: timeout
          in: query
          schema: {"type": "integer", "minimum": 1, "maximum": 60, "default": 30}
        - name: since
          in: query
          schema: {"type": "string", "format": "date-time"}
      responses:
        '200':
          description: "The SMS, a code where allowed, or `timed_out`."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/SmsWaitResult'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/sms/{sms_id}:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/SmsIdPath'
    get:
      operationId: getSms
      tags: [SMS and calls]
      summary: "Get an SMS"
      description: >-
        Codes appear as `[code]`.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The SMS."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Sms'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    delete:
      operationId: deleteSms
      tags: [SMS and calls]
      summary: "Erase an SMS"
      description: >-
        With its media and code row.
      x-required-permission: ['erasure:manage']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '204':
          description: "Erased."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/sms/{sms_id}/media/{n}:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/SmsIdPath'
      - $ref: '#/components/parameters/MediaIndexPath'
    get:
      operationId: getSmsMedia
      tags: [SMS and calls]
      summary: "Get an MMS item"
      description: >-
        Served as stored.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The bytes."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/octet-stream:
              schema: {type: string, format: binary}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/sms/{sms_id}/resolve:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/SmsIdPath'
    post:
      operationId: resolveSms
      tags: [SMS and calls]
      summary: "Resolve an uncertain SMS"
      description: >-
        `sent` or `not_sent`.
      x-required-permission: ['phone:send']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/SmsResolveRequest'}
      responses:
        '200':
          description: "The SMS."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Sms'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/sms/{sms_id}/release:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/SmsIdPath'
    post:
      operationId: releaseSms
      tags: [SMS and calls]
      summary: "Release a held SMS"
      description: >-
        Under the quarantine release rule.
      x-required-permission: ['quarantine:review']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The SMS."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Sms'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/sms-conversations:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    get:
      operationId: listSmsConversations
      tags: [SMS and calls]
      summary: "List SMS conversations"
      description: >-
        Per number and counterparty.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: "A page of conversations."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/SmsConversationPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/calls:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    post:
      operationId: placeCall
      tags: [SMS and calls]
      summary: "Place a call"
      description: >-
        Through a `vapi` route only; under `phone.calls.outbound = approval` the answer is the `outbound_call` request.
      x-required-permission: ['phone:call']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/CallCreateRequest'}
      responses:
        '202':
          description: "The call, or the request."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/CallOrRequest'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '402':
          description: "A spend cap would be exceeded."
          x-error-codes: [spend_cap_reached]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: "Refused destination or policy."
          x-error-codes: [destination_blocked, policy_denied]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "The route cannot place calls, or the key is in use."
          x-error-codes: [voice_route_unsupported, phone_number_not_ready, idempotency_conflict]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: listCalls
      tags: [SMS and calls]
      summary: "List calls"
      description: >-
        Newest first.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: "A page of calls."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/CallPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/calls/{call_id}:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/CallIdPath'
    get:
      operationId: getCall
      tags: [SMS and calls]
      summary: "Get a call"
      description: >-
        The call record.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The call."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Call'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    delete:
      operationId: deleteCall
      tags: [SMS and calls]
      summary: "Erase a call record"
      description: >-
        Deletes the row.
      x-required-permission: ['erasure:manage']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '204':
          description: "Erased."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-usage:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    get:
      operationId: getPhoneUsage
      tags: [SMS and calls]
      summary: "Phone usage this month"
      description: >-
        Counts exactly what the caps count.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "Usage per number."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneUsage'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-consent:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: getPhoneConsent
      tags: [SMS and calls]
      summary: "Read a consent entry"
      description: >-
        By number; masked hint only.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - name: number
          in: query
          required: true
          schema: {"$ref": "#/components/schemas/E164"}
      responses:
        '200':
          description: "The entry."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneConsent'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    post:
      operationId: setPhoneConsent
      tags: [SMS and calls]
      summary: "Record an opt-out or opt-in"
      description: >-
        `opted_out` needs `phone:send`; `opted_in` needs `phone:manage` and evidence.
      x-required-permission: ['phone:send']
      x-key-levels: [platform, partner, tenant, identity]
      x-permission-notes: "`opted_in` needs `phone:manage`."
      x-rate-limit-buckets: [api]
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneConsentRequest'}
      responses:
        '200':
          description: "The entry."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneConsent'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-numbers/{number_id}/port-outs/{port_out_id}/authorize:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/NumberIdPath'
      - $ref: '#/components/parameters/PortOutIdPath'
    post:
      operationId: authorizePhonePortOut
      tags: [Phone numbers]
      summary: "Authorize a port-out"
      description: >-
        A decision reserved for people; never automatic.
      x-required-permission: ['phone:approve']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      responses:
        '200':
          description: "The port-out."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhonePortOut'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "No longer pending."
          x-error-codes: [port_out_closed]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/phone-numbers/{number_id}/port-outs/{port_out_id}/reject:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
      - $ref: '#/components/parameters/NumberIdPath'
      - $ref: '#/components/parameters/PortOutIdPath'
    post:
      operationId: rejectPhonePortOut
      tags: [Phone numbers]
      summary: "Reject a port-out"
      description: >-
        A decision reserved for people; never automatic.
      x-required-permission: ['phone:approve']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhonePortOutDecision'}
      responses:
        '200':
          description: "The port-out."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhonePortOut'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '409':
          description: "No longer pending."
          x-error-codes: [port_out_closed]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone-charges:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: listPhoneCharges
      tags: [Phone numbers]
      summary: "Phone charges, caps and holds"
      description: >-
        For a month.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - name: month
          in: query
          schema: {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}$"}
      responses:
        '200':
          description: "The charges."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneCharges'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /partners/{partner_id}/phone-statement:
    parameters:
      - $ref: '#/components/parameters/PartnerIdPath'
    get:
      operationId: getPhoneStatement
      tags: [Phone numbers]
      summary: "A partner's phone statement"
      description: >-
        Charges per tenant and category.
      x-required-permission: ['phone:read']
      x-key-levels: [platform, partner]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - name: month
          in: query
          required: true
          schema: {"type": "string", "pattern": "^[0-9]{4}-[0-9]{2}$"}
      responses:
        '200':
          description: "The statement."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneStatement'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/phone/reconcile:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    post:
      operationId: reconcileTenantPhone
      tags: [Phone numbers]
      summary: "Reconcile a tenant's phone state now"
      description: >-
        At most once every 10 minutes.
      x-required-permission: ['phone:manage']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '202':
          description: "Started."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Job'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /platform/phone/reconcile:
    post:
      operationId: reconcilePlatformPhone
      tags: [Phone numbers]
      summary: "Run the nightly phone reconciler now"
      description: >-
        The job of the phone design § 17.
      x-required-permission: ['platform:ops']
      x-key-levels: [platform]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '202':
          description: "Started."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Job'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /platform/phone/probe:
    post:
      operationId: probePhoneAccount
      tags: [Phone numbers]
      summary: "Probe the provider account's level"
      description: >-
        Records `platform_state.phone_account`.
      x-required-permission: ['platform:ops']
      x-key-levels: [platform]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: "The result."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneAccountProbe'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /platform/phone-numbers/adopt:
    post:
      operationId: adoptPhoneNumber
      tags: [Phone numbers]
      summary: "Adopt a number already in the provider account"
      description: >-
        Stamps ownership; refuses a number already held.
      x-required-permission: ['platform:ops']
      x-key-levels: [platform]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/PhoneAdoptRequest'}
      responses:
        '201':
          description: "The number."
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/PhoneNumber'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /hooks/telnyx:
    servers:
      - url: https://{host}
        description: Deployment root (unversioned).
        variables:
          host:
            default: mail.example.com
            description: The deployment's API host (`PM_API_HOST`).
    post:
      operationId: receiveTelnyxEvent
      x-sdk: false
      tags: [Links and hooks]
      summary: "Telnyx events"
      description: >-
        No API key. Ed25519 over `telnyx-timestamp|raw body`, 5-minute window, in every environment. Writes the raw event to R2 and queues a pointer before answering; reads nothing from D1.
      security: []
      x-rate-limit-buckets: []
      x-idempotency: none
      requestBody:
        required: true
        content:
          application/json:
            schema: {type: object, description: 'A Telnyx event envelope, verified before it is parsed.'}
      responses:
        '200':
          description: "Accepted."
        '403':
          description: "A missing or bad signature, or a stale timestamp."
          x-error-codes: [invalid_signature]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '413':
          description: "Over 256 KB."
          x-error-codes: [payload_too_large]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '503':
          description: "Not configured, or staging failed (the provider retries)."
          x-error-codes: [unavailable]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}

```
## 3. Parameters (end of `components.parameters`)

```yaml
    IdempotencyKeyRequired:
      name: Idempotency-Key
      in: header
      required: true
      description: >-
        Required on every phone route that can cost money, move a number or decide a request. 1–255 printable
        ASCII characters; reuse the same key on every retry of the same request. Missing:
        `400 idempotency_key_required`; malformed: `400 invalid_idempotency_key`.
      schema: {type: string, minLength: 1, maxLength: 255}
    IdempotencyKeySms:
      name: Idempotency-Key
      in: header
      required: false
      description: Required on every real SMS send; optional on a dry run, where it is never recorded.
      schema: {type: string, minLength: 1, maxLength: 255}
    NumberIdPath:
      name: number_id
      in: path
      required: true
      schema: {$ref: '#/components/schemas/PhoneNumberId'}
    PhoneRequestIdPath:
      name: request_id
      in: path
      required: true
      schema: {type: string, pattern: '^phr_[0-9A-HJKMNP-TV-Z]{26}$'}
    CountryPath:
      name: country
      in: path
      required: true
      schema: {type: string, pattern: '^[A-Z]{2}$'}
    PhoneDocumentIdPath:
      name: document_id
      in: path
      required: true
      schema: {type: string, pattern: '^cpd_[0-9A-HJKMNP-TV-Z]{26}$'}
    PhoneProfileIdPath:
      name: profile_id
      in: path
      required: true
      schema: {type: string, pattern: '^cpp_[0-9A-HJKMNP-TV-Z]{26}$'}
    PhoneRegistrationIdPath:
      name: registration_id
      in: path
      required: true
      schema: {type: string, pattern: '^a2p_[0-9A-HJKMNP-TV-Z]{26}$'}
    PhoneConnectorIdPath:
      name: connector_id
      in: path
      required: true
      schema: {type: string, pattern: '^pvc_[0-9A-HJKMNP-TV-Z]{26}$'}
    SmsIdPath:
      name: sms_id
      in: path
      required: true
      schema: {type: string, pattern: '^sms_[0-9A-HJKMNP-TV-Z]{26}$'}
    MediaIndexPath:
      name: n
      in: path
      required: true
      schema: {type: integer, minimum: 0, maximum: 9}
    CallIdPath:
      name: call_id
      in: path
      required: true
      schema: {type: string, pattern: '^cal_[0-9A-HJKMNP-TV-Z]{26}$'}
    PortOutIdPath:
      name: port_out_id
      in: path
      required: true
      schema: {type: string, pattern: '^pto_[0-9A-HJKMNP-TV-Z]{26}$'}
```

## 4. Schemas (end of `components.schemas`)

```yaml
    # ── Phone numbers ────────────────────────────────────────────────────────────────────────────

    E164:
      type: string
      description: A phone number in E.164 form.
      pattern: '^\+[1-9][0-9]{6,14}$'
      examples: ['+447700900123']
    PhoneNumberId:
      type: string
      pattern: '^pnm_[0-9A-HJKMNP-TV-Z]{26}$'
    PhoneNumberType:
      type: string
      enum: [mobile, local, national, toll_free]
    PhoneNumberStatus:
      type: string
      enum: [ordering, uncertain, under_review, requirements_pending, configuring, active, suspended,
             port_out_pending, porting_in, releasing, released, ported_out, failed]
    NextStep:
      type: object
      required: [who, kind]
      properties:
        who: {type: string, enum: [person, operator, agent]}
        kind: {type: string, description: 'For example `register_us_a2p`, `enable_emergency`, `set_voice_route`, `submit_compliance`, `platform_prerequisite`.'}
        request_id: {type: [string, 'null']}
        console_url: {type: [string, 'null'], format: uri}
    Capability:
      type: object
      required: [state]
      properties:
        state: {type: string, enum: [enabled, activating, registration_required, not_available]}
        reason: {type: [string, 'null'], description: 'With `not_available`: for example `brand_country_not_supported`, `no_us_emergency_address`, `legacy_unpapered`, `not_on_row`.'}
        next_step: {oneOf: [{$ref: '#/components/schemas/NextStep'}, {type: 'null'}]}
    PhoneCapabilities:
      type: object
      required: [outbound_sms, inbound_sms, inbound_calls, outbound_calls]
      properties:
        outbound_sms: {$ref: '#/components/schemas/Capability'}
        inbound_sms: {$ref: '#/components/schemas/Capability'}
        inbound_calls: {$ref: '#/components/schemas/Capability'}
        outbound_calls: {$ref: '#/components/schemas/Capability'}
    FollowOn:
      type: object
      required: [capability, kind, who]
      properties:
        capability: {type: string, enum: [outbound_sms, inbound_sms, inbound_calls, outbound_calls]}
        kind: {type: string, enum: [register_us_a2p, verify_toll_free, enable_emergency, set_voice_route, not_available]}
        reason: {type: [string, 'null']}
        setup_pence: {type: [integer, 'null'], minimum: 0}
        monthly_pence: {type: [integer, 'null'], minimum: 0}
        typical_wait_days: {type: [array, 'null'], items: {type: integer}, minItems: 2, maxItems: 2}
        who: {type: string, enum: [person, operator]}
        once_per: {type: [string, 'null'], enum: [tenant, number, null]}
    PhoneQuote:
      type: object
      required: [currency, rate_card_version, valid_until]
      properties:
        currency: {type: string, const: GBP}
        setup_pence: {type: integer, minimum: 0}
        monthly_pence: {type: integer, minimum: 0}
        max_setup_pence: {type: integer, minimum: 0}
        max_monthly_pence: {type: integer, minimum: 0}
        usage: {type: [string, 'null']}
        follow_on: {type: array, items: {$ref: '#/components/schemas/FollowOn'}}
        expires_at: {type: [string, 'null'], format: date-time}
        months_charged_before_expiry: {type: [integer, 'null']}
        rate_card_version: {type: integer}
        valid_until: {type: string, format: date-time}
        provider: {type: [object, 'null'], description: 'Provider USD costs and the exchange rate; platform and partner keys only.'}
    PhoneCountry:
      type: object
      required: [country, number_type, status, capabilities, eligible]
      properties:
        country: {type: string}
        number_type: {$ref: '#/components/schemas/PhoneNumberType'}
        status: {type: string, enum: [available, requires_local_presence]}
        capabilities: {type: array, items: {type: string, enum: [sms, voice, mms_inbound]}}
        sms_use: {type: string, enum: [two_way, a2p_only, none]}
        local_presence: {type: string, enum: [none, address_in_country, address_in_area, physical_presence]}
        default_for_country: {type: boolean}
        destinations: {type: object, additionalProperties: {type: string, enum: [tested_ok, untested, not_supported]}}
        registration: {type: [object, 'null']}
        review_hours: {type: [integer, 'null']}
        emergency: {type: object}
        eligible: {type: boolean}
        reason: {type: [string, 'null'], enum: [local_presence_required, compliance_missing, not_allowed_by_policy, platform_prerequisite, entity_type, null]}
    PhoneCountryList:
      type: object
      required: [data]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/PhoneCountry'}}
    PhoneSetupStep:
      type: object
      required: [kind, status]
      properties:
        kind: {type: string, enum: [platform_prerequisite, profile, documents, submit_compliance, register_us_a2p, verify_toll_free, enable_emergency]}
        status: {type: string, enum: [not_started, draft, waiting_for_person, in_review, done, rejected, optional, not_applicable]}
        who: {type: [string, 'null'], enum: [person, operator, null]}
        missing: {type: array, items: {type: string}}
        request_id: {type: [string, 'null']}
        comments: {type: [string, 'null'], description: 'Provider comments on a rejection; untrusted text.'}
        fee: {type: [object, 'null'], properties: {setup_pence: {type: integer}, monthly_pence: {type: integer}}}
        typical_wait_days: {type: [array, 'null'], items: {type: integer}}
    PhoneSetup:
      type: object
      required: [country, status, steps]
      properties:
        country: {type: string}
        status: {type: string, enum: [not_started, in_review, action_needed, ready, unavailable]}
        reason: {type: [string, 'null']}
        steps: {type: array, items: {$ref: '#/components/schemas/PhoneSetupStep'}}
        after_setup: {type: object, additionalProperties: {type: string, enum: [enabled, activating, registration_required, not_available]}}
        next_step: {oneOf: [{$ref: '#/components/schemas/NextStep'}, {type: 'null'}]}
        draft_id: {type: [string, 'null']}
    PhoneSetupList:
      type: object
      required: [data]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/PhoneSetup'}}
    PhoneSetupDraftRequest:
      type: object
      additionalProperties: false
      properties:
        business_description: {type: string, minLength: 1, maxLength: 1000}
        use_case: {type: string, minLength: 1, maxLength: 1000}
        sample_messages: {type: array, minItems: 2, maxItems: 5, items: {type: string, minLength: 20, maxLength: 1024}}
        consent_flow: {type: string, minLength: 40, maxLength: 1000}
        website: {type: string, format: uri, maxLength: 1000}
        privacy_policy_url: {type: string, format: uri, maxLength: 1000}
        terms_url: {type: string, format: uri, maxLength: 1000}
        opt_in_image_url: {type: string, format: uri, maxLength: 1000}
    PhoneSetupDraft:
      allOf:
        - {$ref: '#/components/schemas/PhoneSetupDraftRequest'}
        - type: object
          required: [id, country, updated_at]
          properties:
            id: {type: string, pattern: '^pdr_[0-9A-HJKMNP-TV-Z]{26}$'}
            country: {type: string}
            drafted_by_key_id: {type: [string, 'null']}
            updated_at: {type: string, format: date-time}
    PhoneBudgetRequest:
      type: object
      required: [enabled]
      properties:
        enabled: {type: boolean}
        countries: {type: array, items: {type: string, pattern: '^[A-Z]{2}$'}}
        number_types: {type: array, items: {$ref: '#/components/schemas/PhoneNumberType'}}
        max_numbers_per_identity: {type: integer, minimum: 1, maximum: 20}
        max_new_numbers_per_month: {type: integer, minimum: 1, maximum: 200}
        max_setup_pence_per_number: {type: integer, minimum: 0}
        max_monthly_pence_per_number: {type: integer, minimum: 0}
        expiring_only: {type: boolean, default: false}
        on_behalf_of: {type: string, minLength: 1, maxLength: 128}
    PhoneBudget:
      allOf:
        - {$ref: '#/components/schemas/PhoneBudgetRequest'}
        - type: object
          required: [version]
          properties:
            version: {type: integer}
            set_by: {type: object}
            set_at: {type: [string, 'null'], format: date-time}
            this_month: {type: object, properties: {covered: {type: integer}, resets_at: {type: string, format: date-time}}}
    PhoneBuyRequest:
      type: object
      required: [country]
      properties:
        country: {type: string, pattern: '^[A-Z]{2}$'}
        type: {$ref: '#/components/schemas/PhoneNumberType'}
        area_code: {type: string, pattern: '^[0-9]{2,5}$'}
        contains: {type: string, pattern: '^[0-9]{1,15}$'}
        locality: {type: string, maxLength: 100}
        number: {$ref: '#/components/schemas/E164'}
        quote_token: {type: string}
        expires_at: {type: [string, 'null'], format: date-time}
        primary_for: {type: array, items: {type: string, enum: [sms, voice]}}
        acknowledge_not_available: {type: boolean, default: false}
        client_reference: {type: string, minLength: 1, maxLength: 128}
    PhoneNumber:
      type: object
      required: [id, identity_id, phone_number, country, number_type, status, setup_state, capabilities]
      properties:
        id: {$ref: '#/components/schemas/PhoneNumberId'}
        identity_id: {$ref: '#/components/schemas/IdentityId'}
        phone_number: {$ref: '#/components/schemas/E164'}
        country: {type: string}
        number_type: {$ref: '#/components/schemas/PhoneNumberType'}
        status: {$ref: '#/components/schemas/PhoneNumberStatus'}
        setup_state: {type: string, enum: [ready, carrier_pending, configuring_sms, configuring_voice, registration_pending]}
        capabilities: {$ref: '#/components/schemas/PhoneCapabilities'}
        primary_for: {type: array, items: {type: string, enum: [sms, voice]}}
        provisioning: {type: object, properties: {status: {type: string}, code: {type: [string, 'null']}, retryable: {type: boolean}}}
        requirements_status: {type: [string, 'null']}
        deadline_at: {type: [string, 'null'], format: date-time}
        action_needed: {type: [object, 'null']}
        expires_at: {type: [string, 'null'], format: date-time}
        port_pin: {type: string, enum: [set, missing]}
        compliance_status: {type: string, enum: [own, legacy_group]}
        legacy_deadline: {type: [string, 'null'], format: date-time}
        active_from: {type: [string, 'null'], format: date-time}
        active_until: {type: [string, 'null'], format: date-time}
        request_id: {type: [string, 'null']}
        client_reference: {type: [string, 'null']}
        provider: {type: [object, 'null'], description: 'Provider IDs; platform and partner keys only.'}
    PhoneNumberPage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/PhoneNumber'}}
        next_cursor: {type: [string, 'null']}
    PhoneCandidate:
      type: object
      required: [phone_number, features, quote, quote_token]
      properties:
        phone_number: {$ref: '#/components/schemas/E164'}
        locality: {type: [string, 'null']}
        region: {type: [string, 'null']}
        features: {type: array, items: {type: string}}
        quote: {$ref: '#/components/schemas/PhoneQuote'}
        quote_token: {type: string}
    PhoneCandidateList:
      type: object
      required: [data]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/PhoneCandidate'}}
    PhonePortabilityRequest:
      type: object
      required: [numbers]
      properties:
        numbers: {type: array, minItems: 1, maxItems: 50, items: {$ref: '#/components/schemas/E164'}}
    PhonePortabilityList:
      type: object
      required: [data]
      properties:
        data: {type: array, items: {type: object, properties: {phone_number: {$ref: '#/components/schemas/E164'}, portable: {type: boolean}, fast_portable: {type: boolean}, messaging_capable: {type: boolean}, reason: {type: [string, 'null']}}}}
    PhoneNumberUpdateRequest:
      type: object
      properties:
        primary_for: {type: array, items: {type: string, enum: [sms, voice]}}
        expires_at: {type: [string, 'null'], format: date-time}
    PhoneNumberMoveRequest:
      type: object
      required: [to_identity_id]
      properties:
        to_identity_id: {$ref: '#/components/schemas/IdentityId'}
    PhoneNumberLookup:
      type: object
      required: [identity_id, number_id]
      properties:
        identity_id: {$ref: '#/components/schemas/IdentityId'}
        number_id: {$ref: '#/components/schemas/PhoneNumberId'}
    PhoneVoiceRoute:
      type: object
      required: [kind, status]
      properties:
        kind: {type: string, enum: [vapi, sip, forward, none]}
        config: {type: object, description: 'Without secrets.'}
        status: {type: string, enum: [pending, ready, failed]}
        vapi_phone_number_id: {type: [string, 'null']}
    PhoneRequestKind:
      type: string
      enum: [purchase_number, port_in, release_number, submit_compliance, register_us_a2p, verify_toll_free, enable_emergency, set_voice_route, outbound_call]
    PhoneRequestStatus:
      type: string
      enum: [pending_approval, approved, executing, in_progress, action_needed, uncertain, completed, failed, rejected, canceled]
    PhoneRequestCreate:
      type: object
      required: [kind, payload]
      properties:
        kind: {$ref: '#/components/schemas/PhoneRequestKind'}
        payload: {type: object, description: 'Per kind; for `purchase_number`, `selection` (`mode` `exact` with `phone_number` and `quote_token`, or `any` with `country`, `type` and optional location) and optional `expires_at`.'}
        client_reference: {type: string, maxLength: 128}
    PhoneRequest:
      type: object
      required: [id, kind, status, digest, payload]
      properties:
        id: {type: string, pattern: '^phr_[0-9A-HJKMNP-TV-Z]{26}$'}
        identity_id: {$ref: '#/components/schemas/IdentityId'}
        kind: {$ref: '#/components/schemas/PhoneRequestKind'}
        status: {$ref: '#/components/schemas/PhoneRequestStatus'}
        reason: {type: [string, 'null']}
        payload: {type: object}
        quote: {oneOf: [{$ref: '#/components/schemas/PhoneQuote'}, {type: 'null'}]}
        digest: {type: string, pattern: '^[0-9a-f]{64}$'}
        blocked_by: {type: array, items: {type: string, enum: [compliance, platform_prerequisite]}}
        approved_by: {type: [string, 'null'], enum: [person, key, standing_budget, purchase, null]}
        budget_version: {type: [integer, 'null']}
        details: {type: object, properties: {outside_budget: {type: [string, 'null']}}}
        next_step: {oneOf: [{$ref: '#/components/schemas/NextStep'}, {type: 'null'}]}
        release_at: {type: [string, 'null'], format: date-time}
        decided_by_key_id: {type: [string, 'null']}
        decided_by_user_id: {type: [string, 'null']}
        on_behalf_of: {type: [string, 'null']}
        expires_at: {type: [string, 'null'], format: date-time}
        result: {type: [object, 'null']}
    PhoneRequestPage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/PhoneRequest'}}
        next_cursor: {type: [string, 'null']}
    PhoneRequestApproveRequest:
      type: object
      required: [digest]
      properties:
        digest: {type: string, pattern: '^[0-9a-f]{64}$'}
        note: {type: string, maxLength: 500}
        on_behalf_of: {type: string, minLength: 1, maxLength: 128}
    PhoneRequestRejectRequest:
      type: object
      required: [reason]
      properties:
        reason: {type: string, minLength: 1, maxLength: 500}
    PhoneRequestResolveRequest:
      type: object
      required: [outcome]
      properties:
        outcome: {type: string, enum: [ordered, not_ordered]}
        provider_order_id: {type: string}
    PhoneComplianceProfileUpdate:
      type: object
      properties:
        legal_name: {type: string, maxLength: 200}
        trading_name: {type: string, maxLength: 200}
        entity_type: {type: string, enum: [limited_company, llp, partnership, public_company, non_profit, sole_trader]}
        registration_country: {type: string, pattern: '^[A-Z]{2}$'}
        company_number: {type: string, maxLength: 64}
        company_registry: {type: object, properties: {country: {type: string}, region: {type: [string, 'null']}, name: {type: string}}}
        tax_ids: {type: object, writeOnly: true, additionalProperties: {type: string}}
        website: {type: string, format: uri}
        use_case: {type: string, maxLength: 1000}
        representative: {type: object, description: 'Name and role readable; email, phone and birth data write-only.'}
        addresses: {type: array, maxItems: 20, items: {type: object}}
    PhoneComplianceProfile:
      allOf:
        - {$ref: '#/components/schemas/PhoneComplianceProfileUpdate'}
        - type: object
          required: [id]
          properties:
            id: {type: string, pattern: '^cpp_[0-9A-HJKMNP-TV-Z]{26}$'}
            status: {type: array, items: {type: object, properties: {country: {type: string}, number_type: {type: string}, action: {type: string}, status: {type: string}, expires_at: {type: [string, 'null'], format: date-time}}}}
    PhoneDocumentCreate:
      type: object
      required: [kind, filename, content_type, size, sha256]
      properties:
        kind: {type: string, enum: [company_registration_certificate, proof_of_address, representative_id, registration_form, power_of_attorney, letter_of_authorisation, invoice, other]}
        filename: {type: string, maxLength: 255}
        content_type: {type: string, enum: [application/pdf, image/png, image/jpeg]}
        size: {type: integer, minimum: 1, maximum: 20971520}
        sha256: {type: string, pattern: '^[0-9a-f]{64}$'}
        valid_until: {type: [string, 'null'], format: date}
    PhoneDocument:
      type: object
      required: [id, kind, status]
      properties:
        id: {type: string, pattern: '^cpd_[0-9A-HJKMNP-TV-Z]{26}$'}
        kind: {type: string}
        filename: {type: string}
        content_type: {type: string}
        size: {type: integer}
        status: {type: string, enum: [pending_upload, ready, deleted]}
        valid_until: {type: [string, 'null'], format: date}
    PhoneDocumentUpload:
      allOf:
        - {$ref: '#/components/schemas/PhoneDocument'}
        - type: object
          required: [upload_url]
          properties:
            upload_url: {type: string, format: uri}
    PhoneDocumentList:
      type: object
      required: [data]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/PhoneDocument'}}
    PhoneExportRequest:
      type: object
      required: [reason]
      properties:
        reason: {type: string, minLength: 1, maxLength: 500}
    PhoneExportLink:
      type: object
      required: [url, expires_at]
      properties:
        url: {type: string, format: uri}
        expires_at: {type: string, format: date-time}
    PhoneRegistrations:
      type: object
      properties:
        configured: {type: boolean}
        brand_entity: {type: [string, 'null'], enum: [standard, sole_proprietor, null]}
        brand_status: {type: [string, 'null']}
        campaign_status: {type: [string, 'null']}
        otp_status: {type: [string, 'null'], enum: [not_required, sent, verified, expired, null]}
        numbers_registered: {type: array, items: {$ref: '#/components/schemas/PhoneNumberId'}}
        failure_reasons: {type: array, items: {type: string}}
    PhoneOtpRequest:
      type: object
      required: [pin]
      properties:
        pin: {type: string, pattern: '^[0-9]{4,8}$', writeOnly: true}
    PhoneConnectorCreate:
      type: object
      required: [kind, region, api_key]
      properties:
        kind: {type: string, const: vapi}
        region: {type: string, enum: [us, eu]}
        api_key: {type: string, writeOnly: true}
    PhoneConnectorRotate:
      type: object
      required: [api_key]
      properties:
        api_key: {type: string, writeOnly: true}
    PhoneConnector:
      type: object
      required: [id, kind, region, status]
      properties:
        id: {type: string, pattern: '^pvc_[0-9A-HJKMNP-TV-Z]{26}$'}
        kind: {type: string}
        region: {type: string}
        status: {type: string, enum: [active, failed, deleted]}
    PhoneConnectorList:
      type: object
      required: [data]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/PhoneConnector'}}
    PhonePortOut:
      type: object
      required: [id, number_id, status, respond_by]
      properties:
        id: {type: string, pattern: '^pto_[0-9A-HJKMNP-TV-Z]{26}$'}
        number_id: {$ref: '#/components/schemas/PhoneNumberId'}
        status: {type: string}
        respond_by: {type: string, format: date-time}
        decision: {type: [string, 'null'], enum: [authorized, rejected, provider_authorized, null]}
    PhonePortOutList:
      type: object
      required: [data]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/PhonePortOut'}}
    PhonePortOutDecision:
      type: object
      required: [reason_code]
      properties:
        reason_code: {type: string}
        reason: {type: string, maxLength: 500}
    SmsSendRequest:
      type: object
      required: [to, text]
      properties:
        from: {type: string, description: 'A number ID or E.164 of this identity.'}
        to: {$ref: '#/components/schemas/E164'}
        text: {type: string, minLength: 1}
        kind: {type: string, enum: [transactional, marketing, verification], default: transactional}
        conversation_id: {type: [string, 'null']}
        client_reference: {type: string, maxLength: 128}
    Sms:
      type: object
      required: [id, number_id, direction, status]
      properties:
        id: {type: string, pattern: '^sms_[0-9A-HJKMNP-TV-Z]{26}$'}
        conversation_id: {type: string}
        number_id: {$ref: '#/components/schemas/PhoneNumberId'}
        direction: {type: string, enum: [inbound, outbound]}
        status: {type: string}
        from: {type: string}
        to: {type: string}
        text: {type: [string, 'null'], description: 'Untrusted; codes appear as `[code]`.'}
        segments: {type: [integer, 'null']}
        flags: {type: array, items: {type: string}}
        reason: {type: [string, 'null']}
        client_reference: {type: [string, 'null']}
        received_at: {type: [string, 'null'], format: date-time}
        sent_at: {type: [string, 'null'], format: date-time}
    SmsDryRun:
      type: object
      required: [would_send]
      properties:
        would_send: {type: boolean}
        segments: {type: integer}
        price_pence: {type: integer}
    SmsPage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/Sms'}}
        next_cursor: {type: [string, 'null']}
    SmsConversationPage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {type: object, properties: {id: {type: string}, number_id: {type: string}, counterparty: {type: string}, last_message_at: {type: string, format: date-time}}}}
        next_cursor: {type: [string, 'null']}
    SmsWaitResult:
      type: object
      required: [timed_out]
      properties:
        sms: {oneOf: [{$ref: '#/components/schemas/Sms'}, {type: 'null'}]}
        verification: {type: [object, 'null'], properties: {kind: {type: string}, value: {type: string}, account_id: {type: [string, 'null']}}}
        timed_out: {type: boolean}
    SmsResolveRequest:
      type: object
      required: [outcome]
      properties:
        outcome: {type: string, enum: [sent, not_sent]}
    CallCreateRequest:
      type: object
      required: [to, purpose]
      properties:
        from: {type: string}
        to: {$ref: '#/components/schemas/E164'}
        purpose: {type: string, minLength: 1, maxLength: 500}
        assistant: {type: object, properties: {assistant_id: {type: string}, variables: {type: object}}}
    Call:
      type: object
      required: [id, number_id, direction]
      properties:
        id: {type: string, pattern: '^cal_[0-9A-HJKMNP-TV-Z]{26}$'}
        number_id: {$ref: '#/components/schemas/PhoneNumberId'}
        direction: {type: string, enum: [inbound, outbound]}
        counterparty: {type: string}
        route_kind: {type: [string, 'null']}
        status: {type: string}
        outcome: {type: [string, 'null']}
        disclosure_applied: {type: boolean}
        started_at: {type: [string, 'null'], format: date-time}
        ended_at: {type: [string, 'null'], format: date-time}
        duration_seconds: {type: [integer, 'null']}
        cost_pence: {type: [integer, 'null']}
        source: {type: string, enum: [webhook, cdr]}
    CallOrRequest:
      oneOf:
        - {$ref: '#/components/schemas/Call'}
        - {$ref: '#/components/schemas/PhoneRequest'}
    CallPage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/Call'}}
        next_cursor: {type: [string, 'null']}
    PhoneUsage:
      type: object
      properties:
        month: {type: string}
        numbers: {type: array, items: {type: object, properties: {number_id: {type: string}, sms_out_segments: {type: integer}, sms_in_segments: {type: integer}, calls: {type: integer}, voice_minutes: {type: integer}}}}
    PhoneConsentRequest:
      type: object
      required: [number, state]
      properties:
        number: {$ref: '#/components/schemas/E164'}
        state: {type: string, enum: [opted_out, opted_in]}
        purpose: {type: string, enum: [marketing, all]}
        evidence: {type: string, maxLength: 1000}
    PhoneConsent:
      type: object
      properties:
        number_hint: {type: string}
        state: {type: [string, 'null'], enum: [opted_out, opted_in, null]}
        purpose: {type: [string, 'null']}
        source: {type: [string, 'null']}
        updated_at: {type: [string, 'null'], format: date-time}
    PhoneCharges:
      type: object
      properties:
        month: {type: string}
        charges: {type: array, items: {type: object, properties: {id: {type: string}, kind: {type: string}, pence: {type: integer}, number_id: {type: [string, 'null']}, created_at: {type: string, format: date-time}}}}
        caps: {type: object, properties: {tenant_monthly_cap_pence: {type: [integer, 'null']}, identity_monthly_cap_pence: {type: [integer, 'null']}, registration_cap_pence: {type: [integer, 'null']}, used_pence: {type: integer}, registration_used_pence: {type: integer}}}
        holds_pence: {type: integer}
    PhoneStatement:
      type: object
      properties:
        partner_id: {type: string}
        month: {type: string}
        tenants: {type: array, items: {type: object, properties: {tenant_id: {type: string}, categories: {type: object, additionalProperties: {type: integer}}}}}
    PhoneAccountProbe:
      type: object
      required: [level, probed_at]
      properties:
        level: {type: string, enum: [paid, verified, unknown]}
        probed_at: {type: string, format: date-time}
        us_enabled: {type: boolean}
    PhoneAdoptRequest:
      type: object
      required: [identity_id, phone_number]
      properties:
        identity_id: {$ref: '#/components/schemas/IdentityId'}
        phone_number: {$ref: '#/components/schemas/E164'}
        vapi_phone_number_id: {type: string}
```
