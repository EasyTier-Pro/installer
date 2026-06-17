# Desktop Desired-State Reconcile Design

This document designs a future `desktop reconcile --json` operation for desktop clients that want the installer to converge the local EasyTier service to a requested desired state. It is intentionally a design document only: no destructive reconcile command is shipped by this plan.

The operation should stay conservative. In particular, identity mismatch and config mismatch cases must not silently repoint a running service to a different account or Console endpoint.

## Goals

- Give desktop clients a single operation that can inspect current state and choose a safe lifecycle action.
- Reuse the existing desktop `status`, `install`, `update`, and `uninstall` semantics instead of duplicating decision logic in the UI.
- Make the operation safe for unattended use when the current installation already matches the desired state.
- Require explicit confirmation for destructive or account-repointing changes.

## Non-Goals

- Do not add new network calls beyond the current desktop install/update/status flows.
- Do not change existing `desktop install`, `desktop update`, or `desktop uninstall` behavior.
- Do not auto-uninstall or purge data without explicit product and maintainer approval.

## Request

The request body would be a single JSON object written to `stdin`, following the existing desktop protocol style.

| Field | Required | Description |
|-------|----------|-------------|
| `install_dir` | No | Desired installation directory. If omitted, use the same defaulting rules as existing desktop commands. |
| `config_server` | No | Desired config server base address. If omitted, reconcile cannot prove config match and must avoid config-repointing actions. |
| `bootstrap_token` | Yes for `apply`, optional for `plan_only` | Desired bootstrap token. It is used only to calculate/compare fingerprint; the raw value must never appear in events or logs. |
| `version` | Yes for install/update decisions | Desired EasyTier version, normalized with the same rules as existing desktop status/install/update. |
| `purge` | No | Desired purge policy for destructive cleanup. Defaults to `false`. Any `true` path must use the purge safety checks from plan 004 before implementation. |
| `mode` | No | `plan_only` or `apply`. Defaults to `plan_only` until maintainers explicitly approve apply semantics. |
| `confirm` | No | Optional object for explicit confirmations, such as reinstalling on identity mismatch or enabling purge. Exact shape is an open question. |

Example placeholder request:

```json
{
  "mode": "plan_only",
  "bootstrap_token": "BOOTSTRAP_TOKEN",
  "install_dir": "/opt/easytier",
  "config_server": "tcp://console.example.invalid:22020",
  "version": "v2.6.4",
  "purge": false
}
```

## Output Modes

`plan_only` computes status and returns the planned action without changing files, services, or process state. This should be the default and should be safe to call repeatedly from UI code.

`apply` computes the same plan and executes only actions that are safe under the conservative decision table. If the plan resolves to `NeedsConfirmation` or `Reject`, `apply` must not perform partial repair.

## Safe No-Op Cases

Reconcile should return `Noop` when all of the following are true:

- Service is installed and running.
- Core and CLI binaries are present.
- Installed version matches the requested version, when a version is provided.
- Bootstrap fingerprint matches the requested token fingerprint, when a token is provided.
- Config server matches the requested config server, when a config server is provided.
- Existing `ready` status is true.

Stopped-but-otherwise-matching service is not a pure no-op. It is a repair case that may start the service if maintainers approve that behavior for `apply`.

## Repair Cases

The operation should recognize these repair categories:

| Case | Default plan | Apply behavior |
|------|--------------|----------------|
| Missing service with binaries missing | `Install` | Run existing install flow if `bootstrap_token` and `version` are present. |
| Missing service with binaries present | `Install` | Prefer existing install flow so service registration is recreated consistently. |
| Stopped service, identity/config/version match | `StartService` | Future apply may start the service without reinstalling. |
| Version mismatch, identity and config match | `Update` | Run existing update flow for the requested version. |
| Identity mismatch | `NeedsConfirmation` | Do not silently reinstall or repoint. Require explicit reinstall intent. |
| Config mismatch | `NeedsConfirmation` | Do not silently repoint a running service. Require explicit reinstall intent. |
| Identity mismatch and config mismatch | `NeedsConfirmation` | Treat as account/environment change; require explicit reinstall intent. |
| Binaries missing while service is installed and identity/config match | `Install` or `Update` | Recreate binaries through existing lifecycle flow; exact choice depends on whether target version is known. |

