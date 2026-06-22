# Security Policy

## Reporting a vulnerability

Please report security issues **privately** via GitHub's
[private vulnerability reporting](https://github.com/conglei/joblode/security/advisories/new)
rather than opening a public issue. We aim to acknowledge reports within a few days.

## Operational notes

- The server is intended to run **locally** and bind to `127.0.0.1`.
- The MCP-over-HTTP endpoint is a tool surface — do not expose it on a public interface without
  authentication.
- API keys (e.g. for a ranking model) are read from the environment and are never written to disk or logs.
