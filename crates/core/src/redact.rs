//! Masking secrets in text shown to people (logs, errors, build output).
//!
//! Two rules keep the mask from eating ordinary words:
//! - a value is only masked where it stands as a whole token, so a secret
//!   `auto` never turns `automatic` into `***matic`;
//! - values taken from environment variables are only treated as secrets when
//!   the key says so or the value looks random ([`env_secrets`]), so
//!   `ENABLED=true` does not hide every `true` in the logs.

const MASK: &str = "***";

/// Mask every whole-token occurrence of each secret in `text`.
///
/// Secrets shorter than 4 bytes, booleans, `null`, and short numbers are
/// skipped: masking them would hide ordinary output and protect nothing.
/// Longer secrets are applied first, so one secret containing another is
/// masked in full.
pub fn redact(text: &str, secrets: &[&str]) -> String {
    let mut list: Vec<&str> = secrets
        .iter()
        .copied()
        .filter(|secret| maskable(secret))
        .collect();
    list.sort_by_key(|secret| std::cmp::Reverse(secret.len()));
    list.dedup();
    let mut out = text.to_owned();
    for secret in list {
        out = replace_tokens(&out, secret);
    }
    out
}

/// The values of `env` worth masking: those under a key that names a secret
/// (`*_PASSWORD`, `*_TOKEN`, `*_KEY`, `DATABASE_URL`, …), values that look
/// random, and passwords embedded in URLs (`scheme://user:password@host`).
/// Configuration such as `LOG_LEVEL=info`, `NODE_ENV=production` or
/// `FEATURE=true` is left out.
pub fn env_secrets<K: AsRef<str>, V: AsRef<str>>(env: &[(K, V)]) -> Vec<String> {
    let mut out = Vec::new();
    for (key, value) in env {
        let (key, value) = (key.as_ref(), value.as_ref());
        if let Some(password) = url_password(value) {
            out.push(password.to_owned());
        }
        if maskable(value) && (secret_key(key) || looks_random(value)) {
            out.push(value.to_owned());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn maskable(value: &str) -> bool {
    if value.len() < 4 || value.trim().is_empty() {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "true" | "false" | "null" | "none" | "undefined" | "yes"
    ) {
        return false;
    }
    // Ports, counts and sizes; a numeric secret this short protects nothing.
    !(value.len() < 8 && value.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Keys whose value is a credential regardless of how it looks.
fn secret_key(key: &str) -> bool {
    const WORDS: &[&str] = &[
        "PASSWORD",
        "PASSWD",
        "PASS",
        "PWD",
        "PW",
        "SECRET",
        "SECRETS",
        "TOKEN",
        "TOKENS",
        "KEY",
        "KEYS",
        "APIKEY",
        "PRIVATE",
        "CREDENTIAL",
        "CREDENTIALS",
        "AUTH",
        "SALT",
        "PEPPER",
        "SIGNATURE",
        "COOKIE",
        "SESSION",
        "DSN",
        "CERT",
        "PEM",
        "WEBHOOK",
        "PASSPHRASE",
        "BEARER",
    ];
    let key = key.to_ascii_uppercase();
    if matches!(
        key.as_str(),
        "DATABASE_URL" | "REDIS_URL" | "MONGO_URL" | "MONGODB_URI" | "AMQP_URL"
    ) {
        return true;
    }
    key.split(|c: char| !c.is_ascii_alphanumeric()).any(|part| {
        WORDS.contains(&part)
            || part.ends_with("PASSWORD")
            || part.ends_with("TOKEN")
            || part.ends_with("SECRET")
    })
}

/// A value with no spaces that mixes character kinds like a generated
/// password, key, or token does.
fn looks_random(value: &str) -> bool {
    if value.len() < 12 || value.chars().any(char::is_whitespace) {
        return false;
    }
    if value.contains("://") || value.starts_with('/') || value.starts_with("./") {
        return false;
    }
    let lower = value.bytes().any(|b| b.is_ascii_lowercase());
    let upper = value.bytes().any(|b| b.is_ascii_uppercase());
    let digit = value.bytes().any(|b| b.is_ascii_digit());
    let other = value.bytes().any(|b| !b.is_ascii_alphanumeric());
    let kinds = [lower, upper, digit, other]
        .iter()
        .filter(|kind| **kind)
        .count();
    let hex = value.len() >= 24 && value.bytes().all(|b| b.is_ascii_hexdigit());
    hex || (kinds >= 3 && digit) || (value.len() >= 20 && kinds >= 2 && digit)
}

fn url_password(value: &str) -> Option<&str> {
    let rest = &value[value.find("://")? + 3..];
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    let userinfo = &authority[..authority.rfind('@')?];
    let password = &userinfo[userinfo.find(':')? + 1..];
    (!password.is_empty()).then_some(password)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn replace_tokens(text: &str, secret: &str) -> String {
    let first = secret.chars().next().is_some_and(is_word);
    let last = secret.chars().next_back().is_some_and(is_word);
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut from = 0;
    while let Some(found) = text[from..].find(secret) {
        let start = from + found;
        let end = start + secret.len();
        let before_ok = !first || !text[..start].chars().next_back().is_some_and(is_word);
        let after_ok = !last || !text[end..].chars().next().is_some_and(is_word);
        if before_ok && after_ok {
            out.push_str(&text[copied..start]);
            out.push_str(MASK);
            copied = end;
            from = end;
        } else {
            from = start + secret.chars().next().map_or(1, char::len_utf8);
        }
    }
    out.push_str(&text[copied..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_whole_tokens_only() {
        assert_eq!(redact("automatic auto", &["auto"]), "automatic ***");
        assert_eq!(redact("pdfjs-dist", &["pdfjs"]), "***-dist");
        assert_eq!(redact("token=s3cr3t;", &["s3cr3t"]), "token=***;");
        assert_eq!(redact("xs3cr3t", &["s3cr3t"]), "xs3cr3t");
        assert_eq!(redact("tok-'$x'-value", &["tok-'$x'-value"]), "***");
        assert_eq!(redact("Bearer a.b.c", &["a.b.c"]), "Bearer ***");
    }

    #[test]
    fn symbol_edges_match_inside_words() {
        assert_eq!(redact("x=-p@ss!y", &["-p@ss!"]), "x=***y");
    }

    #[test]
    fn skips_trivial_values() {
        let text = r#"{"ok":true,"port":5432,"x":null}"#;
        assert_eq!(redact(text, &["true", "5432", "null", "on"]), text);
        assert_eq!(redact("pin 12345678", &["12345678"]), "pin ***");
    }

    #[test]
    fn longer_secrets_first() {
        assert_eq!(redact("abcd-efgh", &["abcd", "abcd-efgh"]), "***");
    }

    #[test]
    fn env_secrets_pick_credentials_not_config() {
        let env = [
            ("NODE_ENV", "production"),
            ("LOG_LEVEL", "info"),
            ("OCR_ENABLED", "true"),
            ("PDF_ENGINE", "pdfjs"),
            ("MODE", "auto"),
            ("PUBLIC_URL", "https://example.com/app"),
            ("POSTGRES_PASSWORD", "hunter22"),
            ("API_TOKEN", "abcd"),
            ("STRIPE_KEY", "sk_live_51Hx"),
            ("SOMETHING", "Zx9!qP2#mL7v"),
            ("SHA", "0123456789abcdef0123456789abcdef"),
            ("DATABASE_URL", "postgres://app:pw-9f8e7d@db:5432/app"),
            ("CACHE", "redis://:r3d1s-pass@cache:6379"),
        ];
        let got = env_secrets(&env);
        for secret in [
            "hunter22",
            "abcd",
            "sk_live_51Hx",
            "Zx9!qP2#mL7v",
            "0123456789abcdef0123456789abcdef",
            "postgres://app:pw-9f8e7d@db:5432/app",
            "pw-9f8e7d",
            "r3d1s-pass",
        ] {
            assert!(got.iter().any(|s| s == secret), "missing {secret}: {got:?}");
        }
        for plain in [
            "production",
            "info",
            "true",
            "pdfjs",
            "auto",
            "https://example.com/app",
        ] {
            assert!(!got.iter().any(|s| s == plain), "masked {plain}");
        }
    }

    #[test]
    fn secret_keys() {
        for key in [
            "DB_PASSWORD",
            "GITHUB_TOKEN",
            "AWS_SECRET_ACCESS_KEY",
            "SENTRY_DSN",
            "JWT_SECRET",
            "REFRESHTOKEN",
        ] {
            assert!(secret_key(key), "{key}");
        }
        for key in [
            "NODE_ENV",
            "PORT",
            "KEYBOARD_LAYOUT",
            "PASSTHROUGH",
            "MONKEY",
        ] {
            assert!(!secret_key(key), "{key}");
        }
    }
}
