use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Read,
    Admin,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Admin => "admin",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Self::Read),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }

    /// Read tokens can list the catalog and ask for a prep list.
    /// Admin tokens can do that and also change data or keys.
    pub fn allows(self, required: Self) -> bool {
        match required {
            Self::Read => true,
            Self::Admin => matches!(self, Self::Admin),
        }
    }
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn token_matches(presented: &str, expected_hash: &str) -> bool {
    let actual = hash_token(presented);
    constant_eq(actual.as_bytes(), expected_hash.as_bytes())
}

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).expect("operating system random number generator");
    format!("mc_{}", hex::encode(bytes))
}

fn constant_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.iter().zip(right) {
        diff |= a ^ b;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_and_not_the_token() {
        let token = "mc_example_token_value";
        let hash = hash_token(token);
        assert_eq!(hash, hash_token(token));
        assert_ne!(hash, token);
        assert_eq!(hash.len(), 64);
        assert!(token_matches(token, &hash));
        assert!(!token_matches("other-token-value", &hash));
    }

    #[test]
    fn read_scope_cannot_administer() {
        assert!(Scope::Read.allows(Scope::Read));
        assert!(!Scope::Read.allows(Scope::Admin));
        assert!(Scope::Admin.allows(Scope::Read));
        assert!(Scope::Admin.allows(Scope::Admin));
    }
}
