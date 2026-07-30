# Security Policy

## Scope

These contracts are compliance-critical primitives meant for real issuers.
In scope for a security report:

- Anything that lets a transfer go through despite the sender or recipient
  being denylisted, not allowlisted, or in a disallowed jurisdiction.
- Anything that lets a non-admin/non-issuer bypass `require_auth()` and
  mutate allowlist, denylist, or jurisdiction state.
- Anything that causes incorrect or missing compliance events, in a way
  that would hide a compliance-relevant state change from off-chain
  monitoring.

Out of scope: issues in the example contracts under `/examples` that don't
affect the primitives themselves, and purely cosmetic/documentation issues
(please file those as a regular GitHub issue instead).

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Report privately via [GitHub Security Advisories](https://github.com/stellar-compliance-kit/compliance-primitives/security/advisories/new)
for this repository. Include the affected contract(s), a description of
the issue, and steps to reproduce if possible.

## What to expect

We aim to acknowledge new reports within a few business days and to keep
you updated as we investigate and work on a fix. Once a fix is available,
we'll coordinate on disclosure timing with you before making the report
public.
