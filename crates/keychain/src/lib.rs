//! Personal access tokens in the macOS Keychain, one item per `login@host`.

use std::fmt;

const SERVICE: &str = "dev.flaticols.switchfold";

/// A Keychain failure, with a message suitable for the interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeychainError(String);

impl fmt::Display for KeychainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for KeychainError {}

fn entry(login: &str, host: &str) -> Result<keyring::Entry, KeychainError> {
    keyring::Entry::new(SERVICE, &format!("{login}@{host}"))
        .map_err(|error| KeychainError(format!("Keychain is unavailable: {error}")))
}

/// Stores `token` for the account, replacing any previous one.
pub fn store_token(login: &str, host: &str, token: &str) -> Result<(), KeychainError> {
    entry(login, host)?
        .set_password(token)
        .map_err(|error| KeychainError(format!("Couldn’t save the token: {error}")))
}

/// Reads the account's token, or `None` when none is stored.
pub fn read_token(login: &str, host: &str) -> Result<Option<String>, KeychainError> {
    match entry(login, host)?.get_password() {
        Ok(token) => Ok(Some(token)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(KeychainError(format!("Couldn’t read the token: {error}"))),
    }
}

/// Deletes the account's token; succeeds when none is stored.
pub fn delete_token(login: &str, host: &str) -> Result<(), KeychainError> {
    match entry(login, host)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(KeychainError(format!("Couldn’t delete the token: {error}"))),
    }
}
