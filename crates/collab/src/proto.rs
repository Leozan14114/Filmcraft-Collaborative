//! The wire protocol between FilmCraft clients and the relay.
//!
//! A shared project is a flat *document*: a map from string keys (`meta`, `item/12`,
//! `track/7/v/9`…) to JSON values. The relay never looks inside a value. Every change is an
//! [`Changes`] map (a key and its new value, or `None` to delete it) sent as one atomic operation
//! that names the revision it was made against (`base_rev`). The relay orders operations and gives
//! each accepted one the next revision; see [`crate::room::Room`].
//!
//! All messages are JSON text frames, tagged by `type`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Bumped on any incompatible change to this module.
pub const PROTOCOL_VERSION: u32 = 1;
/// Longest key the relay accepts.
pub const MAX_KEY_LEN: usize = 256;
/// Most keys one operation may touch.
pub const MAX_KEYS_PER_OP: usize = 20_000;
/// Most keys a shared document may hold.
pub const MAX_DOC_KEYS: usize = 200_000;
/// Longest user name (characters).
pub const MAX_NAME_CHARS: usize = 48;
/// Longest session id (characters).
pub const MAX_SESSION_CHARS: usize = 64;
/// Most users in one session.
pub const MAX_USERS: usize = 32;

/// Port used when the relay address does not name one.
pub const DEFAULT_PORT: u16 = 9001;

pub type Key = String;
/// The shared document.
pub type Doc = BTreeMap<Key, Value>;
/// One operation's changes. `None` deletes the key. A present value is never JSON `null` (that
/// would not survive the round trip as `Some`); documents hold objects.
pub type Changes = BTreeMap<Key, Option<Value>>;

/// Someone in the session.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UserInfo {
    pub client_id: u64,
    pub name: String,
    /// `#rrggbb`, assigned by the relay so every machine draws a user the same way.
    pub color: String,
    /// What the user is looking at (active sequence, playhead…). Opaque to the relay; relayed to
    /// the others and kept for late joiners. Cursors and shared selection will travel here.
    #[serde(default)]
    pub presence: Value,
}

/// An accepted operation, as the relay logs and broadcasts it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AppliedOp {
    pub rev: u64,
    pub client_id: u64,
    pub op_id: u64,
    pub changes: Changes,
}

/// Client to relay.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ClientMsg {
    /// First message of a client that shares its project as a new session.
    Create { protocol: u32, session: String, name: String, token: Option<String>, state: Doc },
    /// First message of a client that joins a session. `last_rev` is set when it reconnects and
    /// still holds the document as of that revision.
    Join { protocol: u32, session: String, name: String, token: Option<String>, last_rev: Option<u64> },
    /// A change made against revision `base_rev`.
    Op { op_id: u64, base_rev: u64, changes: Changes },
    /// Ask for the whole current document (after a gap or a failed merge).
    RequestState,
    /// Where this user is looking.
    Presence { data: Value },
    /// Leave the session.
    Leave,
}

/// Relay to client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ServerMsg {
    /// The whole document at revision `rev`.
    Welcome { session: String, client_id: u64, rev: u64, state: Doc, users: Vec<UserInfo> },
    /// A reconnecting client is only missing these operations (in order).
    Catchup { session: String, client_id: u64, rev: u64, ops: Vec<AppliedOp>, users: Vec<UserInfo> },
    /// An operation was accepted (also sent to its author, as the acknowledgement).
    Applied { rev: u64, client_id: u64, op_id: u64, changes: Changes },
    /// The author's operation was refused; `keys` were changed by someone else first (or the
    /// operation was invalid, `reason` says). The author merges and sends again.
    Rejected { op_id: u64, rev: u64, reason: String, keys: Vec<Key> },
    UserJoin { user: UserInfo },
    UserLeave { client_id: u64 },
    Presence { client_id: u64, data: Value },
    /// The relay could not keep up with this client; it must ask for the state again.
    ResyncRequired,
    /// The request failed; the connection is closed after it when `fatal`.
    Error { message: String, fatal: bool },
}

/// Colours given to users in turn.
const PALETTE: [&str; 10] = ["#e6194b", "#3cb44b", "#4363d8", "#f58231", "#911eb4", "#008080", "#f032e6", "#9a6324", "#808000", "#469990"];

/// The colour for the n-th user to join.
pub fn color_for(client_id: u64) -> String {
    let n = PALETTE.len() as u64;
    let index = (client_id.saturating_sub(1) % n) as usize;
    PALETTE.get(index).copied().unwrap_or("#888888").to_string()
}

/// Whether `s` can name a session: 1 to 64 characters of letters, digits, `-`, `_` and `.`.
pub fn valid_session_id(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= MAX_SESSION_CHARS && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A user name made safe to show: control characters removed, trimmed, cut to the limit.
pub fn clean_name(s: &str) -> String {
    let cleaned: String = s.chars().filter(|c| !c.is_control()).collect();
    let cut: String = cleaned.trim().chars().take(MAX_NAME_CHARS).collect();
    if cut.is_empty() { "Guest".to_string() } else { cut }
}

/// A key the relay accepts.
pub fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.len() <= MAX_KEY_LEN && k.chars().all(|c| !c.is_control())
}

/// Compare two secrets without stopping at the first difference.
pub fn secrets_equal(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= usize::from(x ^ y);
    }
    diff == 0
}

