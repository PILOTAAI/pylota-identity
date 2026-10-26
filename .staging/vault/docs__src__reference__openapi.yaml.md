Where this goes: `docs/src/reference/openapi.yaml`. (1) The tag below into `tags:` after `Service accounts`; (2) the paths into `paths:` after the service-account paths (after the phone paths if pasted first); (3) the components into `components:` under `securitySchemes`, `parameters` and `schemas`. Error codes are those of `errors.md`; every route is in the router table. Source: [Vault](../project/design/vault.md).

## 1. Tag

```yaml
  - name: Vault
    description: >-
      Logins, API keys and browser sessions that agents use without seeing them: items, grants, uses,
      tickets, consumers, custody and the vault audit (FR-VLT). Values are write-only. The consumer routes
      take no API key: they are authenticated by an RFC 9421 signature (`vaultConsumerSignature`).
```

## 2. Paths

```yaml
  # ──────────────────────────────────────── Vault ───────────────────────────────────────

  /identities/{identity_id}/vault/items:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    post:
      operationId: createIdentityVaultItem
      tags: [Vault]
      summary: Create a n identity-scoped vault item
      description: >-
        Stores a login, API key or session for exact https origins bound to an approved service-ledger entry.
        Values are write-only and never returned. Sealing needs only the tenant's public key, so no unlock key is
        needed in partner custody. Emits `vault.item.created`; audit `item.created`.
      x-required-permission: ['vault:write']
      x-key-levels: [platform, partner, tenant, identity]
      x-permission-notes: 'An identity key creates items only for its own identity.'
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultItemCreateRequest'}
      responses:
        '201':
          description: The item, without values.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultItemCreated'}
        '400':
          description: >-
            Invalid body or origins (`details.errors[].path`), or a JWK or value of the wrong shape.
          x-error-codes: [invalid_request, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            The identity is paused, or the idempotency key is in use.
          x-error-codes: [identity_paused, idempotency_conflict, request_in_progress]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '413': {$ref: '#/components/responses/PayloadTooLarge'}
        '422':
          description: >-
            The vault is off for this tenant; a refused kind, value or domain; a missing or unapproved ledger entry (`ledger_entry_not_approved`) or an origin outside it (`origin_not_in_ledger`); a limit.
          x-error-codes: [vault_disabled, vault_kind_refused, vault_item_limit_reached, vault_session_too_large, invalid_request]
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
      operationId: listIdentityVaultItems
      tags: [Vault]
      summary: List an identity's vault items
      description: >-
        Newest first. The identity list includes tenant-scoped items with an active grant for the identity.
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - {name: kind, in: query, required: false, description: 'Only items of this kind.', schema: {$ref: '#/components/schemas/VaultItemKind'}}
        - {name: state, in: query, required: false, description: '`active` or `archived`.', schema: {type: string}}
        - {name: health, in: query, required: false, description: 'Only items with this health.', schema: {type: string}}
        - {name: account_id, in: query, required: false, description: 'Only items of this ledger entry.', schema: {type: string}}
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: A page of items.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultItemPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '410': {$ref: '#/components/responses/CursorExpired'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/vault/items:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    post:
      operationId: createTenantVaultItem
      tags: [Vault]
      summary: Create a  tenant-scoped vault item
      description: >-
        Stores a login, API key or session for exact https origins bound to an approved service-ledger entry.
        Values are write-only and never returned. Sealing needs only the tenant's public key, so no unlock key is
        needed in partner custody. Emits `vault.item.created`; audit `item.created`.
      x-required-permission: ['vault:write']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultItemCreateRequest'}
      responses:
        '201':
          description: The item, without values.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultItemCreated'}
        '400':
          description: >-
            Invalid body or origins (`details.errors[].path`), or a JWK or value of the wrong shape.
          x-error-codes: [invalid_request, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            The identity is paused, or the idempotency key is in use.
          x-error-codes: [identity_paused, idempotency_conflict, request_in_progress]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '413': {$ref: '#/components/responses/PayloadTooLarge'}
        '422':
          description: >-
            The vault is off for this tenant; a refused kind, value or domain; a missing or unapproved ledger entry (`ledger_entry_not_approved`) or an origin outside it (`origin_not_in_ledger`); a limit.
          x-error-codes: [vault_disabled, vault_kind_refused, vault_item_limit_reached, vault_session_too_large, invalid_request]
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
      operationId: listTenantVaultItems
      tags: [Vault]
      summary: List a tenant's vault items
      description: >-
        Every item of the tenant, newest first.
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - {name: kind, in: query, required: false, description: 'Only items of this kind.', schema: {$ref: '#/components/schemas/VaultItemKind'}}
        - {name: state, in: query, required: false, description: '`active` or `archived`.', schema: {type: string}}
        - {name: health, in: query, required: false, description: 'Only items with this health.', schema: {type: string}}
        - {name: account_id, in: query, required: false, description: 'Only items of this ledger entry.', schema: {type: string}}
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: A page of items.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultItemPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '410': {$ref: '#/components/responses/CursorExpired'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/items/{item_id}:
    parameters:
      - $ref: '#/components/parameters/VaultItemIdPath'
    get:
      operationId: getVaultItem
      tags: [Vault]
      summary: Read a vault item
      description: >-
        Metadata only; never a value.
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: The item.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultItem'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    patch:
      operationId: updateVaultItem
      tags: [Vault]
      summary: Change a vault item's metadata
      description: >-
        Changes `title`, `origins`, `approval`, `state`, `otp_channel` and `rotation`. An added origin widens no
        grant; a removed one leaves grants with the rest. A looser `approval` is a decision reserved for people.
        Values change only through a new version. Emits `vault.item.updated`.
      x-required-permission: ['vault:write']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultItemUpdateRequest'}
      responses:
        '200':
          description: The item.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultItem'}
        '400':
          description: >-
            Invalid field.
          x-error-codes: [invalid_request, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: >-
            Origin outside the ledger entry, a refused domain, or a managed item.
          x-error-codes: [invalid_request, vault_kind_refused, vault_disabled]
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
    delete:
      operationId: deleteVaultItem
      tags: [Vault]
      summary: Crypto-shred a vault item
      description: >-
        Deletes every version (each data key and ciphertext), the item's grants, and closes its uses, in one
        batch; the R2 objects are deleted after the commit and retried daily. Emits `vault.item.deleted` with the
        receipt.
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      responses:
        '200':
          description: The shred receipt.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultShredReceipt'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/items/{item_id}/versions:
    parameters:
      - $ref: '#/components/parameters/VaultItemIdPath'
    post:
      operationId: rotateVaultItem
      tags: [Vault]
      summary: Write a new version of a vault item
      description: >-
        The kind's values and `overlap_hours`. Fields not given are carried over, which needs the unlock key in
        partner custody. Open uses close (`item_rotated`). Emits `vault.item.rotated`.
      x-required-permission: ['vault:write']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
        - $ref: '#/components/parameters/VaultUnlockHeader'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultItemVersionRequest'}
      responses:
        '201':
          description: The item with its new current version.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultItem'}
        '400':
          description: >-
            Invalid values, or the unlock header where it is not accepted.
          x-error-codes: [invalid_request, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: >-
            Unlock key missing or invalid in partner custody, or the key lacks the permission.
          x-error-codes: [vault_unlock_required, vault_unlock_invalid, permission_denied, scope_denied]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: >-
            A refused value, a managed item, or limits.
          x-error-codes: [vault_kind_refused, vault_session_too_large, invalid_request, vault_disabled]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /identities/{identity_id}/vault/grants:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    post:
      operationId: requestVaultGrant
      tags: [Vault]
      summary: Ask for a vault grant
      description: >-
        Records that the identity wants to use an item for some operations at some of its origins; a person
        decides. With `approve: true`, a caller holding `vault:approve` that may decide creates it active
        (`on_behalf_of` required from an API key). Emits `vault.grant.requested`.
      x-required-permission: ['vault:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-permission-notes: 'An identity key requests only for its own identity; `approve: true` needs `vault:approve`, which identity keys cannot hold.'
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultGrantRequest'}
      responses:
        '201':
          description: The grant.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultGrant'}
        '400':
          description: >-
            Invalid operations or origins, or `on_behalf_of` missing.
          x-error-codes: [invalid_request, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: >-
            No approved ledger entry of this identity, or a decision reserved for people.
          x-error-codes: [vault_not_granted, permission_denied, scope_denied, tenant_suspended, partner_suspended]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            A covering grant exists, or the identity is paused.
          x-error-codes: [vault_grant_exists, identity_paused, idempotency_conflict, request_in_progress]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: >-
            The vault is off, or the grant limits are reached.
          x-error-codes: [vault_disabled, vault_grant_limit_reached]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: listIdentityVaultGrants
      tags: [Vault]
      summary: List an identity's vault grants
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - {name: status, in: query, required: false, description: 'Only grants with this status.', schema: {type: string}}
        - {name: item_id, in: query, required: false, description: 'Only grants on this item.', schema: {type: string}}
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: A page of grants.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultGrantPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '410': {$ref: '#/components/responses/CursorExpired'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/vault/grants:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: listTenantVaultGrants
      tags: [Vault]
      summary: List a tenant's vault grants (the approval queue)
      description: >-
        Oldest pending first by default.
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - {name: status, in: query, required: false, description: 'Only grants with this status (default `pending_approval`).', schema: {type: string}}
        - {name: item_id, in: query, required: false, description: 'Only grants on this item.', schema: {type: string}}
        - {name: identity_id, in: query, required: false, description: 'Only grants of this identity.', schema: {type: string}}
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: A page of grants.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultGrantPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '410': {$ref: '#/components/responses/CursorExpired'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/grants/{grant_id}:
    parameters:
      - $ref: '#/components/parameters/VaultGrantIdPath'
    get:
      operationId: getVaultGrant
      tags: [Vault]
      summary: Read a vault grant
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: The grant.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultGrant'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/grants/{grant_id}/approve:
    parameters:
      - $ref: '#/components/parameters/VaultGrantIdPath'
    post:
      operationId: approveVaultGrant
      tags: [Vault]
      summary: Approve a vault grant
      description: >-
        A decision reserved for people (`Decision::ApproveVault`); the requesting key never approves; the ledger
        entry is re-checked. The same decision again answers `200` and writes nothing. Emits
        `vault.grant.approved`.
      x-required-permission: ['vault:approve']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultGrantDecision'}
      responses:
        '200':
          description: The grant.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultGrant'}
        '400':
          description: >-
            Invalid body, `on_behalf_of` missing for an API key that may decide, or a missing idempotency key.
          x-error-codes: [invalid_request, idempotency_key_required, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            Not pending, or the idempotency key is in use.
          x-error-codes: [vault_not_pending, idempotency_conflict, request_in_progress]
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

  /vault/grants/{grant_id}/deny:
    parameters:
      - $ref: '#/components/parameters/VaultGrantIdPath'
    post:
      operationId: denyVaultGrant
      tags: [Vault]
      summary: Deny a vault grant
      description: >-
        Always allowed for a key holding `vault:approve`. Emits `vault.grant.denied`.
      x-required-permission: ['vault:approve']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultGrantDecision'}
      responses:
        '200':
          description: The grant.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultGrant'}
        '400':
          description: >-
            Invalid body, `on_behalf_of` missing for an API key that may decide, or a missing idempotency key.
          x-error-codes: [invalid_request, idempotency_key_required, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            Not pending, or the idempotency key is in use.
          x-error-codes: [vault_not_pending, idempotency_conflict]
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

  /vault/grants/{grant_id}/revoke:
    parameters:
      - $ref: '#/components/parameters/VaultGrantIdPath'
    post:
      operationId: revokeVaultGrant
      tags: [Vault]
      summary: Revoke a vault grant
      description: >-
        `vault:approve`, or `vault:request` from a key of the grantee identity. Closes open uses
        (`grant_revoked`). Revoking an ended grant answers `200` unchanged. Emits `vault.grant.revoked`.
      x-required-permission: ['vault:approve']
      x-key-levels: [platform, partner, tenant, identity]
      x-permission-notes: 'An identity key may revoke only its own identity''s grants, with `vault:request`.'
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultGrantDecision'}
      responses:
        '200':
          description: The grant.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultGrant'}
        '400':
          description: >-
            Invalid body, `on_behalf_of` missing for an API key that may decide, or a missing idempotency key.
          x-error-codes: [invalid_request, idempotency_key_required, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            Not pending, or the idempotency key is in use.
          x-error-codes: [idempotency_conflict]
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

  /identities/{identity_id}/vault/uses:
    parameters:
      - $ref: '#/components/parameters/IdentityIdPath'
    post:
      operationId: createVaultUse
      tags: [Vault]
      summary: Ask to use a vault item now
      description: >-
        The orchestrator's request from a deterministic trigger. When no person must decide, the answer carries a
        one-use ticket to redeem within 60 seconds; a replay never returns it (`ticket_replayed: false`).
        `operation: sign` (P1) takes `vault:request`, may come from an identity key, and answers with the
        signature. Minting in partner custody needs the unlock key.
      x-required-permission: ['vault:use']
      x-key-levels: [platform, partner, tenant]
      x-permission-notes: '`vault:use` cannot be held by identity keys; `operation: sign` needs `vault:request` instead and admits identity keys for their own identity.'
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
        - $ref: '#/components/parameters/VaultUnlockHeader'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultUseCreateRequest'}
      responses:
        '201':
          description: The use, and a ticket when approved at once.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultUseCreated'}
        '400':
          description: >-
            Invalid operation, phases, trigger or origin; `sms_not_available`; missing idempotency key.
          x-error-codes: [invalid_request, idempotency_key_required, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: >-
            No covering grant, the vault refuses this use now, the consumer is not active, or an unlock key is needed.
          x-error-codes: [vault_not_granted, vault_refused, consumer_not_active, vault_unlock_required, vault_unlock_invalid, permission_denied, scope_denied, tenant_suspended, partner_suspended]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404':
          description: >-
            Item or consumer not found.
          x-error-codes: [vault_item_not_found, vault_consumer_not_found, identity_not_found]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            The identity is paused, or the idempotency key is in use.
          x-error-codes: [identity_paused, idempotency_conflict, request_in_progress]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: >-
            The vault is off.
          x-error-codes: [vault_disabled]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429':
          description: >-
            `RL_VAULT`, or 100 uses waiting for approval (`details.bucket = "vault_pending"`).
          x-error-codes: [rate_limited]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    get:
      operationId: listIdentityVaultUses
      tags: [Vault]
      summary: List an identity's vault uses
      description: >-
        Never shows a ticket, its hash or a sealed key.
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - {name: status, in: query, required: false, description: 'Only uses with this status.', schema: {type: string}}
        - {name: item_id, in: query, required: false, description: 'Only uses of this item.', schema: {type: string}}
        - {name: run_id, in: query, required: false, description: 'Only uses of this run.', schema: {type: string}}
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: A page of uses.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultUsePage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '410': {$ref: '#/components/responses/CursorExpired'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/vault/uses:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: listTenantVaultUses
      tags: [Vault]
      summary: List a tenant's vault uses (the approval queue)
      description: >-
        Default `status=pending_approval`, oldest first.
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - {name: status, in: query, required: false, description: 'Only uses with this status.', schema: {type: string}}
        - {name: item_id, in: query, required: false, description: 'Only uses of this item.', schema: {type: string}}
        - {name: identity_id, in: query, required: false, description: 'Only uses of this identity.', schema: {type: string}}
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: A page of uses.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultUsePage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '410': {$ref: '#/components/responses/CursorExpired'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/uses/{use_id}:
    parameters:
      - $ref: '#/components/parameters/VaultUseIdPath'
    get:
      operationId: getVaultUse
      tags: [Vault]
      summary: Read a vault use
      description: >-
        Never shows its ticket.
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: The use.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultUse'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/uses/{use_id}/approve:
    parameters:
      - $ref: '#/components/parameters/VaultUseIdPath'
    post:
      operationId: approveVaultUse
      tags: [Vault]
      summary: Approve a vault use
      description: >-
        Bound to the use's digest; a decision reserved for people; the creating key never approves; must be before
        `deadline_at`. Does not return a ticket. Emits `vault.use.approved`.
      x-required-permission: ['vault:approve']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultUseApproveRequest'}
      responses:
        '200':
          description: The use.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultUse'}
        '400':
          description: >-
            Invalid body, a digest that is not the use's (path `digest`), or a missing idempotency key.
          x-error-codes: [invalid_request, idempotency_key_required, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            Not in a state that can be decided, or the idempotency key is in use.
          x-error-codes: [vault_not_pending, idempotency_conflict, request_in_progress]
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

  /vault/uses/{use_id}/deny:
    parameters:
      - $ref: '#/components/parameters/VaultUseIdPath'
    post:
      operationId: denyVaultUse
      tags: [Vault]
      summary: Deny a vault use
      description: >-
        Emits `vault.use.denied`.
      x-required-permission: ['vault:approve']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: required
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyRequired'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultGrantDecision'}
      responses:
        '200':
          description: The use.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultUse'}
        '400':
          description: >-
            Invalid body, a digest that is not the use's (path `digest`), or a missing idempotency key.
          x-error-codes: [invalid_request, idempotency_key_required, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            Not in a state that can be decided, or the idempotency key is in use.
          x-error-codes: [vault_not_pending, idempotency_conflict]
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

  /vault/uses/{use_id}/cancel:
    parameters:
      - $ref: '#/components/parameters/VaultUseIdPath'
    post:
      operationId: cancelVaultUse
      tags: [Vault]
      summary: Cancel a vault use
      description: >-
        A pending or approved use becomes `canceled`; an `in_use` one closes (`canceled`).
      x-required-permission: ['vault:use']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      responses:
        '200':
          description: The use.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultUse'}
        '400':
          description: >-
            Invalid body, a digest that is not the use's (path `digest`), or a missing idempotency key.
          x-error-codes: [invalid_request, idempotency_key_required, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            Not in a state that can be decided, or the idempotency key is in use.
          x-error-codes: [vault_not_pending, idempotency_conflict]
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

  /vault/uses/{use_id}/ticket:
    parameters:
      - $ref: '#/components/parameters/VaultUseIdPath'
    post:
      operationId: mintVaultTicket
      tags: [Vault]
      summary: Mint a ticket for an approved vault use
      description: >-
        Allowed while the use is approved and no phase has been redeemed; replaces any earlier ticket, which stops
        matching at once. Re-checks grant, ledger, item, consumer, identity and tenant. Not recorded for
        idempotency: each call mints a new secret.
      x-required-permission: ['vault:use']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/VaultUnlockHeader'
      responses:
        '200':
          description: The ticket, shown only here.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultTicket'}
        '403':
          description: >-
            The grant or ledger no longer allows it, the consumer is not active, or an unlock key is needed.
          x-error-codes: [vault_not_granted, vault_refused, consumer_not_active, vault_unlock_required, vault_unlock_invalid, permission_denied, scope_denied]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            The use is not approved, or a phase was redeemed.
          x-error-codes: [vault_not_pending, vault_phase_spent]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: >-
            The vault or redemption is off.
          x-error-codes: [vault_disabled]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/uses/{use_id}/signature:
    parameters:
      - $ref: '#/components/parameters/VaultUseIdPath'
    post:
      operationId: getVaultSignature
      tags: [Vault]
      summary: Get the signature of an approved sign use (P1)
      description: >-
        Once; the use then closes. In partner custody needs the unlock key.
      x-required-permission: ['vault:request']
      x-key-levels: [platform, partner, tenant, identity]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/VaultUnlockHeader'
      responses:
        '200':
          description: The signature.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultSignature'}
        '409':
          description: >-
            Not approved, or already answered.
          x-error-codes: [vault_not_pending, vault_phase_spent]
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

  /vault/consumers:
    post:
      operationId: registerVaultConsumer
      tags: [Vault]
      summary: Register a vault consumer
      description: >-
        Created `pending_activation`. Private keys are generated where they are used; a JWK with a private member
        is refused.
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultConsumerCreateRequest'}
      responses:
        '201':
          description: The consumer.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumer'}
        '400':
          description: >-
            Invalid body, `private_key_supplied`, `contract_unsupported`.
          x-error-codes: [invalid_request, invalid_idempotency_key]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: >-
            The consumer limit for the level is reached.
          x-error-codes: [vault_consumer_limit_reached, vault_disabled]
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
      operationId: listVaultConsumers
      tags: [Vault]
      summary: List the vault consumers the key reaches
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: A page of consumers.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumerPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '410': {$ref: '#/components/responses/CursorExpired'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/consumers/{consumer_id}:
    parameters:
      - $ref: '#/components/parameters/VaultConsumerIdPath'
    get:
      operationId: getVaultConsumer
      tags: [Vault]
      summary: Read a vault consumer
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: The consumer.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumer'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    delete:
      operationId: revokeVaultConsumer
      tags: [Vault]
      summary: Revoke a vault consumer
      description: >-
        Its public keys are cleared and its open uses close; the row stays for the audit trail.
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      responses:
        '200':
          description: The consumer, `revoked`.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumer'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/consumers/{consumer_id}/activate:
    parameters:
      - $ref: '#/components/parameters/VaultConsumerIdPath'
    post:
      operationId: activateVaultConsumer
      tags: [Vault]
      summary: Activate a vault consumer
      description: >-
        A decision reserved for people (`Decision::ActivateConsumer`): a platform key; for a tenant-level
        self-hosted consumer, a signed-in owner or admin; or any `vault:admin` key with `PM_CONSOLE=off`.
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultConsumerActivateRequest'}
      responses:
        '200':
          description: The consumer.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumer'}
        '400':
          description: >-
            Invalid body.
          x-error-codes: [invalid_request, invalid_idempotency_key]
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

  /vault/consumers/{consumer_id}/suspend:
    parameters:
      - $ref: '#/components/parameters/VaultConsumerIdPath'
    post:
      operationId: suspendVaultConsumer
      tags: [Vault]
      summary: Suspend a vault consumer
      description: >-
        Closes its open uses (`consumer_suspended`).
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      responses:
        '200':
          description: The consumer.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumer'}
        '400':
          description: >-
            Invalid body.
          x-error-codes: [invalid_request, invalid_idempotency_key]
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

  /vault/consumers/{consumer_id}/resume:
    parameters:
      - $ref: '#/components/parameters/VaultConsumerIdPath'
    post:
      operationId: resumeVaultConsumer
      tags: [Vault]
      summary: Resume a vault consumer
      description: >-
        As activation.
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultConsumerActivateRequest'}
      responses:
        '200':
          description: The consumer.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumer'}
        '400':
          description: >-
            Invalid body.
          x-error-codes: [invalid_request, invalid_idempotency_key]
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

  /vault/consumers/{consumer_id}/rotate-keys:
    parameters:
      - $ref: '#/components/parameters/VaultConsumerIdPath'
    post:
      operationId: rotateKeysVaultConsumer
      tags: [Vault]
      summary: Rotate keys a vault consumer
      description: >-
        New keys take effect at once; the previous signing key verifies until the overlap ends.
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultConsumerKeysRequest'}
      responses:
        '200':
          description: The consumer.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumer'}
        '400':
          description: >-
            Invalid body.
          x-error-codes: [invalid_request, invalid_idempotency_key]
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

  /tenants/{tenant_id}/vault:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: getTenantVault
      tags: [Vault]
      summary: Read a tenant's vault status
      x-required-permission: ['vault:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      responses:
        '200':
          description: The status.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultStatus'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/vault/rotate-key:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    post:
      operationId: rotateTenantVaultKey
      tags: [Vault]
      summary: Rotate a tenant's vault key
      description: >-
        A new active key; versions are re-wrapped by the every-minute sweep. Emits `vault.key.changed`.
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
        - $ref: '#/components/parameters/VaultUnlockHeader'
      responses:
        '200':
          description: The status.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultStatus'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/vault/custody:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    post:
      operationId: enrolVaultCustody
      tags: [Vault]
      summary: Move a tenant's vault into partner custody
      description: >-
        The tenant's own partner key only. Returns one or two unlock keys once; a replay answers without them
        (`secret_replayed: false`).
      x-required-permission: ['vault:admin']
      x-key-levels: [partner]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultCustodyRequest'}
      responses:
        '200':
          description: The unlock keys, shown once.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultCustodyResponse'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
    delete:
      operationId: releaseVaultCustody
      tags: [Vault]
      summary: Return a tenant's vault to operator custody
      description: >-
        Needs a valid unlock key.
      x-required-permission: ['vault:admin']
      x-key-levels: [partner]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
        - $ref: '#/components/parameters/VaultUnlockHeader'
      responses:
        '200':
          description: The status.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultStatus'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/vault/revoke-all:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    post:
      operationId: revokeAllVault
      tags: [Vault]
      summary: Revoke every vault grant and use of a tenant
      description: >-
        Revokes every grant and closes every use; optionally deletes every session version and archives every
        item. Emits `vault.key.changed` (`revoke_all`).
      x-required-permission: ['vault:admin']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: optional
      parameters:
        - $ref: '#/components/parameters/IdempotencyKeyOptional'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultRevokeAllRequest'}
      responses:
        '200':
          description: Counts.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
            Idempotent-Replayed: {$ref: '#/components/headers/Idempotent-Replayed'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultRevokeAllResult'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /tenants/{tenant_id}/vault/audit:
    parameters:
      - $ref: '#/components/parameters/TenantIdPath'
    get:
      operationId: listVaultAudit
      tags: [Vault]
      summary: Read a tenant's vault audit
      description: >-
        Rows in `seq` order with their chain hashes.
      x-required-permission: ['audit:read']
      x-key-levels: [platform, partner, tenant]
      x-rate-limit-buckets: [api]
      x-idempotency: none
      parameters:
        - {name: item_id, in: query, required: false, description: 'Only rows about this item.', schema: {type: string}}
        - {name: identity_id, in: query, required: false, description: 'Only rows about this identity.', schema: {type: string}}
        - {name: consumer_id, in: query, required: false, description: 'Only rows about this consumer.', schema: {type: string}}
        - {name: action, in: query, required: false, description: 'Only rows with this action.', schema: {type: string}}
        - {name: since_seq, in: query, required: false, description: 'Rows after this sequence number.', schema: {type: integer, minimum: 0}}
        - $ref: '#/components/parameters/Limit'
        - $ref: '#/components/parameters/Cursor'
      responses:
        '200':
          description: A page of audit rows.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultAuditPage'}
        '400': {$ref: '#/components/responses/BadRequest'}
        '410': {$ref: '#/components/responses/CursorExpired'}
        '401': {$ref: '#/components/responses/Unauthorized'}
        '403': {$ref: '#/components/responses/Forbidden'}
        '404': {$ref: '#/components/responses/NotFound'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/consumer/self:
    get:
      operationId: getVaultConsumerSelf
      tags: [Vault]
      summary: Read the calling consumer's own record
      x-sdk: false
      security: [{vaultConsumerSignature: []}]
      x-rate-limit-buckets: [consumer]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/VaultSignatureInput'
        - $ref: '#/components/parameters/VaultSignature'
      responses:
        '200':
          description: Done.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumer'}
        '401':
          description: >-
            Any signature failure; the body is the same whatever the cause.
          x-error-codes: [consumer_unauthenticated]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: >-
            The consumer is not active, or its partner is suspended.
          x-error-codes: [consumer_not_active]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/consumer/redeem:
    post:
      operationId: redeemVaultTicket
      tags: [Vault]
      summary: Redeem one phase of a vault ticket
      x-sdk: false
      description: >-
        The checks of design § 8.6 in order; a binding refusal closes the use. The answer's packet is HPKE-sealed
        to the consumer's current encryption key. A phase is spent at the commit.
      security: [{vaultConsumerSignature: []}]
      x-rate-limit-buckets: [consumer]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/VaultSignatureInput'
        - $ref: '#/components/parameters/VaultSignature'
        - $ref: '#/components/parameters/VaultContentDigest'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultRedeemRequest'}
      responses:
        '200':
          description: Done.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultRedeemResponse'}
        '401':
          description: >-
            Any signature failure; the body is the same whatever the cause.
          x-error-codes: [consumer_unauthenticated]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '400':
          description: >-
            A phase that is not the use's, or `new_password` before `credentials`.
          x-error-codes: [invalid_request]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: >-
            A binding refusal (the use closes), a grant or ledger change, or the consumer is not active.
          x-error-codes: [vault_refused, vault_not_granted, consumer_not_active]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404':
          description: >-
            Unknown ticket, a ticket of another consumer, or a tenant out of reach.
          x-error-codes: [vault_ticket_not_found]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            The phase was spent, or the TOTP step was taken (retryable).
          x-error-codes: [vault_phase_spent, vault_totp_step_used]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '410':
          description: >-
            The use is closed or expired (`details.reason`).
          x-error-codes: [vault_ticket_expired]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '422':
          description: >-
            The vault or redemption is off.
          x-error-codes: [vault_disabled]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '413': {$ref: '#/components/responses/PayloadTooLarge'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/consumer/report:
    post:
      operationId: reportVaultOutcome
      tags: [Vault]
      summary: Report the outcome of a phase
      x-sdk: false
      description: >-
        Once per outcome; answers with `closed`.
      security: [{vaultConsumerSignature: []}]
      x-rate-limit-buckets: [consumer]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/VaultSignatureInput'
        - $ref: '#/components/parameters/VaultSignature'
        - $ref: '#/components/parameters/VaultContentDigest'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultReportRequest'}
      responses:
        '200':
          description: Done.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumerAck'}
        '401':
          description: >-
            Any signature failure; the body is the same whatever the cause.
          x-error-codes: [consumer_unauthenticated]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: >-
            The consumer is not active, or its partner is suspended.
          x-error-codes: [consumer_not_active]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404':
          description: >-
            Unknown ticket.
          x-error-codes: [vault_ticket_not_found]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            Already written back.
          x-error-codes: [vault_phase_spent]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '410':
          description: >-
            The window ended.
          x-error-codes: [vault_ticket_expired]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '413': {$ref: '#/components/responses/PayloadTooLarge'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/consumer/write-back:
    post:
      operationId: writeBackVault
      tags: [Vault]
      summary: Write back a session, a rotated secret or a password confirmation
      x-sdk: false
      description: >-
        Within the window; once per use. Needs only the tenant's public key.
      security: [{vaultConsumerSignature: []}]
      x-rate-limit-buckets: [consumer]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/VaultSignatureInput'
        - $ref: '#/components/parameters/VaultSignature'
        - $ref: '#/components/parameters/VaultContentDigest'
      requestBody:
        required: true
        content:
          application/json:
            schema: {$ref: '#/components/schemas/VaultWriteBackRequest'}
      responses:
        '200':
          description: Done.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultConsumerAck'}
        '401':
          description: >-
            Any signature failure; the body is the same whatever the cause.
          x-error-codes: [consumer_unauthenticated]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: >-
            The consumer is not active, or its partner is suspended.
          x-error-codes: [consumer_not_active]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404':
          description: >-
            Unknown ticket.
          x-error-codes: [vault_ticket_not_found]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '409':
          description: >-
            Already written back.
          x-error-codes: [vault_phase_spent]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '410':
          description: >-
            The window ended.
          x-error-codes: [vault_ticket_expired]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '413': {$ref: '#/components/responses/PayloadTooLarge'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}

  /vault/consumer/uses/{use_id}:
    parameters:
      - $ref: '#/components/parameters/VaultUseIdPath'
    get:
      operationId: getVaultUseStatusForConsumer
      tags: [Vault]
      summary: Read a use's status as its consumer
      x-sdk: false
      description: >-
        For a use bound to the calling consumer. `closed` reflects the live switches. At most 6 a minute per use.
      security: [{vaultConsumerSignature: []}]
      x-rate-limit-buckets: [consumer]
      x-idempotency: none
      parameters:
        - $ref: '#/components/parameters/VaultSignatureInput'
        - $ref: '#/components/parameters/VaultSignature'
      responses:
        '200':
          description: The status.
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
            RateLimit-Limit: {$ref: '#/components/headers/RateLimit-Limit'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/VaultUseStatus'}
        '401':
          description: >-
            Any signature failure; the body is the same whatever the cause.
          x-error-codes: [consumer_unauthenticated]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '403':
          description: >-
            The consumer is not active, or its partner is suspended.
          x-error-codes: [consumer_not_active]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '404':
          description: >-
            A use of another consumer, or unknown.
          x-error-codes: [vault_ticket_not_found]
          headers:
            Request-Id: {$ref: '#/components/headers/Request-Id'}
          content:
            application/json:
              schema: {$ref: '#/components/schemas/Error'}
        '429': {$ref: '#/components/responses/TooManyRequests'}
        '500': {$ref: '#/components/responses/InternalError'}
        '503': {$ref: '#/components/responses/ServiceUnavailable'}
```

