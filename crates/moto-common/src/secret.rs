//! Secret wrapper to prevent accidental logging of sensitive data.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A wrapper for sensitive data that prevents accidental exposure in logs.
///
/// The inner value is hidden in `Debug` and `Display` implementations,
/// showing `[REDACTED]` instead. Use `.expose()` to access the actual value.
///
/// # Example
///
/// ```
/// use moto_common::Secret;
///
/// let api_key = Secret::new("sk-secret-key-12345".to_string());
///
/// // Debug output shows [REDACTED]
/// assert_eq!(format!("{:?}", api_key), "Secret([REDACTED])");
///
/// // Access the actual value when needed
/// assert_eq!(api_key.expose(), "sk-secret-key-12345");
/// ```
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    /// Create a new secret wrapper.
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Expose the inner secret value.
    ///
    /// Use this sparingly - only when the actual value is needed.
    pub const fn expose(&self) -> &T {
        &self.0
    }

    /// Consume the wrapper and return the inner value.
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([REDACTED])")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_value() {
        let secret = Secret::new("my-secret-value");
        assert_eq!(format!("{secret:?}"), "Secret([REDACTED])");
    }

    #[test]
    fn display_hides_value() {
        let secret = Secret::new("my-secret-value");
        assert_eq!(format!("{secret}"), "[REDACTED]");
    }

    #[test]
    fn expose_reveals_value() {
        let secret = Secret::new("my-secret-value");
        assert_eq!(*secret.expose(), "my-secret-value");
    }

    #[test]
    fn into_inner_consumes() {
        let secret = Secret::new(String::from("owned-secret"));
        let value = secret.into_inner();
        assert_eq!(value, "owned-secret");
    }
}
