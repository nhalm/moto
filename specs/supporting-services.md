# Supporting Services

| | |
|--------|----------------------------------------------|
| Status | Bare Frame |
| Version | 0.1 |
| Last Updated | 2026-01-19 |

## Overview

Defines the supporting services (Postgres, Redis, etc.) that run inside the garage to support development and testing.

## Jobs to Be Done

- [ ] Define which services are included
- [ ] Define how services are deployed (helm charts, raw manifests)
- [ ] Define service configuration
- [ ] Define data persistence (ephemeral vs persistent)
- [ ] Define how to add new services

## Specification

_To be written_

## Notes

Initial services:
- PostgreSQL
- Redis

Services are scoped to garage namespace. Easy to add more as needed.
