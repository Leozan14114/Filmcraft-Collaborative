//! One collaborative session as the relay sees it: the authoritative document, the revision
//! counter, a short log of accepted operations and the people present.
//!
//! This is the *sequencer*. Operations arrive in some order; each is accepted or refused whole:
//!
//! - it is accepted when no key it touches was changed after the revision it was based on
//!   (`base_rev`). It gets the next revision and becomes part of the document;
//! - otherwise it is refused, naming the keys, and its author merges the newer values into its
//!   local copy and sends again. Nothing a client sends can overwrite a newer change unseen.
//!
//! Deleted keys keep their last revision (a tombstone) so a late write to one is still caught.
//! The `Room` is plain data with no I/O, so the whole ordering policy is unit-tested here.

use std::collections::{BTreeMap, VecDeque};

use serde_json::Value;

use crate::proto::{AppliedOp, Changes, Doc, Key, MAX_DOC_KEYS, MAX_KEYS_PER_OP, MAX_USERS, UserInfo, clean_name, color_for, secrets_equal, valid_key};

/// Accepted operations kept for clients that reconnect.
pub const LOG_LIMIT: usize = 4096;

/// Why an operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum Reject {
    /// Somebody changed these keys after the operation's base revision.
    Stale(Vec<Key>),
    /// The operation itself is not acceptable.
    Invalid(String),
}

pub struct Room {
    token: Option<String>,
    rev: u64,
    state: Doc,
    key_rev: BTreeMap<Key, u64>,
    log: VecDeque<AppliedOp>,
    users: BTreeMap<u64, UserInfo>,
    next_client: u64,
}

impl Room {
    /// A session that starts from `state`, which is revision 1.
    pub fn new(state: Doc, token: Option<String>) -> Self {
        let key_rev = state.keys().map(|k| (k.clone(), 1)).collect();
        Self { token, rev: 1, state, key_rev, log: VecDeque::new(), users: BTreeMap::new(), next_client: 1 }
    }

    /// Check a state offered by a client that wants to start a session.
    pub fn validate_initial(state: &Doc) -> Result<(), String> {
        if state.len() > MAX_DOC_KEYS {
            return Err(format!("the project has {} parts, more than the {MAX_DOC_KEYS} allowed", state.len()));
        }
        if let Some(bad) = state.keys().find(|k| !valid_key(k)) {
            return Err(format!("invalid key {bad:?}"));
        }
        Ok(())
    }

    pub fn rev(&self) -> u64 {
        self.rev
    }
    pub fn state(&self) -> &Doc {
        &self.state
    }
    pub fn users(&self) -> Vec<UserInfo> {
        self.users.values().cloned().collect()
    }

    /// Whether a client presenting `offered` may enter.
    pub fn token_ok(&self, offered: Option<&str>) -> bool {
        match (&self.token, offered) {
            (None, _) => true,
            (Some(want), Some(got)) => secrets_equal(want, got),
            (Some(_), None) => false,
        }
    }

    /// Admit a user: a new client id and colour, or `None` when the session is full.
    pub fn add_user(&mut self, name: &str) -> Option<UserInfo> {
        if self.users.len() >= MAX_USERS {
            return None;
        }
        let client_id = self.next_client;
        self.next_client += 1;
        let user = UserInfo { client_id, name: clean_name(name), color: color_for(client_id), presence: Value::Null };
        self.users.insert(client_id, user.clone());
        Some(user)
    }

    pub fn remove_user(&mut self, client_id: u64) -> bool {
        self.users.remove(&client_id).is_some()
    }

    pub fn set_presence(&mut self, client_id: u64, data: Value) -> bool {
        match self.users.get_mut(&client_id) {
            Some(u) => {
                u.presence = data;
                true
            }
            None => false,
        }
    }

    /// Decide on an operation. On success the document and revision have moved on and the returned
    /// [`AppliedOp`] is what every client must receive, in this order.
    pub fn apply(&mut self, client_id: u64, op_id: u64, base_rev: u64, changes: Changes) -> Result<AppliedOp, Reject> {
        if changes.is_empty() {
            return Err(Reject::Invalid("empty operation".into()));
        }
        if changes.len() > MAX_KEYS_PER_OP {
            return Err(Reject::Invalid(format!("operation touches {} keys, more than {MAX_KEYS_PER_OP}", changes.len())));
        }
        if base_rev > self.rev {
            return Err(Reject::Invalid(format!("operation is based on revision {base_rev}, which does not exist yet")));
        }
        for (key, value) in &changes {
            if !valid_key(key) {
                return Err(Reject::Invalid(format!("invalid key {key:?}")));
            }
            if matches!(value, Some(Value::Null)) {
                return Err(Reject::Invalid(format!("key {key:?} holds null")));
            }
        }
        let stale: Vec<Key> = changes.keys().filter(|k| self.key_rev.get(*k).is_some_and(|r| *r > base_rev)).cloned().collect();
        if !stale.is_empty() {
            return Err(Reject::Stale(stale));
        }
        let adds = changes.iter().filter(|(k, v)| v.is_some() && !self.state.contains_key(*k)).count();
        if self.state.len() + adds > MAX_DOC_KEYS {
            return Err(Reject::Invalid(format!("the project would exceed {MAX_DOC_KEYS} parts")));
        }
        self.rev += 1;
        for (key, value) in &changes {
            match value {
                Some(v) => {
                    self.state.insert(key.clone(), v.clone());
                }
                None => {
                    self.state.remove(key);
                }
            }
            self.key_rev.insert(key.clone(), self.rev);
        }
        let applied = AppliedOp { rev: self.rev, client_id, op_id, changes };
        self.log.push_back(applied.clone());
        while self.log.len() > LOG_LIMIT {
            self.log.pop_front();
        }
        Ok(applied)
    }

