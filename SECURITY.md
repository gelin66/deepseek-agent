# Security Policy

This local coding agent can read and modify files, execute commands, access the
network, and use a DeepSeek API credential. Security work is not the product
roadmap, but these operating rules remain mandatory:

- never commit API keys, access tokens, session databases, or raw provider
  traffic containing secrets;
- keep credentials in the environment or the platform credential store;
- redact paths, prompts, tool output, and headers before sharing diagnostics;
- bind the local runtime API to loopback unless the user explicitly configures
  a trusted authenticated network path;
- report suspected credential exposure or unintended command/file access to
  the repository owner privately before publishing details.

The supported development state is the latest local `main` product branch once
that branch is established. Imported CodeWhale releases and removed chat/cloud
integrations are not maintained by this product line.
