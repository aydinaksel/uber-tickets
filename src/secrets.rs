use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::path::PathBuf;

const GMAIL_APP_PASSWORD_PATH: &str = "/run/secrets/gmail-app-password";
const R2_CREDENTIALS_PATH: &str = "/run/secrets/r2-credentials";

pub struct GmailCredentials {
    pub username: String,
    pub app_password: String,
}

pub struct ObjectStorage {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub endpoint: String,
    pub bucket: String,
    pub object_key: String,
}

pub struct Configuration {
    pub gmail: GmailCredentials,
    pub object_storage: ObjectStorage,
    pub vault_directory: PathBuf,
}

impl Configuration {
    pub fn load() -> Result<Self> {
        let app_password = std::fs::read_to_string(GMAIL_APP_PASSWORD_PATH)
            .with_context(|| format!("reading {GMAIL_APP_PASSWORD_PATH}"))?;
        let credentials = parse_environment_file(R2_CREDENTIALS_PATH)?;

        Ok(Self {
            gmail: GmailCredentials {
                username: read_environment("GMAIL_USERNAME")?,
                app_password: app_password.trim().to_owned(),
            },
            object_storage: ObjectStorage {
                access_key_id: take(&credentials, "R2_ACCESS_KEY_ID")?,
                secret_access_key: take(&credentials, "R2_SECRET_ACCESS_KEY")?,
                endpoint: take(&credentials, "R2_ENDPOINT")?,
                bucket: take(&credentials, "R2_BUCKET")?,
                object_key: take(&credentials, "R2_OBJECT_KEY")?,
            },
            vault_directory: PathBuf::from(read_environment("VAULT_DIRECTORY")?),
        })
    }
}

fn read_environment(key: &str) -> Result<String> {
    std::env::var(key).with_context(|| format!("environment variable {key} is not set"))
}

fn parse_environment_file(path: &str) -> Result<HashMap<String, String>> {
    let contents = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let entries = contents
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect();
    Ok(entries)
}

fn take(credentials: &HashMap<String, String>, key: &str) -> Result<String> {
    credentials
        .get(key)
        .cloned()
        .ok_or_else(|| anyhow!("{key} missing from {R2_CREDENTIALS_PATH}"))
}
