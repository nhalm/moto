# Moto Cron

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Last Updated | 2026-01-21 |

## Overview

Scheduled tasks running on the cluster as Kubernetes CronJobs. Handles time-based operations like TTL cleanup for expired garages.

## Specification

_To be written_

## Notes

- Runs as K8s CronJobs, not as part of moto-club
- TTL cleanup checks for expired garages and tears them down
- Should be lightweight and idempotent