/// A relay address as people type it, made into the `ws://host:port` the connection needs.
///
/// Accepted: `192.168.1.10`, `192.168.1.10:9001`, `relay.example.org`, `ws://relay:9100`, `[::1]`,
/// `[::1]:9100`. The port defaults to [`DEFAULT_PORT`] (like Minecraft's 25565) and `ws://` is
/// optional. `wss://` is refused: the relay speaks plain WebSocket.
pub fn relay_url(input: &str) -> Result<String, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("Enter the relay address.".into());
    }
    let rest = if s.len() >= 5 && s.is_char_boundary(5) && s[..5].eq_ignore_ascii_case("ws://") {
        &s[5..]
    } else if s.len() >= 6 && s.is_char_boundary(6) && s[..6].eq_ignore_ascii_case("wss://") {
        return Err("wss:// is not supported: the relay speaks plain ws:// (use a private network such as Tailscale).".into());
    } else if s.contains("://") {
        return Err("Type just the address, for example 192.168.1.10 or myrelay:9001.".into());
    } else {
        s
    };
    let rest = rest.trim_end_matches('/');
    let (host, port_text) = if let Some(inner) = rest.strip_prefix('[') {
        let (h, after) = inner.split_once(']').ok_or("The address has a '[' without a ']'.")?;
        let port = if after.is_empty() { None } else { Some(after.strip_prefix(':').ok_or("Unexpected text after ']' in the address.")?) };
        (format!("[{h}]"), port)
    } else if rest.matches(':').count() > 1 {
        // an IPv6 address typed without brackets
        (format!("[{rest}]"), None)
    } else {
        match rest.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (rest.to_string(), None),
        }
    };
    let bracketed = host.len() > 2 && host.starts_with('[') && host.ends_with(']');
    let named = !host.is_empty() && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    let host_ok = bracketed || named;
    if !host_ok {
        return Err("The relay address is not valid (use a name or an IP address).".into());
    }
    let port = match port_text {
        None => DEFAULT_PORT,
        Some(p) => p.parse::<u16>().ok().filter(|p| *p != 0).ok_or("The port must be a number from 1 to 65535.")?,
    };
    Ok(format!("ws://{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::{ClientMsg, ServerMsg, clean_name, relay_url, secrets_equal, valid_key, valid_session_id};

    #[test]
    fn relay_addresses_are_forgiving() {
        assert_eq!(relay_url("192.168.1.10").unwrap(), "ws://192.168.1.10:9001");
        assert_eq!(relay_url("  192.168.1.10:9100 ").unwrap(), "ws://192.168.1.10:9100");
        assert_eq!(relay_url("ws://relay.example.org").unwrap(), "ws://relay.example.org:9001");
        assert_eq!(relay_url("WS://relay:7000/").unwrap(), "ws://relay:7000");
        assert_eq!(relay_url("[::1]").unwrap(), "ws://[::1]:9001");
        assert_eq!(relay_url("[::1]:9100").unwrap(), "ws://[::1]:9100");
        assert_eq!(relay_url("fe80::1").unwrap(), "ws://[fe80::1]:9001");
    }

    #[test]
    fn bad_relay_addresses_are_refused_with_a_reason() {
        assert!(relay_url("").is_err());
        assert!(relay_url("   ").is_err());
        assert!(relay_url("wss://relay").unwrap_err().contains("wss://"));
        assert!(relay_url("http://relay").is_err());
        assert!(relay_url("relay:0").is_err());
        assert!(relay_url("relay:70000").is_err());
        assert!(relay_url("relay:abc").is_err());
        assert!(relay_url("bad host").is_err());
        assert!(relay_url("[::1").is_err());
        assert!(relay_url(":9001").is_err());
    }

    #[test]
    fn messages_round_trip_as_tagged_json() {
        let m = ClientMsg::Op { op_id: 3, base_rev: 9, changes: [("a".to_string(), Some(serde_json::json!({"x": 1}))), ("b".to_string(), None)].into() };
        let text = serde_json::to_string(&m).unwrap();
        assert!(text.contains("\"type\":\"Op\""));
        assert_eq!(serde_json::from_str::<ClientMsg>(&text).unwrap(), m);
        let s = ServerMsg::ResyncRequired;
        assert_eq!(serde_json::from_str::<ServerMsg>(&serde_json::to_string(&s).unwrap()).unwrap(), s);
    }

    #[test]
    fn hostile_text_is_not_a_message() {
        assert!(serde_json::from_str::<ClientMsg>("{\"type\":\"Nope\"}").is_err());
        assert!(serde_json::from_str::<ClientMsg>("[1,2]").is_err());
    }

    #[test]
    fn validation_helpers() {
        assert!(valid_session_id("film-1.cut_2"));
        assert!(!valid_session_id(""));
        assert!(!valid_session_id("a b"));
        assert!(!valid_session_id(&"x".repeat(65)));
        assert!(valid_key("track/1/v/2"));
        assert!(!valid_key(""));
        assert!(!valid_key("a\nb"));
        assert_eq!(clean_name("  \u{7}Ann  "), "Ann");
        assert_eq!(clean_name("   "), "Guest");
        assert!(secrets_equal("abc", "abc"));
        assert!(!secrets_equal("abc", "abd"));
        assert!(!secrets_equal("abc", "abcd"));
    }
}
