//! Protected byte storage only. A backend failure is never an empty vault.
use super::protocol::{ControlError, ControlErrorCode};
use std::sync::Mutex;

/// Implementations must replace an entry atomically or return an error.
/// The host must stay disabled when any protected-storage operation fails.
pub trait SecretStore: Send + Sync {
    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, ControlError>;
    fn write(&self, key: &str, value: &[u8]) -> Result<(), ControlError>;
    fn delete(&self, key: &str) -> Result<(), ControlError>;
}

// Serialize compound read/modify/write transactions across host service handles.
pub(crate) static TRANSACTION: Mutex<()> = Mutex::new(());
static VAULT_ACCESS: Mutex<()> = Mutex::new(());

pub(crate) fn vault_unavailable() -> ControlError {
    ControlError {
        code: ControlErrorCode::HostNotReady,
        message: "Protected storage is unavailable. LAN hosting remains disabled.".into(),
        retryable: false,
    }
}

/// Uses an explicitly compiled native backend; unsupported targets fail closed.
/// Construction does not access the vault. No startup path invokes this yet.
pub struct NativeSecretStore;

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
impl SecretStore for NativeSecretStore {
    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, ControlError> {
        let _guard = VAULT_ACCESS.lock().map_err(|_| vault_unavailable())?;
        let entry = keyring::Entry::new("com.audion.app.lan-controller", key)
            .map_err(|_| vault_unavailable())?;
        match entry.get_secret() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(vault_unavailable()),
        }
    }
    fn write(&self, key: &str, value: &[u8]) -> Result<(), ControlError> {
        let _guard = VAULT_ACCESS.lock().map_err(|_| vault_unavailable())?;
        keyring::Entry::new("com.audion.app.lan-controller", key)
            .and_then(|entry| entry.set_secret(value))
            .map_err(|_| vault_unavailable())
    }
    fn delete(&self, key: &str) -> Result<(), ControlError> {
        let _guard = VAULT_ACCESS.lock().map_err(|_| vault_unavailable())?;
        match keyring::Entry::new("com.audion.app.lan-controller", key)
            .and_then(|entry| entry.delete_credential())
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(vault_unavailable()),
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
impl SecretStore for NativeSecretStore {
    fn read(&self, _: &str) -> Result<Option<Vec<u8>>, ControlError> {
        Err(vault_unavailable())
    }
    fn write(&self, _: &str, _: &[u8]) -> Result<(), ControlError> {
        Err(vault_unavailable())
    }
    fn delete(&self, _: &str) -> Result<(), ControlError> {
        Err(vault_unavailable())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    pub struct MemoryStore(pub Mutex<HashMap<String, Vec<u8>>>);
    impl SecretStore for MemoryStore {
        fn read(&self, key: &str) -> Result<Option<Vec<u8>>, ControlError> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn write(&self, key: &str, value: &[u8]) -> Result<(), ControlError> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), ControlError> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }
    pub struct UnavailableStore;
    impl SecretStore for UnavailableStore {
        fn read(&self, _: &str) -> Result<Option<Vec<u8>>, ControlError> {
            Err(vault_unavailable())
        }
        fn write(&self, _: &str, _: &[u8]) -> Result<(), ControlError> {
            Err(vault_unavailable())
        }
        fn delete(&self, _: &str) -> Result<(), ControlError> {
            Err(vault_unavailable())
        }
    }
}