## 3. Components

Under `components.securitySchemes`:

```yaml
    vaultConsumerSignature:
      type: apiKey
      in: header
      name: Signature
      description: >-
        Only for `/v1/vault/consumer/*`: an RFC 9421 HTTP message signature made with the consumer's Ed25519
        key, labelled `sig1`, over `("@method" "@authority" "@path" "content-digest")` (three components for
        `GET`), with `created`, `expires` (at most 60 seconds after `created`), a `nonce` of 16–64 random
        bytes, `keyid` (the RFC 7638 thumbprint of the consumer's signing JWK), `alg="ed25519"` and
        `tag="pylota-identity-vault-consumer"`. API keys are refused on these routes.
```

Under `components.parameters`:

```yaml
    IdempotencyKeyRequired:
      name: Idempotency-Key
      in: header
      required: true
      description: >-
        Required. 1–255 printable ASCII characters, kept for 30 days, scoped as `IdempotencyKeyOptional`.
        Missing: `400 idempotency_key_required`. A replayed vault use never carries its ticket.
      schema:
        type: string
        minLength: 1
        maxLength: 255
        pattern: '^[\x20-\x7E]{1,255}$'
    VaultUnlockHeader:
      name: X-Pylota-Identity-Vault-Unlock
      in: header
      required: false
      description: >-
        Partner custody only: an unlock key (`pvu_` + 52 Crockford base32 characters), accepted only from the
        tenant's own partner key on the unwrapping routes. Never logged, stored or echoed.
      schema:
        type: string
        pattern: '^pvu_[0-9a-hjkmnp-tv-z]{52}$'
    VaultItemIdPath:
      name: item_id
      in: path
      required: true
      schema: {$ref: '#/components/schemas/VaultItemId'}
    VaultGrantIdPath:
      name: grant_id
      in: path
      required: true
      schema: {$ref: '#/components/schemas/VaultGrantId'}
    VaultUseIdPath:
      name: use_id
      in: path
      required: true
      schema: {$ref: '#/components/schemas/VaultUseId'}
    VaultConsumerIdPath:
      name: consumer_id
      in: path
      required: true
      schema: {$ref: '#/components/schemas/VaultConsumerId'}
    VaultSignatureInput:
      name: Signature-Input
      in: header
      required: true
      description: The RFC 9421 signature parameters (see `vaultConsumerSignature`).
      schema:
        type: string
    VaultSignature:
      name: Signature
      in: header
      required: true
      description: The RFC 9421 signature, `sig1=:<base64>:`.
      schema:
        type: string
    VaultContentDigest:
      name: Content-Digest
      in: header
      required: true
      description: RFC 9530 `sha-256=:<base64>:` of the body as sent.
      schema:
        type: string
```

Under `components.schemas`:

```yaml
    # ── Vault ────────────────────────────────────────────────────────────────────────────────────

    VaultItemId:
      type: string
      description: Vault item ID (`vit_` + ULID).
      pattern: '^vit_[0-9A-HJKMNP-TV-Z]{26}$'
    VaultGrantId:
      type: string
      description: Vault grant ID (`vgr_` + ULID).
      pattern: '^vgr_[0-9A-HJKMNP-TV-Z]{26}$'
    VaultUseId:
      type: string
      description: Vault use ID (`vus_` + ULID).
      pattern: '^vus_[0-9A-HJKMNP-TV-Z]{26}$'
    VaultConsumerId:
      type: string
      description: Vault consumer ID (`vcn_` + ULID).
      pattern: '^vcn_[0-9A-HJKMNP-TV-Z]{26}$'
    VaultOrigin:
      type: string
      description: An exact `https` origin, canonicalised (lower case, IDNA A-label, no path, port 443 dropped).
      pattern: '^https://[a-z0-9.-]+(:[0-9]{4,5})?$'
      examples: ['https://tfl.gov.uk']
    VaultItemKind:
      type: string
      enum: [login, api_key, session, managed_secret]
    VaultApprovalLevel:
      type: string
      enum: [grant, per_run, per_use]
    VaultOperation:
      type: string
      enum: [fill, rotate_fill, session_load, session_capture, inject, sign]
    VaultPhase:
      type: string
      enum: [credentials, totp, email_otp, sms_otp, session, secret, new_password]
    VaultTrigger:
      type: string
      enum: [login_form, signup_form, reauth_prompt, otp_field, session_start, person_handoff, api_request, password_change, sign_request]
    VaultHealth:
      type: string
      enum: [ok, needs_auth, needs_attention, needs_rotation]
    VaultOkpJwk:
      type: object
      description: A public OKP JWK. A JWK with a private member (`d`) is refused.
      required: [kty, crv, x]
      properties:
        kty: {type: string, const: OKP}
        crv: {type: string, enum: [Ed25519, X25519]}
        x: {type: string}
      additionalProperties: false
    VaultPasswordInput:
      oneOf:
        - type: string
          minLength: 1
          maxLength: 1024
          description: The password. Write-only.
        - type: object
          required: [generate]
          properties:
            generate:
              type: object
              properties:
                length: {type: integer, minimum: 16, maximum: 128, default: 24}
                classes:
                  type: array
                  items: {type: string, enum: [lower, upper, digit, symbol]}
    VaultOtpChannel:
      oneOf:
        - type: object
          required: [kind, address, sender_domains]
          properties:
            kind: {type: string, const: email}
            address: {$ref: '#/components/schemas/EmailAddress'}
            sender_domains: {type: array, minItems: 1, maxItems: 6, items: {type: string}}
        - type: object
          required: [kind, number_id, senders]
          properties:
            kind: {type: string, const: phone}
            number_id: {type: string}
            senders: {type: array, minItems: 1, maxItems: 10, items: {type: string}}
    VaultSessionState:
      type: object
      description: Playwright `storageState` shape. Write-only; filtered to the item's origins before sealing.
      required: [cookies]
      properties:
        cookies:
          type: array
          maxItems: 500
          items:
            type: object
            required: [name, value, domain, path]
            properties:
              name: {type: string}
              value: {type: string}
              domain: {type: string}
              path: {type: string}
              expires: {type: number}
              httpOnly: {type: boolean}
              secure: {type: boolean}
              sameSite: {type: string, enum: [Strict, Lax, None]}
        origins:
          type: array
          maxItems: 10
          items:
            type: object
            required: [origin, local_storage]
            properties:
              origin: {$ref: '#/components/schemas/VaultOrigin'}
              local_storage:
                type: array
                items:
                  type: object
                  required: [name, value]
                  properties:
                    name: {type: string}
                    value: {type: string}
    VaultItemCreateRequest:
      type: object
      description: >-
        One request shape for the three kinds; fields that do not belong to `kind` are refused. Values
        (`password`, `totp`, `recovery_codes`, `secret`, `state`) are write-only.
      required: [kind, title, origins]
      properties:
        kind: {type: string, enum: [login, api_key, session]}
        title: {type: string, minLength: 1, maxLength: 120}
        account_id:
          $ref: '#/components/schemas/ServiceAccountId'
        username: {type: string, maxLength: 254}
        origins:
          type: array
          minItems: 1
          maxItems: 10
          items: {$ref: '#/components/schemas/VaultOrigin'}
        approval: {$ref: '#/components/schemas/VaultApprovalLevel'}
        password: {$ref: '#/components/schemas/VaultPasswordInput'}
        totp:
          type: string
          description: An `otpauth://totp/` URI. Write-only; never returned, not even to a person.
        recovery_codes:
          type: array
          minItems: 1
          maxItems: 20
          items: {type: string, maxLength: 64}
        otp_channel: {$ref: '#/components/schemas/VaultOtpChannel'}
        secret: {type: string, minLength: 1, maxLength: 16384}
        inject:
          type: object
          required: [scheme]
          properties:
            scheme: {type: string, enum: [bearer, basic, header]}
            header_name: {type: string, maxLength: 128}
            username: {type: string, maxLength: 254}
        rotate_after_use: {type: boolean, default: false}
        sign:
          type: object
          description: P1.
          properties:
            scheme: {type: string, enum: [hmac_sha256, aws_sigv4]}
            encoding: {type: string, enum: [hex, base64]}
            access_key_id: {type: string}
            region: {type: string}
            service: {type: string}
        state: {$ref: '#/components/schemas/VaultSessionState'}
        session:
          type: object
          properties:
            ttl_seconds: {type: integer, minimum: 3600}
        import:
          type: object
          required: [source, source_ref]
          properties:
            source: {type: string, enum: [pylota_sealed_session]}
            source_ref: {type: string, maxLength: 64}
        rotation:
          type: object
          properties:
            remind_after_days: {type: [integer, 'null'], minimum: 7, maximum: 730}
    VaultItemUpdateRequest:
      type: object
      minProperties: 1
      properties:
        title: {type: string, minLength: 1, maxLength: 120}
        origins:
          type: array
          minItems: 1
          maxItems: 10
          items: {$ref: '#/components/schemas/VaultOrigin'}
        approval: {$ref: '#/components/schemas/VaultApprovalLevel'}
        state: {type: string, enum: [active, archived]}
        otp_channel:
          oneOf:
            - {$ref: '#/components/schemas/VaultOtpChannel'}
            - {type: 'null'}
        rotation:
          type: object
          properties:
            remind_after_days: {type: [integer, 'null'], minimum: 7, maximum: 730}
    VaultItemVersionRequest:
      type: object
      description: The kind's values (write-only) and the overlap for the previous version.
      properties:
        password: {$ref: '#/components/schemas/VaultPasswordInput'}
        totp: {type: string}
        recovery_codes: {type: array, items: {type: string, maxLength: 64}}
        secret: {type: string, minLength: 1, maxLength: 16384}
        state: {$ref: '#/components/schemas/VaultSessionState'}
        overlap_hours: {type: integer, minimum: 0, maximum: 168, default: 24}
    VaultItem:
      type: object
      description: A vault item. Never carries a value. `title` and `username` are not secrets; show `title` as untrusted text.
      required: [id, tenant_id, identity_id, kind, title, account_id, username, origins, approval, state, health, health_reason, current_version, has, otp_channel, rotation, session, managed_by, created_by, first_used_at, last_used_at, created_at, updated_at]
      properties:
        id: {$ref: '#/components/schemas/VaultItemId'}
        tenant_id: {$ref: '#/components/schemas/TenantId'}
        identity_id:
          oneOf:
            - {$ref: '#/components/schemas/IdentityId'}
            - {type: 'null'}
        kind: {$ref: '#/components/schemas/VaultItemKind'}
        title: {type: string}
        account_id: {type: [string, 'null']}
        username: {type: [string, 'null']}
        origins: {type: array, items: {$ref: '#/components/schemas/VaultOrigin'}}
        approval: {$ref: '#/components/schemas/VaultApprovalLevel'}
        state: {type: string, enum: [active, archived]}
        health: {$ref: '#/components/schemas/VaultHealth'}
        health_reason:
          type: [string, 'null']
          enum: [session_rejected, session_expired, credentials_rejected, otp_rejected, value_exposed, rotation_due, rotation_unconfirmed, manual, null]
        current_version: {type: integer, minimum: 1}
        has:
          type: object
          properties:
            password: {type: boolean}
            totp: {type: boolean}
            recovery_codes: {type: boolean}
            email_otp: {type: boolean}
            sms_otp: {type: boolean}
            secret: {type: boolean}
            session_state: {type: boolean}
        otp_channel:
          oneOf:
            - {$ref: '#/components/schemas/VaultOtpChannel'}
            - {type: 'null'}
        rotation:
          type: object
          properties:
            remind_after_days: {type: [integer, 'null']}
            last_rotated_at: {type: [string, 'null'], format: date-time}
        session:
          type: [object, 'null']
          properties:
            ttl_seconds: {type: integer}
            expires_at: {type: string, format: date-time}
            size_bytes: {type: integer}
            cookie_count: {type: integer}
            origin_count: {type: integer}
        managed_by: {type: [string, 'null'], enum: [phone, null]}
        created_by:
          type: object
          properties:
            key_id: {type: [string, 'null']}
            user_id: {type: [string, 'null']}
        first_used_at: {type: [string, 'null'], format: date-time}
        last_used_at: {type: [string, 'null'], format: date-time}
        created_at: {type: string, format: date-time}
        updated_at: {type: string, format: date-time}
    VaultItemCreated:
      allOf:
        - {$ref: '#/components/schemas/VaultItem'}
        - type: object
          properties:
            cookies_dropped: {type: integer}
            clearance_cookies_dropped: {type: integer}
            origins_dropped: {type: integer}
    VaultItemPage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/VaultItem'}}
        next_cursor: {type: [string, 'null']}
    VaultShredReceipt:
      type: object
      required: [item_id, deleted, versions_shredded, r2_objects_deleted, uses_closed, grants_deleted, restorable_until, custody]
      properties:
        item_id: {$ref: '#/components/schemas/VaultItemId'}
        deleted: {type: boolean, const: true}
        versions_shredded: {type: integer}
        r2_objects_deleted: {type: integer}
        uses_closed: {type: integer}
        grants_deleted: {type: integer}
        restorable_until: {type: [string, 'null'], format: date-time}
        custody: {type: string, enum: [operator, partner]}
    VaultGrantRequest:
      type: object
      required: [item_id, operations, purpose]
      properties:
        item_id: {$ref: '#/components/schemas/VaultItemId'}
        operations: {type: array, minItems: 1, items: {$ref: '#/components/schemas/VaultOperation'}}
        origins: {type: array, minItems: 1, maxItems: 10, items: {$ref: '#/components/schemas/VaultOrigin'}}
        purpose: {type: string, minLength: 1, maxLength: 500, description: Untrusted text shown to the approver.}
        max_uses: {type: [integer, 'null'], minimum: 1, maximum: 100000}
        approve: {type: boolean, default: false}
        expires_in: {type: integer, minimum: 3600, maximum: 7776000}
        on_behalf_of: {type: string, minLength: 1, maxLength: 128, pattern: '^[\x21-\x3F\x41-\x7E]+$'}
    VaultGrantDecision:
      type: object
      properties:
        expires_in: {type: integer, minimum: 3600, maximum: 7776000, default: 604800}
        note: {type: string, maxLength: 500}
        reason: {type: string, maxLength: 500}
        on_behalf_of: {type: string, minLength: 1, maxLength: 128, pattern: '^[\x21-\x3F\x41-\x7E]+$'}
    VaultGrant:
      type: object
      required: [id, tenant_id, item_id, identity_id, operations, origins, status, purpose, max_uses, use_count, requested_by_key_id, decided_by_key_id, decided_by_user_id, on_behalf_of, note, decided_at, expires_at, revoked_at, revoke_reason, created_at, updated_at]
      properties:
        id: {$ref: '#/components/schemas/VaultGrantId'}
        tenant_id: {$ref: '#/components/schemas/TenantId'}
        item_id: {$ref: '#/components/schemas/VaultItemId'}
        identity_id: {$ref: '#/components/schemas/IdentityId'}
        operations: {type: array, items: {$ref: '#/components/schemas/VaultOperation'}}
        origins: {type: array, items: {$ref: '#/components/schemas/VaultOrigin'}}
        status: {type: string, enum: [pending_approval, active, denied, revoked, expired]}
        purpose: {type: string}
        max_uses: {type: [integer, 'null']}
        use_count: {type: integer}
        requested_by_key_id: {type: [string, 'null']}
        decided_by_key_id: {type: [string, 'null']}
        decided_by_user_id: {type: [string, 'null']}
        on_behalf_of: {type: [string, 'null']}
        note: {type: [string, 'null']}
        decided_at: {type: [string, 'null'], format: date-time}
        expires_at: {type: string, format: date-time}
        revoked_at: {type: [string, 'null'], format: date-time}
        revoke_reason: {type: [string, 'null']}
        created_at: {type: string, format: date-time}
        updated_at: {type: string, format: date-time}
    VaultGrantPage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/VaultGrant'}}
        next_cursor: {type: [string, 'null']}
    VaultUseCreateRequest:
      type: object
      required: [item_id, operation, consumer_id, run_id, origin, trigger, purpose]
      properties:
        item_id: {$ref: '#/components/schemas/VaultItemId'}
        operation: {$ref: '#/components/schemas/VaultOperation'}
        phases: {type: array, minItems: 1, uniqueItems: true, items: {$ref: '#/components/schemas/VaultPhase'}}
        consumer_id: {$ref: '#/components/schemas/VaultConsumerId'}
        run_id: {type: string, minLength: 1, maxLength: 128, pattern: '^[\x21-\x3F\x41-\x7E]+$'}
        origin: {$ref: '#/components/schemas/VaultOrigin'}
        trigger: {$ref: '#/components/schemas/VaultTrigger'}
        purpose: {type: string, minLength: 1, maxLength: 200}
        capture_into: {type: [string, 'null']}
        run_window_seconds: {type: [integer, 'null'], minimum: 300, maximum: 7200}
        version: {type: string, enum: [current, previous], default: current}
        sign_request:
          type: object
          description: P1, for `operation = sign` only.
          properties:
            message_base64: {type: string, maxLength: 87384}
            method: {type: string}
            url: {type: string, format: uri}
            headers: {type: object, maxProperties: 50, additionalProperties: {type: string}}
            body_sha256: {type: string}
            timestamp: {type: string, format: date-time}
    VaultUse:
      type: object
      description: A use never carries its ticket, its hash or a sealed key.
      required: [id, tenant_id, identity_id, item_id, grant_id, operation, phases, phases_redeemed, consumer_id, run_id, origin, trigger, purpose, status, approval_reason, approved_by, digest, deadline_at, redeem_by, closes_at, close_reason, outcome, created_by_key_id, decided_by_key_id, decided_by_user_id, on_behalf_of, created_at, updated_at]
      properties:
        id: {$ref: '#/components/schemas/VaultUseId'}
        tenant_id: {$ref: '#/components/schemas/TenantId'}
        identity_id: {$ref: '#/components/schemas/IdentityId'}
        item_id: {$ref: '#/components/schemas/VaultItemId'}
        grant_id: {$ref: '#/components/schemas/VaultGrantId'}
        operation: {$ref: '#/components/schemas/VaultOperation'}
        phases: {type: array, items: {$ref: '#/components/schemas/VaultPhase'}}
        phases_redeemed: {type: array, items: {$ref: '#/components/schemas/VaultPhase'}}
        consumer_id: {type: [string, 'null']}
        run_id: {type: string}
        origin: {$ref: '#/components/schemas/VaultOrigin'}
        trigger: {$ref: '#/components/schemas/VaultTrigger'}
        purpose: {type: string}
        status: {type: string, enum: [pending_approval, approved, in_use, closed, denied, expired, canceled]}
        approval_reason: {type: [string, 'null'], enum: [per_use, per_run, operation, tenant_floor, null]}
        approved_by: {type: [string, 'null'], enum: [person, key, run, grant, null]}
        digest: {type: string, pattern: '^[0-9a-f]{64}$'}
        deadline_at: {type: [string, 'null'], format: date-time}
        redeem_by: {type: [string, 'null'], format: date-time}
        closes_at: {type: [string, 'null'], format: date-time}
        close_reason: {type: [string, 'null']}
        outcome: {type: [string, 'null']}
        created_by_key_id: {type: string}
        decided_by_key_id: {type: [string, 'null']}
        decided_by_user_id: {type: [string, 'null']}
        on_behalf_of: {type: [string, 'null']}
        created_at: {type: string, format: date-time}
        updated_at: {type: string, format: date-time}
    VaultUseCreated:
      type: object
      required: [use, ticket, ticket_expires_at]
      properties:
        use: {$ref: '#/components/schemas/VaultUse'}
        ticket: {type: [string, 'null'], description: 'Shown once, here or by `mintVaultTicket`. Never replayed.'}
        ticket_expires_at: {type: [string, 'null'], format: date-time}
        ticket_replayed: {type: boolean}
        signature: {$ref: '#/components/schemas/VaultSignature'}
    VaultUseApproveRequest:
      type: object
      required: [digest]
      properties:
        digest: {type: string, pattern: '^[0-9a-f]{64}$'}
        note: {type: string, maxLength: 500}
        on_behalf_of: {type: string, minLength: 1, maxLength: 128}
    VaultUsePage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/VaultUse'}}
        next_cursor: {type: [string, 'null']}
    VaultTicket:
      type: object
      required: [ticket, ticket_expires_at, phases, use_id]
      properties:
        ticket: {type: string, pattern: '^vtk_[0-9a-hjkmnp-tv-z]{12}_[0-9a-hjkmnp-tv-z]{52}$'}
        ticket_expires_at: {type: string, format: date-time}
        phases: {type: array, items: {$ref: '#/components/schemas/VaultPhase'}}
        use_id: {$ref: '#/components/schemas/VaultUseId'}
    VaultSignature:
      type: object
      description: P1. A signature is not the secret.
      properties:
        mac: {type: string}
        authorization: {type: string}
        x_amz_date: {type: string}
        x_amz_content_sha256: {type: string}
    VaultConsumerDeclarations:
      type: object
      required: [session_recording, session_logging, captcha_solving, extensions, honest_identity, observation_paused_during_fill, post_fill_check, modes]
      properties:
        session_recording: {type: string, const: 'off'}
        session_logging: {type: string, const: 'off'}
        captcha_solving: {type: string, const: 'off'}
        extensions: {type: string, const: none}
        honest_identity: {type: string, enum: [web_bot_auth, user_agent, none]}
        observation_paused_during_fill: {type: boolean, const: true}
        post_fill_check: {type: boolean, const: true}
        modes: {type: array, minItems: 1, items: {type: string, enum: [cdp, proxy]}}
    VaultConsumerCreateRequest:
      type: object
      required: [kind, name, level, hosting, contract, signing_jwk, encryption_jwk, declarations]
      properties:
        kind: {type: string, enum: [browser, egress_proxy, executor]}
        name: {type: string, minLength: 1, maxLength: 120}
        level: {type: string, enum: [platform, partner, tenant]}
        tenant_id: {$ref: '#/components/schemas/TenantId'}
        hosting: {type: string, enum: [self_hosted, third_party]}
        provider: {type: [string, 'null'], minLength: 1, maxLength: 64}
        contract: {type: string, const: vault-consumer/1}
        signing_jwk: {$ref: '#/components/schemas/VaultOkpJwk'}
        encryption_jwk: {$ref: '#/components/schemas/VaultOkpJwk'}
        declarations: {$ref: '#/components/schemas/VaultConsumerDeclarations'}
    VaultConsumerActivateRequest:
      type: object
      required: [note]
      properties:
        note: {type: string, minLength: 1, maxLength: 500}
    VaultConsumerKeysRequest:
      type: object
      required: [signing_jwk, encryption_jwk]
      properties:
        signing_jwk: {$ref: '#/components/schemas/VaultOkpJwk'}
        encryption_jwk: {$ref: '#/components/schemas/VaultOkpJwk'}
        overlap_seconds: {type: integer, minimum: 0, maximum: 86400, default: 3600}
    VaultConsumer:
      type: object
      required: [id, kind, name, level, tenant_id, partner_id, hosting, provider, contract, signing_kid, encryption_kid, declarations, status, created_at, updated_at]
      properties:
        id: {$ref: '#/components/schemas/VaultConsumerId'}
        kind: {type: string, enum: [browser, egress_proxy, executor]}
        name: {type: string}
        level: {type: string, enum: [platform, partner, tenant]}
        tenant_id: {type: [string, 'null']}
        partner_id: {type: [string, 'null']}
        hosting: {type: string, enum: [self_hosted, third_party]}
        provider: {type: [string, 'null']}
        contract: {type: string}
        signing_kid: {type: [string, 'null']}
        encryption_kid: {type: [string, 'null']}
        signing_jwk: {$ref: '#/components/schemas/VaultOkpJwk'}
        encryption_jwk: {$ref: '#/components/schemas/VaultOkpJwk'}
        declarations: {$ref: '#/components/schemas/VaultConsumerDeclarations'}
        status: {type: string, enum: [pending_activation, active, suspended, revoked]}
        activated_at: {type: [string, 'null'], format: date-time}
        created_at: {type: string, format: date-time}
        updated_at: {type: string, format: date-time}
    VaultConsumerPage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/VaultConsumer'}}
        next_cursor: {type: [string, 'null']}
    VaultStatus:
      type: object
      required: [enabled, custody, kid, items, active_grants, pending_grants, pending_uses, retiring_versions]
      properties:
        enabled: {type: boolean}
        custody: {type: string, enum: [operator, partner]}
        kid: {type: [string, 'null']}
        items: {type: integer}
        active_grants: {type: integer}
        pending_grants: {type: integer}
        pending_uses: {type: integer}
        retiring_versions: {type: integer}
    VaultCustodyRequest:
      type: object
      required: [slots]
      properties:
        slots: {type: integer, enum: [1, 2]}
    VaultCustodyResponse:
      type: object
      required: [custody, unlock_keys, secret_replayed]
      properties:
        custody: {type: string, const: partner}
        unlock_keys: {type: array, maxItems: 2, items: {type: string}, description: 'Shown once; empty on a replay.'}
        secret_replayed: {type: boolean}
        operator_copy_restorable_until: {type: [string, 'null'], format: date-time}
    VaultRevokeAllRequest:
      type: object
      properties:
        shred_sessions: {type: boolean, default: false}
        archive_items: {type: boolean, default: false}
    VaultRevokeAllResult:
      type: object
      required: [grants_revoked, uses_closed, session_versions_deleted, items_archived]
      properties:
        grants_revoked: {type: integer}
        uses_closed: {type: integer}
        session_versions_deleted: {type: integer}
        items_archived: {type: integer}
    VaultAuditRow:
      type: object
      required: [id, tenant_id, seq, prev_hash, row_hash, action, created_at]
      properties:
        id: {type: string, pattern: '^vad_[0-9A-HJKMNP-TV-Z]{26}$'}
        tenant_id: {$ref: '#/components/schemas/TenantId'}
        seq: {type: integer, minimum: 1}
        prev_hash: {type: string, pattern: '^[0-9a-f]{64}$'}
        row_hash: {type: string, pattern: '^[0-9a-f]{64}$'}
        action: {type: string}
        item_id: {type: [string, 'null']}
        version: {type: [integer, 'null']}
        grant_id: {type: [string, 'null']}
        use_id: {type: [string, 'null']}
        consumer_id: {type: [string, 'null']}
        identity_id: {type: [string, 'null']}
        actor_key_id: {type: [string, 'null']}
        actor_user_id: {type: [string, 'null']}
        actor_consumer_id: {type: [string, 'null']}
        on_behalf_of: {type: [string, 'null']}
        operation: {type: [string, 'null']}
        phase: {type: [string, 'null']}
        outcome: {type: [string, 'null']}
        reason: {type: [string, 'null']}
        site: {type: [string, 'null'], description: The registrable domain of the origin concerned; never a URL.}
        run_id: {type: [string, 'null']}
        details: {type: object, description: Counts and enumerations only.}
        created_at: {type: string, format: date-time}
    VaultAuditPage:
      type: object
      required: [data, next_cursor]
      properties:
        data: {type: array, items: {$ref: '#/components/schemas/VaultAuditRow'}}
        next_cursor: {type: [string, 'null']}
    VaultRedeemContext:
      type: object
      required: [run_id, trigger, mode]
      properties:
        run_id: {type: string}
        top_level_origin: {type: string}
        frame_origin: {type: string}
        form_kind: {type: [string, 'null'], enum: [login, signup, password_change, otp, reauth, null]}
        trigger: {$ref: '#/components/schemas/VaultTrigger'}
        mode: {type: string, enum: [cdp, proxy]}
        browser:
          type: object
          properties:
            hosting: {type: string, enum: [self_hosted, third_party]}
            provider: {type: [string, 'null']}
            session_recording: {type: string}
            session_logging: {type: string}
            captcha_solving: {type: string}
            extensions: {type: string}
    VaultRedeemRequest:
      type: object
      required: [ticket, phase, context]
      properties:
        ticket: {type: string}
        phase: {$ref: '#/components/schemas/VaultPhase'}
        wait_seconds: {type: integer, minimum: 0, maximum: 60, default: 0}
        context: {$ref: '#/components/schemas/VaultRedeemContext'}
    VaultSealedPacket:
      type: object
      description: HPKE base mode, DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, AES-256-GCM, sealed to the consumer's encryption key named in `enc_kid`.
      required: [suite, enc, ct]
      properties:
        suite: {type: string, const: x25519-hkdfsha256-aes256gcm}
        enc: {type: string, description: base64url}
        ct: {type: string, description: base64url}
    VaultRedeemResponse:
      type: object
      required: [use_id, phase, enc_kid, packet, remaining_phases, closes_at, timed_out]
      properties:
        use_id: {$ref: '#/components/schemas/VaultUseId'}
        phase: {$ref: '#/components/schemas/VaultPhase'}
        enc_kid: {type: string}
        packet:
          oneOf:
            - {$ref: '#/components/schemas/VaultSealedPacket'}
            - {type: 'null'}
        remaining_phases: {type: array, items: {$ref: '#/components/schemas/VaultPhase'}}
        closes_at: {type: string, format: date-time}
        timed_out: {type: boolean}
    VaultReportRequest:
      type: object
      required: [ticket, outcome]
      properties:
        ticket: {type: string}
        phase: {$ref: '#/components/schemas/VaultPhase'}
        outcome: {type: string, enum: [filled, submitted, signed_in, signed_up, credentials_rejected, otp_rejected, session_rejected, value_exposed, value_exposed_unscannable, placeholder_misplaced, origin_mismatch, frame_mismatch, form_not_found, aborted, password_changed, password_change_failed]}
        final: {type: boolean, default: false}
    VaultWriteBackRequest:
      type: object
      required: [ticket, kind]
      properties:
        ticket: {type: string}
        kind: {type: string, enum: [session, secret, password_confirmed]}
        state: {$ref: '#/components/schemas/VaultSessionState'}
        secret: {type: string, minLength: 1, maxLength: 16384}
    VaultConsumerAck:
      type: object
      required: [use_id, closed, close_reason]
      properties:
        use_id: {$ref: '#/components/schemas/VaultUseId'}
        closed: {type: boolean}
        close_reason: {type: [string, 'null']}
        cookies_dropped: {type: integer}
        clearance_cookies_dropped: {type: integer}
    VaultUseStatus:
      type: object
      required: [use_id, status, closed, close_reason, closes_at, remaining_phases]
      properties:
        use_id: {$ref: '#/components/schemas/VaultUseId'}
        status: {type: string}
        closed: {type: boolean}
        close_reason: {type: [string, 'null']}
        closes_at: {type: [string, 'null'], format: date-time}
        remaining_phases: {type: array, items: {$ref: '#/components/schemas/VaultPhase'}}
```

Also add `vit_ vgr_ vus_ vcn_ vad_` to the ID prefixes in `info.description` › Conventions, `/v1/vault/consumer/*`
(signed, no key) to the list of unauthenticated routes, and the `consumer` rate-limit bucket to the rate-limit
convention.
