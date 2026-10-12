use std::time::Duration;

use zeroize::Zeroizing;

pub struct DatabaseLease {
    username: Zeroizing<String>,
    password: Zeroizing<String>,
    lease_id: Zeroizing<String>,
    expires_in: Duration,
}

impl DatabaseLease {
    pub(crate) fn new(
        username: Zeroizing<String>,
        password: Zeroizing<String>,
        lease_id: Zeroizing<String>,
        expires_in: Duration,
    ) -> Self {
        Self {
            username,
            password,
            lease_id,
            expires_in,
        }
    }

    pub fn username(&self) -> &str {
        self.username.as_str()
    }

    pub fn password(&self) -> &str {
        self.password.as_str()
    }

    pub fn expires_in(&self) -> Duration {
        self.expires_in
    }

    pub(crate) fn lease_id(&self) -> &str {
        self.lease_id.as_str()
    }

    pub fn lease_id_for_audit(&self) -> &str {
        self.lease_id.as_str()
    }
}
