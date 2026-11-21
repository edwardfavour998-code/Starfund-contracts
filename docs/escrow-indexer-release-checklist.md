# Escrow Indexer Release Checklist

This checklist is a release-time compatibility review artifact for changes that affect the escrow indexer. It must be completed for any pull request that changes escrow contract events or error codes, and the completed checklist must be attached to the release PR before merge.

The goal is to ensure that topic changes, payload changes, and numeric error changes never reach indexer consumers without a clear migration notice, updated decoder fixtures, and documented backward-compatibility impact.

## When this checklist is required

Complete this checklist if the change touches any of the following:

- Event topic names, topic ordering, or the set of indexed topics (e.g. `escrow_created`, `escrow_funded`, `escrow_released`, `escrow_refunded`, `escrow_disputed`).
- Event payload fields: added, removed, renamed, reordered, or type-changed fields in any escrow event.
- Numeric error codes or error enum discriminants emitted by the escrow contract (e.g. `EscrowError` variants).
- The event schema version or the shape of `docs/EVENT_SCHEMA.md`.

If none of the above apply, state "Not applicable" with a one-line justification and skip the rest.

## Checklist

### 1. Change inventory

- [ ] List every changed event topic (added, removed, renamed, or reordered). If none, write "None".
- [ ] List every changed event payload field (added, removed, renamed, reordered, or type-changed) per event. If none, write "None".
- [ ] List every changed numeric error code or error variant (added, removed, renumbered, or repurposed). If none, write "None".
- [ ] State the event schema version before and after the change.

### 2. Event schema diff

- [ ] Attach an event schema diff comparing the previous and new event definitions (topics, payloads, and error codes).
- [ ] Confirm `docs/EVENT_SCHEMA.md` has been updated to match the new schema.
- [ ] Confirm `docs/escrow-indexer.md` has been updated where consumption behavior changes.

### 3. Decoder fixture updates

- [ ] Update or add decoder fixtures for every changed event topic and payload.
- [ ] Update or add fixtures for every changed numeric error code.
- [ ] Confirm fixtures cover both the old and new shapes where backward compatibility is claimed.
- [ ] Confirm the indexer test suite passes against the updated fixtures.

### 4. Backward-compatibility notes

- [ ] For each changed event, document whether the change is backward compatible for existing consumers.
- [ ] For each breaking change, describe the migration path (new topic, new field, fallback decoding, or versioned handler).
- [ ] For each changed numeric error code, document how consumers that map error codes to messages must update.
- [ ] Note any required consumer-side updates (re-indexing, schema migration, decoder upgrade) and the release in which they take effect.

### 5. Consumer notification

- [ ] Confirm the migration notice has been drafted for indexer consumers.
- [ ] Confirm the notice names the affected topics, payloads, and numeric error codes.
- [ ] Confirm the notice links to the event schema diff and backward-compatibility notes.

### 6. Sign-off

- [ ] Contract author sign-off.
- [ ] Indexer maintainer sign-off.
- [ ] Release manager sign-off.

## Sample completed checklist

The following is a worked example applying this checklist to an existing event-changing PR. It is illustrative; adjust names and versions to match the actual PR.

**PR:** `feat(escrow): add dispute reason to escrow_disputed event and renumber EscrowError`
**Schema version:** `v1` → `v2`

### 1. Change inventory

- Changed event topics: None. `escrow_disputed` topic name and ordering are unchanged.
- Changed event payload fields:
  - `escrow_disputed`: added `reason: u32` field after `escrow_id`.
- Changed numeric error codes:
  - `EscrowError::AlreadyDisputed` renumbered from `7` to `8` to make room for `EscrowError::InvalidDisputeReason = 7`.

### 2. Event schema diff

- [x] Event schema diff attached: `escrow_disputed` payload gains `reason`; `EscrowError` discriminants shift as noted above.
- [x] `docs/EVENT_SCHEMA.md` updated to `v2` with the new `reason` field and the renumbered error table.
- [x] `docs/escrow-indexer.md` updated to describe decoding `reason` and the error code remap.

### 3. Decoder fixture updates

- [x] Added fixture `escrow_disputed_v2_with_reason.json`.
- [x] Retained `escrow_disputed_v1.json` to verify fallback decoding when `reason` is absent.
- [x] Added fixture `escrow_error_invalid_dispute_reason.json` for code `7` and updated the `AlreadyDisputed` fixture to code `8`.
- [x] Indexer test suite passes against updated fixtures.

### 4. Backward-compatibility notes

- `escrow_disputed` payload change is additive: consumers that ignore unknown fields continue to work; consumers that decode strictly must add `reason` or use the versioned decoder.
- Numeric error change is breaking: any consumer mapping `7` to `AlreadyDisputed` must remap `7` to `InvalidDisputeReason` and `8` to `AlreadyDisputed`.
- Required consumer updates: upgrade decoder to `v2`, re-index `escrow_disputed` events emitted after the upgrade block, and update error-code message tables. Effective in the next indexer release.

### 5. Consumer notification

- [x] Migration notice drafted naming `escrow_disputed` payload change and the `EscrowError` renumbering.
- [x] Notice links to the event schema diff and the backward-compatibility notes above.

### 6. Sign-off

- [x] Contract author sign-off.
- [x] Indexer maintainer sign-off.
- [x] Release manager sign-off.

### Required consumer updates identified

1. Upgrade the indexer decoder to `v2` to read `escrow_disputed.reason`.
2. Re-index `escrow_disputed` events from the upgrade block onward.
3. Remap numeric error codes: `7` → `InvalidDisputeReason`, `8` → `AlreadyDisputed`.
4. Update any dashboards or alerts keyed on the old error numbering.