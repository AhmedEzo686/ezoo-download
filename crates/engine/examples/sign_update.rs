//! Used by the release build: signs a release for the program's updater.
//!
//!     UPDATE_SIGNING_KEY=<64 hex> cargo run -p swiftget-engine --example sign_update -- <version> <sha256>
//!
//! prints the signature (hex) for the "sig" field of version.json. The key
//! (Ed25519 seed) is a repository secret and never stored in the repository.

use ring::signature::{Ed25519KeyPair, KeyPair};
use swiftget_engine::selfupdate::{from_hex, signature_ok, signed_message, Release, UPDATE_PUBLIC_KEY};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(version), Some(sha)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: sign_update <version> <sha256>");
        std::process::exit(2);
    };
    let seed = std::env::var("UPDATE_SIGNING_KEY").ok().and_then(|k| from_hex(&k)).unwrap_or_default();
    if seed.len() != 32 {
        eprintln!("UPDATE_SIGNING_KEY is missing or not 64 hex characters");
        std::process::exit(1);
    }
    let kp = Ed25519KeyPair::from_seed_unchecked(&seed).expect("bad key");
    let public: String = kp.public_key().as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if public != UPDATE_PUBLIC_KEY {
        eprintln!("UPDATE_SIGNING_KEY does not belong to the public key built into the program");
        std::process::exit(1);
    }
    let sig: String = kp.sign(signed_message(version, sha).as_bytes()).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    let check = Release { version: version.clone(), url: String::new(), sha256: sha.clone(), sig: sig.clone() };
    assert!(signature_ok(&check, UPDATE_PUBLIC_KEY));
    println!("{sig}");
}
