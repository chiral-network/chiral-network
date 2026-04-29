// [SPEC]
// Unit: src-tauri/src/lib.rs
//
// publish_site_to_directory(state, site_id, name, description, owner_wallet)
//   -> Result<SiteDirectoryEntry, String>
//
// Pre-condition:
//   - state holds a running DHT service.
//   - A locally-owned site with id == site_id exists and has at least one
//     of cdn_url or relay_url set.
//
// Post-condition (intended contract):
//   - On success the per-name DHT record at key `chiral_sitename_<name>`
//     stores a SiteDirectoryEntry that points to the caller's site.
//   - The first wallet to publish a given name owns it. After a
//     successful claim by wallet W, no peer other than W can replace the
//     per-name record's contents. In particular, any DHT record at
//     `chiral_sitename_<name>` whose authenticated origin is not W must
//     be rejected by any peer reading the directory.
//   - The name is one of the strings produced by validate_site_name
//     (lowercase ASCII, [a-z0-9-], 1..=63 chars, no leading/trailing '-').
// [SPEC]

// [INFO]
// validate_site_name(name) -> Result<String, String>
//   Pre-condition: any &str.
//   Post-condition: returns Ok(normalised) where normalised is lowercase
//     and matches [a-z0-9-]{1,63} with no leading/trailing '-'; otherwise
//     Err with a human-readable reason.
// [SPLIT]
// dht.put_dht_value(key, value) -> Result<(), String>
//   Pre-condition: DHT is running.
//   Post-condition: stores value at key in the Kademlia DHT.
//   Caller-required guarantee: this function as currently implemented
//   does NOT authenticate the writer against any prior claim — callers
//   that need ownership semantics must enforce them at a higher layer.
// [INFO]
