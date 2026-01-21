# Moto CLI

| | |
|--------|----------------------------------------------|
| Status | Bare Frame |
| Version | 0.1 |
| Last Updated | 2026-01-19 |

## Overview

Defines the `moto` command-line interface - commands, subcommands, arguments, and user experience.

## Jobs to Be Done

- [ ] Define command hierarchy
- [ ] Define garage subcommands (open, enter, sync, close)
- [ ] Define cluster management commands
- [ ] Define output formats and verbosity
- [ ] Define configuration file format (if any)

## Specification

_To be written_

## Notes

CLI is written in Rust. Primary commands:
- `moto garage open` - spin up isolated environment
- `moto garage enter` - enter the garage (Claude Code ready)
- `moto garage sync` - sync code changes out
- `moto garage close` - tear down
