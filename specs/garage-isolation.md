# Garage Isolation

| | |
|--------|----------------------------------------------|
| Version | 0.1 |
| Last Updated | 2026-01-19 |

## Overview

Defines the isolation model for the garage environment. Network policies, resource limits, secrets handling - everything that keeps Claude Code safe to run in YOLO mode.

## Specification

_To be written_

## Notes

Key principle: Claude Code runs in YOLO mode inside the garage. Must be safe to let it run in loops making changes. Isolation means:
- Can't damage host
- Can't leak real secrets
- Can't access production systems
- Has internet for packages/docs
- Has everything needed to build/test