The words identity mismatch, version mismatch, and config mismatch should map directly to the existing status fields `identity_match`, `version_match`, and `config_server_match`.

## Decision Table

The table below defines conservative default decisions for a future pure helper. `desired known` means the request includes the fields needed to evaluate the condition.

| Current status | Desired state comparison | Purge requested | Decision | Reason |
|----------------|--------------------------|-----------------|----------|--------|
| `ready: true`, binaries present, service running | Identity match, config match, version match | `false` | `Noop` | Local service already satisfies desired state. |
| Service missing and binaries missing | Desired token and version known | `false` | `Install` | Nothing is running to repoint; use existing install flow. |
| Service missing and binaries present | Desired token and version known | `false` | `Install` | Recreate service registration using existing install flow. |
| Service installed but stopped | Identity match, config match, version match | `false` | `StartService` | Non-destructive repair; no account/config change. |
| Service installed and running | Version mismatch only | `false` | `Update` | Existing update flow is appropriate when identity/config match. |
| Service installed | Identity mismatch | `false` | `NeedsConfirmation` | Account change is operationally risky. |
| Service installed | Config mismatch | `false` | `NeedsConfirmation` | Config endpoint change is operationally risky. |
| Service installed | Identity mismatch or config mismatch | `true` | `NeedsConfirmation` | Reinstall/purge requires explicit intent and purge safety checks. |
| Any state | Desired fields are insufficient to evaluate safety | Any | `Reject` | Reconcile cannot prove the operation is safe. |
| Any state | Purge requested without approved purge safety checks | `true` | `Reject` | Destructive cleanup must follow plan 004 safety rules. |

If maintainers later add a pure decision helper, an enum such as `Noop`, `Install`, `Update`, `StartService`, `NeedsConfirmation`, and `Reject` is sufficient for this table. The helper must not touch the filesystem, service manager, network, or process state.

## Destructive Cases

The following cases require explicit confirmation or must be rejected until product semantics are approved:

- Any purge of install directories, caches, logs, or generated service data.
- Any reinstall caused by identity mismatch.
- Any reinstall caused by config mismatch.
- Any operation that would uninstall a running service before installing a replacement.
- Any operation that cannot prove the requested `install_dir` is the same managed installation.

For purge, a future implementation must apply the purge safety checks from plan 004 before deleting anything. If those checks are unavailable, the decision must be `Reject`.

## Future Events

If implemented later, reconcile should preserve the existing JSON Lines event style.

| Event | Important fields |
|-------|------------------|
| `started` | `install_dir`, `mode` |
| `status_evaluated` | Existing status summary fields, excluding raw secrets |
| `reconcile_planned` | `decision`, `reason`, `requires_confirmation`, `destructive` |
| `confirmation_required` | `decision`, `reason`, `required_confirmation` |
| `repair_started` | `decision` |
| `repair_finished` | `decision` |
| `finished` | `decision`, `applied`, `ready` |
| `error` | `code`, `message` |

## Future Error Codes

| Code | Meaning |
|------|---------|
| `invalid_request` | Request schema is invalid or required desired-state fields are missing. |
| `unsafe_reconcile` | The requested apply would require confirmation or destructive behavior. |
| `confirmation_required` | The plan is valid but cannot be applied without explicit user intent. |
| `unsupported_reconcile` | The requested decision is not implemented by the current installer version. |
| `permission_denied` | The process lacks permission to inspect or repair service state. |
| `internal_error` | Unexpected implementation failure. |

## Compatibility

The command should follow existing desktop compatibility rules: clients must ignore unknown fields and conservatively handle unknown events. If a protocol version is later introduced, reconcile should be the first command to require it only if maintainers decide the current event compatibility model is insufficient.

## Open Questions

- Should reconcile ever auto-uninstall, or should uninstall remain a separate explicit desktop command?
- What exact UI flow confirms identity mismatch or config mismatch reinstall intent?
- What should the `confirm` object contain so confirmations are explicit but not tied to localized UI strings?
- Should a stopped matching service be started automatically in `apply`, or should the desktop UI ask first?
- Should `plan_only` require `bootstrap_token`, or is a partial plan acceptable when identity cannot be evaluated?
- How should desktop protocol versioning work if reconcile introduces decisions that older clients cannot understand?
