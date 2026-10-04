use keyring_core::{Entry, get_default_store, set_default_store, unset_default_store};
use papaya::HashMap;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;
use zeroize::Zeroizing;
use crate::error::{SecurityError};

pub struct ApiKeyManager {
    key_map: HashMap<String, Arc<SecretString>>,
}

impl ApiKeyManager {
    fn init() {
        #[cfg(target_os = "macos")]
        {
            use apple_native_keyring_store::Store;
            set_default_store(Store::new().unwarp());
        }
        #[cfg(target_os = "windows")]
        {
            use windows_native_keyring_store::Store;
            set_default_store(Store::new().unwrap());
        }
        #[cfg(target_os = "linux")]
        {
            use linux_keyutils_keyring_store::Store;
            set_default_store(Store::new().unwrap());
        }
    }

    fn new()->Result<Self,SecurityError>{
        Self::init();
        Ok(ApiKeyManager{key_map: HashMap::new()})
    }

    fn get_key(self:&Self, service: &str, user: &str)-> keyring_core::Result<Arc<SecretString>>{
        let cache_key: String = format!("apikey-{}-{}", service,user);
        let guard = self.key_map.pin();
        if let Some(secret_apikey) = guard.get(&cache_key){
            return Ok(secret_apikey.clone());
        }
        let apikey = Entry::new(service, user)?.get_password()?;
        let secret = Arc::new(SecretString::new(apikey.into()));
        let cached_secret_apikey = guard.get_or_insert_with(cache_key, || secret);
        Ok(cached_secret_apikey.clone())
    }

    fn set_key(self:&Self, service: &str, user: &str, secret: &Arc<SecretString>)-> Result<(),SecurityError> {
        let cache_key: String = format!("apikey-{}-{}", service,user);
        let guard = self.key_map.pin();
        let apikey = Entry::new(service,user)?.set_password(secret.clone().expose_secret())?;
        Ok(())
    }
}
