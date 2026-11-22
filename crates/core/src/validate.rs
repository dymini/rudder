use crate::Error;

pub const NAME_RULE: &str = "Name must be 1 to 63 characters: lowercase ASCII letters (a-z), digits (0-9), or hyphens (-), starting with a letter or digit.";

/// Docker-safe resource name: lowercase ASCII, digits, and hyphens.
///
/// Pattern: `^[a-z0-9][a-z0-9-]{0,62}$` (1 to 63 characters).
pub fn validate_name(name: &str) -> Result<(), Error> {
    if is_valid_name(name) {
        Ok(())
    } else {
        Err(Error::InvalidName(name.to_owned()))
    }
}

pub fn is_valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 63 {
        return false;
    }
    let first = bytes[0];
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    bytes
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

/// Image references are passed as a single Docker argument, never through a shell.
pub fn validate_image_ref(image: &str) -> Result<(), Error> {
    let ok = !image.is_empty()
        && image.len() <= 512
        && !image.chars().any(|ch| {
            ch.is_whitespace()
                || ch.is_control()
                || matches!(ch, ';' | '|' | '&' | '$' | '`' | '\'' | '"' | '\\')
        });
    if ok {
        Ok(())
    } else {
        Err(Error::Validation("Image reference must be 1 to 512 UTF-8 bytes, with no whitespace, control characters, semicolons, pipes, ampersands, dollar signs, backticks, quotes, or backslashes.".into()))
    }
}

/// Environment variable names accepted by Docker.
pub fn validate_env_key(key: &str) -> Result<(), Error> {
    let bytes = key.as_bytes();
    let ok = !bytes.is_empty()
        && bytes.len() <= 128
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
    if ok {
        Ok(())
    } else {
        Err(Error::Validation(format!(
            "Environment variable name '{key}' must be 1 to 128 ASCII letters, digits, or underscores, starting with a letter or underscore."
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_short_and_max_length_names() {
        assert!(validate_name("a").is_ok());
        assert!(validate_name("web-1").is_ok());
        assert!(validate_name(&"a".repeat(63)).is_ok());
        assert!(validate_name("0db").is_ok());
    }

    #[test]
    fn rejects_empty_uppercase_underscore_and_too_long() {
        assert_eq!(validate_name(""), Err(Error::InvalidName(String::new())));
        assert!(validate_name("Web").is_err());
        assert!(validate_name("-web").is_err());
        assert!(validate_name("web_app").is_err());
        assert!(validate_name("web app").is_err());
        assert!(validate_name(&"a".repeat(64)).is_err());
    }

    #[test]
    fn naming_errors_describe_the_accepted_rule() {
        for name in [
            "".to_owned(),
            "Upper".into(),
            "under_score".into(),
            "-first".into(),
            "a".repeat(64),
            "tiếng".into(),
        ] {
            let error = validate_name(&name).unwrap_err();
            assert_eq!(error.to_string(), NAME_RULE);
            assert!(error.to_string().contains("1 to 63"));
            assert!(error
                .to_string()
                .contains("starting with a letter or digit"));
        }
        for key in ["", "1NAME", "HAS-DASH", "HAS SPACE"] {
            let error = validate_env_key(key).unwrap_err().to_string();
            assert!(error.contains("1 to 128"));
            assert!(error.contains("starting with a letter or underscore"));
        }
    }

    #[test]
    fn image_refs_reject_shell_metacharacters() {
        assert!(validate_image_ref("ghcr.io/acme/app:1.2").is_ok());
        assert!(validate_image_ref("redis:7").is_ok());
        assert!(validate_image_ref("nginx:1;rm -rf /").is_err());
        assert!(validate_image_ref("has space").is_err());
        assert!(validate_image_ref("").is_err());
    }

    #[test]
    fn env_keys_are_identifiers() {
        assert!(validate_env_key("DATABASE_URL").is_ok());
        assert!(validate_env_key("1URL").is_err());
        assert!(validate_env_key("MY-VAR").is_err());
    }
}