    /// The operations after revision `since`, if the log still reaches back that far.
    pub fn catchup(&self, since: u64) -> Option<Vec<AppliedOp>> {
        if since > self.rev {
            return None;
        }
        if since == self.rev {
            return Some(Vec::new());
        }
        let first = self.log.front()?.rev;
        if first > since + 1 {
            return None;
        }
        Some(self.log.iter().filter(|op| op.rev > since).cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Reject, Room};
    use crate::proto::{Changes, Doc};

    fn room() -> Room {
        let mut doc = Doc::new();
        doc.insert("a".into(), json!({"v": 1}));
        doc.insert("b".into(), json!({"v": 1}));
        Room::new(doc, None)
    }

    fn set(key: &str, v: i64) -> Changes {
        [(key.to_string(), Some(json!({"v": v})))].into()
    }

    #[test]
    fn accepted_operations_get_consecutive_revisions() {
        let mut r = room();
        assert_eq!(r.apply(1, 1, 1, set("a", 2)).unwrap().rev, 2);
        assert_eq!(r.apply(2, 1, 2, set("a", 3)).unwrap().rev, 3);
        assert_eq!(r.state().get("a"), Some(&json!({"v": 3})));
    }

    #[test]
    fn a_write_based_on_an_old_revision_to_a_changed_key_is_refused() {
        let mut r = room();
        r.apply(1, 1, 1, set("a", 2)).unwrap();
        let err = r.apply(2, 1, 1, set("a", 9)).unwrap_err();
        assert_eq!(err, Reject::Stale(vec!["a".to_string()]));
        // nothing moved
        assert_eq!(r.rev(), 2);
        assert_eq!(r.state().get("a"), Some(&json!({"v": 2})));
    }

    #[test]
    fn different_keys_from_the_same_base_both_succeed() {
        let mut r = room();
        r.apply(1, 1, 1, set("a", 2)).unwrap();
        assert!(r.apply(2, 1, 1, set("b", 5)).is_ok());
    }

    #[test]
    fn one_stale_key_refuses_the_whole_operation() {
        let mut r = room();
        r.apply(1, 1, 1, set("a", 2)).unwrap();
        let mut both = set("a", 7);
        both.extend(set("b", 7));
        assert!(matches!(r.apply(2, 1, 1, both), Err(Reject::Stale(_))));
        assert_eq!(r.state().get("b"), Some(&json!({"v": 1})));
    }

    #[test]
    fn deleted_keys_are_still_protected() {
        let mut r = room();
        r.apply(1, 1, 1, [("a".to_string(), None)].into()).unwrap();
        assert!(r.state().get("a").is_none());
        assert!(matches!(r.apply(2, 1, 1, set("a", 4)), Err(Reject::Stale(_))));
        assert!(r.apply(2, 2, 2, set("a", 4)).is_ok());
    }

    #[test]
    fn hostile_operations_are_invalid() {
        let mut r = room();
        assert!(matches!(r.apply(1, 1, 1, Changes::new()), Err(Reject::Invalid(_))));
        assert!(matches!(r.apply(1, 1, 99, set("a", 1)), Err(Reject::Invalid(_))));
        assert!(matches!(r.apply(1, 1, 1, set("bad\nkey", 1)), Err(Reject::Invalid(_))));
        assert!(matches!(r.apply(1, 1, 1, [("a".to_string(), Some(serde_json::Value::Null))].into()), Err(Reject::Invalid(_))));
        assert_eq!(r.rev(), 1);
    }

    #[test]
    fn catchup_returns_what_a_client_missed() {
        let mut r = room();
        r.apply(1, 1, 1, set("a", 2)).unwrap();
        r.apply(1, 2, 2, set("b", 2)).unwrap();
        assert_eq!(r.catchup(3).unwrap().len(), 0);
        assert_eq!(r.catchup(1).unwrap().len(), 2);
        assert_eq!(r.catchup(2).unwrap()[0].rev, 3);
        assert!(r.catchup(50).is_none());
    }

    #[test]
    fn users_get_distinct_ids_colours_and_the_token_is_checked() {
        let mut r = Room::new(Doc::new(), Some("secret".into()));
        assert!(r.token_ok(Some("secret")));
        assert!(!r.token_ok(Some("nope")));
        assert!(!r.token_ok(None));
        let (a, b) = (r.add_user("Ann").unwrap(), r.add_user("Bob").unwrap());
        assert_ne!(a.client_id, b.client_id);
        assert_ne!(a.color, b.color);
        assert!(r.remove_user(a.client_id));
        assert_eq!(r.users().len(), 1);
    }
}
