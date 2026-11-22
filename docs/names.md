# Names and validation errors

Rudder returns the reason when it rejects a name or identifier. The panel displays
that API message rather than replacing it with a generic save failure; MCP callers
receive the same reason. A duplicate-name error means that name is already in use
in the relevant scope. Choose another name rather than changing its spelling rules.

## Resource names

Servers, projects, environments, services (including databases and compose stacks),
template/plugin installations, registries, Git providers, certificates, S3 destinations,
and compose project names use **1–63 lowercase ASCII letters, digits, or hyphens**.
The first character must be a letter or digit. Spaces, uppercase letters, underscores,
and accented characters are not accepted. For example, use `my-app` rather than
`My App` or `my_app`. The corresponding forms show this rule in English or Vietnamese.

## Other identifiers and display labels

These intentionally have different rules; resource-name restrictions do not apply
to every field called “Name”:

| Field | Rule |
| --- | --- |
| Domain / panel host / Access portal / Turnstile host | 1–253 ASCII characters, at least one dot, labels of 1–63 letters/digits/hyphens; no leading or trailing label hyphen, scheme, port, path, or wildcard |
| Compose service | 1–63 ASCII letters/digits/dots/underscores/hyphens; starts with a letter or digit |
| Database name and username | 1–63 ASCII letters/digits/underscores; starts with a letter |
| Environment variable key | 1–128 ASCII letters/digits/underscores; starts with a letter or underscore |
| Access department/rank slug | Trimmed and lowercased, 1–48 letters/digits/hyphens; no leading or trailing hyphen |
| Access department/rank name, rate-limit name, approved-agent name | Non-blank, at most 80 UTF-8 bytes after trimming, no control characters |
| Token name | Non-blank, at most 64 UTF-8 bytes after trimming, no control characters |
| Cache policy, alert, notification-channel name | Non-blank, at most 100 UTF-8 bytes after trimming |
| Backup job name | Non-blank, at most 100 UTF-8 bytes including surrounding whitespace |
| S3 bucket | 1–63 lowercase ASCII letters/digits/hyphens/dots |

UTF-8 byte limits are not character counts: accented letters may occupy several bytes.
User display names are not resource names; they retain their existing normalization
behavior (panel users' non-empty display names are trimmed and truncated to 80 characters).
Templates and plugins report the variable name
alongside domain/email/value rules. Existing duplicate and reserved-name protection
is unchanged, including compose projects already owned by another stack and reserved
plugin image variables.

Git branches, GitHub app/organization names, Unix login names, volume names, backup
object prefixes/filename templates, cache query/cookie names, registry usernames,
basic-auth usernames, image tags, and email addresses likewise report their own
length, character, or reserved-value restrictions. Validation does not rename inputs
or relax naming rules.
