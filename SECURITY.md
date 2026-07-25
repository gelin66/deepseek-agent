# Security Policy

DSE can read and modify workspace files, execute commands, access the network,
and use an official DeepSeek API credential. Treat every installation and
diagnostic artifact as security-sensitive.

## Supported versions

DSE has not published its first public release. Before that release, only the
current repository default branch is in security-support scope. Imported
CodeWhale tags, historical checkpoints, deleted provider/chat integrations,
and local forks are not supported DSE releases.

After public releases begin, this file will list the supported release series
explicitly. Do not infer support from an old tag or frozen evaluation artifact.

## Report a vulnerability privately

Use this repository's **Private vulnerability reporting** / **Report a
vulnerability** flow to create a private GitHub security advisory. Include:

- affected revision or version;
- impact and realistic attack prerequisites;
- minimal reproduction steps;
- whether credentials, files, command execution, sandbox boundaries, or the
  local Run API are involved;
- a proposed disclosure timeline, if relevant.

Do not include a real API key, access token, private repository content, raw
provider transcript, or unredacted local path. If GitHub private vulnerability
reporting is unavailable, contact the repository owner privately through the
owner profile and ask for a secure channel without disclosing vulnerability
details in a public issue.

For conduct incidents rather than software vulnerabilities, use the same
private contact boundary and identify the report as a Code of Conduct matter.

## Security expectations

- Store DeepSeek credentials through `dse login`, the platform credential
  store, or a process-local `DEEPSEEK_API_KEY`; never commit them.
- Keep `.env`, `key.txt`, local Run databases, raw evaluation journals,
  provider payloads, logs, generated packages, and external Cargo targets
  untracked.
- Bind the HTTP Run API to loopback and require `--auth-token` or
  `DSE_APP_SERVER_TOKEN`. Use `--insecure-no-auth` only for an explicit
  loopback-only local experiment.
- Redact prompts, file paths, tool arguments/output, HTTP headers, URLs with
  embedded credentials, and model payloads before sharing diagnostics.
- Do not test destructive commands, sandbox escape, credential extraction, or
  denial of service against systems or data you do not own or have explicit
  permission to test.

## Response process

Maintainers will validate the report, establish affected revisions, coordinate
a fix and regression coverage, and agree on disclosure timing. A fix is not
complete until the relevant deterministic tests, replay/crash behavior, and
release artifact checks pass.

No response-time or bounty commitment is implied before the first public
release. Reporters acting in good faith and within the testing boundaries above
will be credited when they want attribution and coordinated disclosure permits
it.
