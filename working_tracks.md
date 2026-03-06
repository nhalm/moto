# Remaining Work

<!--
This file contains ALL remaining work items across all specs.
Read it in full at the start of each iteration.

- Pick an unblocked item (no `(blocked: ...)` annotation) and implement it
- After completing, move the item to tracks.md under the matching `## spec vX.Y` section
- Check this file for items blocked on what you just completed — remove resolved `(blocked: ...)` annotations
- Keep this file small — it should fit comfortably in context
-->

## moto-club.md bug-fix

(all items completed)

## container-system.md bug-fix

(all items completed)

## moto-bike.md bug-fix

(all items completed)

## keybox.md bug-fix

(all items completed)

## makefile.md bug-fix

(all items completed)

## testing.md bug-fix

(all items completed)

## container-system.md bug-fix (2)

(all items completed)

## garage-isolation.md bug-fix

(all items completed)

## garage-lifecycle.md bug-fix

(all items completed)

## moto-bike.md bug-fix (2)

(all items completed)

## service-deploy.md bug-fix

(all items completed)

## service-deploy.md bug-fix (2)

(all items completed)

## moto-cron.md v0.2

(all items completed)

## moto-cron.md v0.3

(all items completed)

## moto-club-websocket.md v0.2

(all items completed)

## moto-club-websocket.md v0.3

(all items completed)

## moto-wgtunnel.md v0.10

(all items completed)

## moto-cli.md v0.14

(all items completed)

## ai-proxy.md v0.2

(all items completed)

## ai-proxy.md v0.3

- Implement Anthropic → OpenAI streaming SSE response translation (event-by-event)
- Implement tool use translation between OpenAI and Anthropic formats
- Implement Gemini routing via OpenAI-compat mode (no translation, auth injection only)


## ai-proxy.md v0.4

- Implement model-based auto-routing: inspect model field and route to correct provider
- Support all provider keys stored simultaneously (no single-backend limitation)
- Remove MOTO_AI_PROXY_BACKEND env var (routing is automatic)
- Add MOTO_AI_PROXY_MODEL_MAP support for custom model prefix → provider mappings
- Return 503 per-provider when a provider key is missing (other providers still work)
- Add /v1/models endpoint returning merged model list from all configured providers

## ai-proxy.md v0.5

- Use garage SVID for auth instead of predictable garage-{id} token
- Implement passthrough path allowlist (block admin/billing endpoints, return 403 for disallowed paths)
- Implement error sanitization: wrap all errors in OpenAI error format, scrub API key material
- Use SecretString (zeroize-on-drop) for cached API keys
- Add X-Moto-Request-Id response header (correlation ID)
- Add X-Moto-Provider response header (provider that handled request)
- Implement local dev integration: ai-proxy in moto dev up startup sequence
- Implement local dev key seeding (prompt or MOTO_DEV_*_KEY env vars)
- Support --no-ai-proxy flag for moto dev up
- Implement fine-tuned model name handling (strip ft: prefix before matching)
