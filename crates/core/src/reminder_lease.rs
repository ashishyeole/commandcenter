use crate::{Repository, Result};
impl Repository {
    /// Shared by UI and scheduled processes to avoid overlapping notification checks.
    pub fn acquire_reminder_poll(&self, now: i64) -> Result<bool> {
        let n=self.conn.execute("INSERT INTO settings(key,value) VALUES('reminder_poll_lease',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE CAST(settings.value AS INTEGER)<=?2",rusqlite::params![(now+30).to_string(),now]).map_err(|e|e.to_string())?;
        Ok(n == 1)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_connections_share_the_lease() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let a = Repository::open(file.path()).unwrap();
        let b = Repository::open(file.path()).unwrap();
        assert!(a.acquire_reminder_poll(100).unwrap());
        assert!(!b.acquire_reminder_poll(110).unwrap());
        assert!(b.acquire_reminder_poll(131).unwrap());
    }
}
