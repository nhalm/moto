# Proxy Architecture

| | |
|--------|----------------------------------------------|
| Status | Bare Frame |
| Version | 0.1 |
| Last Updated | 2026-01-19 |

## Overview

Defines the proxy layer that sits between customers and their upstream services. Handles tokenization on inbound, detokenization on outbound.

## Jobs to Be Done

- [ ] Define inbound proxy flow (data comes in, gets tokenized)
- [ ] Define outbound proxy flow (tokens go out, get detokenized)
- [ ] Define proxy deployment model
- [ ] Define TLS termination strategy
- [ ] Define performance requirements
- [ ] Define high availability approach

## Specification

_To be written_

## Notes

VGS model: customer points their traffic at the proxy. Proxy intercepts, tokenizes sensitive fields, forwards to customer's backend. On outbound (e.g., to payment processor), proxy detokenizes before sending.
