use reqwest::RequestBuilder;
use serde::Deserialize;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A Vault token.
///
/// It cannot be cloned, only moved, and its memory is overwritten when it is
/// dropped. It travels from where it was read (command line, environment,
/// config file, token cache) through `Config` into the `AuthManager`, which
/// keeps it for the life of the client.
#[derive(Default, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
pub struct Token(String);

impl Token {
    /// True if there is no token
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The token itself. Whoever copies it takes over the wiping
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl From<String> for Token {
    fn from(token: String) -> Self {
        Self(token)
    }
}

// Custom Debug to avoid leaking tokens
impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("***REDACTED***")
    }
}

/// The one owner of the token while the client lives.
///
/// The client, the KV engine and the policy API share it behind an `Arc`;
/// none of them has a token of its own. The token is wiped when the last of
/// them is dropped.
#[derive(Debug)]
pub struct AuthManager {
    token: Token,
}

impl AuthManager {
    /// Create a new authentication manager
    pub fn new(token: Token) -> Self {
        Self { token }
    }

    /// Add authentication header to request
    ///
    /// The HTTP library copies the token into the request's headers; that
    /// copy is not wiped (see the security notes in the README).
    pub fn add_auth_header(&self, request: RequestBuilder) -> RequestBuilder {
        request.header("X-Vault-Token", self.token.expose())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_never_shows_the_token() {
        let token = Token::from("hvs.SECRET".to_string());
        assert_eq!(format!("{token:?}"), "***REDACTED***");

        let auth = AuthManager::new(token);
        assert!(!format!("{auth:?}").contains("hvs.SECRET"));
    }

    #[test]
    fn token_is_wiped() {
        let mut token = Token::from("hvs.SECRET".to_string());

        token.zeroize();

        assert!(token.is_empty());
    }

    #[test]
    fn token_is_wiped_when_dropped() {
        // Fails to compile if the token stops being wiped on drop
        fn wiped_on_drop<T: ZeroizeOnDrop>() {}
        wiped_on_drop::<Token>();
    }
}
