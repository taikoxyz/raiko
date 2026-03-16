# Submit Mock Ticket Script Design

**Date:** 2026-03-16

## Goal

Add a small standalone script for sending ticket requests to an already running `mock_studio` instance, without starting or supervising the service.

## Scope

- Submit one requirement to `POST /api/tickets`
- Print the create response fields in a readable form
- Fetch the saved ticket receipt from `GET /api/tickets/:ticket_id`
- Reuse the same `STUDIO_ADDR` convention as the demo script

## Non-Goals

- No service startup
- No port probing
- No OpenRouter environment validation in the client script
- No polling loop or auto-retry

## Recommended Approach

Create a new script at `script/submit-mock-ticket.sh` instead of adding flags to `run-mock-studio-demo.sh`.

Why:

- The responsibilities stay clean: one script starts a local demo, the other talks to an existing service.
- The user can test many requirements against a long-running `mock_studio` process without process-management side effects.
- The script can stay very small and easy to modify later.

## Data Flow

1. Read `STUDIO_ADDR` or default to `127.0.0.1:4010`
2. Read the requirement from the first argument
3. `POST /api/tickets`
4. Parse `ticket_id`, `rule_id`, `status`, `base_url`, and `error`
5. `GET /api/tickets/:ticket_id`
6. Print the response and the generated rule directory path

## Testing

- Add a shell regression test that starts a tiny local HTTP responder for the two ticket endpoints
- Verify the new script:
  - does not attempt to start `mock_studio`
  - prints the parsed ticket fields
  - fetches the receipt endpoint
