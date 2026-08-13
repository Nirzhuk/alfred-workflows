# Security policy

## Supported versions

Agentflow has not published its first stable release. Until a version support
table is announced, security fixes target the latest code on the default branch
and the newest official binary release, if one exists.

Self-built forks and modified third-party binaries are maintained by their
distributors, not by the Agentflow maintainers.

## Report a vulnerability privately

Do not open a public issue for a suspected vulnerability or include exploit
details, credentials, private workflow data, or personal information in public
discussion.

Use the repository's **Security → Report a vulnerability** flow (GitHub private
vulnerability reporting). If that option is not available yet, contact the
maintainer through their GitHub profile with only a request for a private
reporting channel; do not send sensitive details until a private channel is
confirmed.

Include, when safe:

- the affected version or commit;
- operating system and architecture;
- a concise impact description;
- minimal reproduction steps or a proof of concept with secrets removed; and
- any known workaround.

The maintainers will acknowledge a valid private report, investigate it, and
coordinate disclosure according to severity and release readiness. A precise
response-time commitment will be added once the project has a staffed security
rotation.

## Security-sensitive areas

Agentflow launches authenticated local agent CLIs and may let those agents read
or modify files according to their own permissions. Treat workflow prompts and
imported workflows as executable instructions. Review them before running.

Particularly sensitive reports include:

- command or argument injection into an agent process;
- bypassing Tauri capability or filesystem boundaries;
- exposure of CLI credentials, prompts, model output, workflow data, or local
  file contents;
- webhooks binding beyond loopback without explicit user action;
- unsafe handling of imported workflows or attachments; and
- release-signing or updater compromise.

Never request or share the maintainers' signing certificates, updater private
keys, storefront credentials, or provider credentials as part of a report.
