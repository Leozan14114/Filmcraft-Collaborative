# Collaboration status

FilmCraft can be edited by two or more people at once, on different computers, through a relay.
**This code has not been compiled or run yet.** It was written in an environment with no Rust
toolchain and no network. Before trusting it with a real project, build it and run the tests
(see [Verification status](#verification-status)). Keep normal project backups either way.

## How it works

- A **relay** (`apps/filmcraft-relay`, logic in `crates/collab`) hosts sessions in memory. A session is
  one shared project.
- The shared project is a *document*: a map from keys to JSON values. The engine maps the project
  onto it (`crates/engine/src/collab.rs`):

  | key | holds |
  |---|---|
  | `meta` | project name, bins, settings, LUTs, transcripts, notes… |
  | `item/<id>` | one non-sequence item |
  | `seq/<id>` | a sequence without its tracks, and the order of its tracks |
  | `track/<seq>/<v\|a\|s>/<id>` | one video, audio or submix track (its clips and transitions) |
  | `track/<seq>/c/<id>` | one caption track |

- **Commands are not replicated.** They depend on each machine's selection and active sequence. After
  every command the engine compares the project with what it last reported and sends only the keys
  that really changed. Selection, zoom, playhead, panels and the undo history are not part of the
  project, so they never travel; no per-command classification is needed, and a remote change cannot
  be sent back (it is already what the relay holds).
- **The relay orders operations.** Each client sends an operation (a set of key changes) with the
  revision it was made against. The relay accepts it if no key in it changed after that revision,
  gives it the next revision and sends it to everyone (the author gets it as the acknowledgement).
  Otherwise it refuses it and names the keys.
- **Clients merge, not overwrite.** A client holds three copies: the relay's (`mirror`), its own
  (`local`) and the keys still to send. When someone else's change arrives for a key the client has
  also changed, the two are merged with a three-way merge: objects field by field, clips (and other
  lists of objects with an `id`) by id. Two people moving *different clips of the same track*, or
  editing different properties of one clip, both keep their work. When both changed the *same*
  property differently, the relay's (earlier) value is kept, the clash is counted, and the user sees a
  notice. A refused operation is simply merged and sent again; nothing is rolled back and nothing is
  dropped silently. One operation is in flight at a time.
- **Ids do not collide.** Each client allocates new ids from its own range (`client_id << 40`).
- **Undo and redo are selective** while a session is active: they put back only the parts the undone
  step changed, never a part somebody else has changed since (the user is told when parts were left
  alone). Escape-to-cancel a gesture works the same way.
- **Recovery.** Revisions must arrive consecutively; a gap makes the client ask for the whole state
  and merge it with its local edits. A client that was away reconnects with the revision it holds and
  gets only what it missed (the relay keeps the last 4096 operations), or the whole state if that is
  no longer possible. A client that cannot keep up is disconnected by the relay and catches up the
  same way.
- **Presence.** Each client sends its active sequence and playhead (about four times a second); the
  relay forwards it and the panel shows which sequence each user has open. The message and the relay
  support more (see below), but only this is drawn.

## Start a session

1. Run the relay (already set up on the NAS: `docker compose -f compose.relay.yaml up -d --build`
   after pulling this code, because the relay changed; it listens on `0.0.0.0:9001`).
2. On computer A open the project, then **Window ▸ Collaboration**. Type the *Relay* address
   (`192.168.1.10`, `relay.example.org` or `192.168.1.10:9100`; the port is 9001 when omitted and
   `ws://` is optional), your *User name* and a *Session* id (letters, digits, `-`, `_`, `.`),
   optionally a *Password*, and press **Share open project**. The relay, name and session are
   remembered in `collab.json` next to the other preferences (the password never is).

## Connect a second computer

1. Make sure the second computer can reach the relay (same Tailscale network).
2. The media must be reachable at the **same path** on every computer (shared NAS folder mounted
   identically); the project stores file paths.
3. Open FilmCraft, **Window ▸ Collaboration**, enter the same *Relay*, *Session* and *Password*, your
   own name, and press **Join session**. The shared project replaces the open one, so joining is
   refused if the open project has unsaved changes (save it, or tick *Discard unsaved changes*).
   The joined project has no file yet: use *Save As* to keep a local copy.
4. Edit. Changes appear on the other computer without saving. **Disconnect** leaves the session; the
   project stays open as a local project. Opening another project also ends the session.

If the link drops, the panel reconnects by itself (1, 2, 4, 8, then every 15 s) and resynchronizes.

## Limits (not implemented, or not guaranteed)

- **Not tested at scale.** Large timelines: every change re-serializes the changed tracks (not the whole
  project), but a very long track is sent whole each time it changes.
- **Same-property clashes** keep the earlier edit; the other user's edit of that one property is lost
  (reported, not silent). Edits to different properties or different clips merge.
- **Whole-track granularity on the wire**: two people editing one track are merged on the client, so
  heavy simultaneous editing of one track produces extra refused-and-resent operations.
- **Structural conflicts can break rules the relay cannot see.** The client checks merged tracks
  (no overlapping clips, bounds) and falls back to the other user's version when a merge would break
  them. Two simultaneous edits that nest two sequences into each other make a nest cycle; the editor
  warns, and one nest must be removed by hand.
- **Orphans**: a track created by one user while another concurrently rewrote the sequence's track
  list can leave an unused track entry on the relay; it does not appear in the project.
- **Dynamic trim (J/K/L) running**: remote changes are held until it ends.
- **Machine-specific data** stored in the project (proxy paths, scratch disks) is shared like
  everything else.
- **The relay keeps sessions in memory only.** If it restarts, sessions are gone; each client still has
  its project and can *Share open project* again.
- **Security:** no TLS (`ws://`; use Tailscale or a trusted network). The optional password is checked
  by the relay and sent in clear. Messages, keys, names, session count (256) and users per session (32)
  are bounded; a malicious *participant* can still write bad data into a session, which clients
  validate (bounds, overlaps) before installing.
- **Cursors, shared selection and "who is editing which clip"** are not drawn. The presence channel
  carries arbitrary small JSON, so they can be added without changing the protocol or the relay.
- The Italian (and other) translations of the new panel are missing; it shows English.

## Verification status

What exists: unit tests for the protocol, the merge, the relay's sequencer and the client state
machine (`crates/collab`); a real-socket test with two clients (`crates/collab/tests/net.rs`: join,
initial sync, edits both ways, no echo loop, simultaneous merge, wrong password, reconnect with
catch-up); and engine tests with two sessions through the sequencer on the demo project
(`crates/engine/src/collab_tests.rs`: lossless round trip, only changed tracks reported, both
directions, no echo, simultaneous edits, selective undo/redo, ending a session).

What was actually run: only a Python model of the merge + sequencer + client protocol (300 random
interleavings of three clients with delays and reordering all converged to the relay's state). **None
of the Rust was compiled or tested.** Run `cargo fmt`, `cargo check --workspace`,
`cargo test --workspace`, and `cargo clippy --workspace`; expect to fix some compile errors and
lints. `Cargo.lock` must be regenerated (it did not contain `tokio-tungstenite` even before this
change; `cargo build` without `--locked` updates it). Also test by hand with two real computers.

The earlier relay's `Cargo.toml` did not list `serde_json`, which its source used; the new relay has
no such issue (it depends on `filmcraft-collab`).
