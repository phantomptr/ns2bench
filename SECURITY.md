# Security policy

## Supported versions

Only the latest release gets security fixes.

## Reporting a vulnerability

Please report privately through
[GitHub security advisories](https://github.com/phantomptr/ns2bench/security/advisories/new).
If that isn't possible, contact PhantomPtr on [Discord](https://discord.com/channels/1464735724434624524/1571706050539815042)
without posting details publicly. You'll get an answer within a few days.

## Scope and trust model

- The engine listens on `127.0.0.1:7630` by default. Binding to another address requires
  an API token. Browser requests from other sites and DNS-rebinding host names are refused.
- The input device (NS2Bench Bridge) accepts one authenticated host on its management
  link. Keep that link private (a cable, USB network or an isolated LAN).
- The offline network service never forwards, NATs or proxies traffic and answers DNS
  only for the console's connectivity test. A change that could give the console
  internet access is a security bug.
- Desktop updates are downloaded only from this repository's GitHub releases and must
  match the SHA-256 in the release manifest.
