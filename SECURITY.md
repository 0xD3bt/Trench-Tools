# Security Policy

## Status

LaunchDeck is an actively developed open-source project. Security hardening is ongoing, and interfaces, dependencies, and internal behavior may change over time.

Do not treat the software as security-audited or production-safe by default.

## Reporting Issues

If you discover a security issue, please do not open a public issue with full exploit details.

Instead, report it privately to the project maintainer through a private contact method you have available.

When reporting an issue, include:

- a short description of the problem
- affected files, flows, or endpoints
- reproduction steps if possible
- impact assessment
- any suggested mitigation or fix

## Scope

Security reports are especially helpful for:

- private key handling
- wallet signing flows
- dependency or supply-chain concerns
- API exposure or sensitive data leaks
- local file handling
- unsafe defaults or configuration issues

Current local-runtime areas that are especially worth reviewing:

- the Rust-hosted browser routes under `/api/*`
- engine routes under `/engine/*`
- local upload and image-library handling under `.local/launchdeck`
- persisted settings and report files under `.local/launchdeck`
- any use of `LAUNCHDECK_ENGINE_AUTH_TOKEN` for protecting engine-oriented routes

## Responsibility

This project is provided as-is under the repository license. Users are responsible for how they configure, deploy, and operate it.
