//! Time based one time passwords (RFC 6238), the codes an authenticator app shows.
//!
//! Written out rather than pulled in as a crate because the algorithm is a few lines and the parameters
//! are fixed: HMAC-SHA1, 6 digits, 30 second steps. Those are the only values every authenticator app
//! supports, and the ones the `otpauth` URI leaves implied, so nothing here is configurable. SHA-1's
//! collision weakness does not apply to its use inside HMAC, which is why the standard still uses it.
//!
//! Pure functions only. Storing the secret and remembering the last accepted step belong to the auth
//! service and its repository.

use hmac::{Hmac, Mac};
use rand_core::{OsRng, RngCore};
use sha1::Sha1;

/// Seconds each code lives for.
pub const STEP_SECONDS: i64 = 30;
const DIGITS: u32 = 6;

/// How many steps either side of now are accepted, to forgive a phone whose clock has drifted a little
/// and somebody who types a code just as it turns over.
const DRIFT_STEPS: i64 = 1;

/// 160 bits, the length RFC 4226 recommends and the one authenticator apps expect.
pub const SECRET_BYTES: usize = 20;

pub fn generate_secret() -> Vec<u8> {
    let mut secret = vec![0u8; SECRET_BYTES];
    OsRng.fill_bytes(&mut secret);
    secret
}

/// The time step a moment falls in.
///
/// Floored all the way, as RFC 6238 defines it: truncating division would make the step either side of
/// the epoch sixty seconds long. Only a clock set before 1970 reaches that, but the step is what the
/// replay guard compares, so it should mean the same thing everywhere.
pub fn step_at(now_ms: i64) -> i64 {
    now_ms.div_euclid(1000).div_euclid(STEP_SECONDS)
}

/// The code for one time step (RFC 4226 section 5.3), as a number, so leading zeros are the caller's to
/// print.
pub fn code_at(secret: &[u8], step: i64) -> u32 {
    let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(secret) else {
        // HMAC accepts keys of any length, so this cannot happen; a code nobody can type is the safe
        // answer if it somehow did.
        return u32::MAX;
    };
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();

    // Dynamic truncation: the low nibble of the last byte picks where to read four bytes from.
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let value = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    value % 10u32.pow(DIGITS)
}

/// Check a typed code, returning the step it matched.
///
/// A code is refused if its step is not newer than `last_used_step`, so one that was just used cannot be
/// replayed while it is still showing on the screen. The caller stores the returned step as the new
/// `last_used_step`.
pub fn verify(secret: &[u8], typed: &str, now_ms: i64, last_used_step: Option<i64>) -> Option<i64> {
    let digits: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() != DIGITS as usize || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let typed: u32 = digits.parse().ok()?;

    let now = step_at(now_ms);
    (now - DRIFT_STEPS..=now + DRIFT_STEPS)
        .filter(|step| last_used_step.is_none_or(|last| *step > last))
        .find(|step| code_at(secret, *step) == typed)
}

/// The URI an authenticator app reads from the QR code.
///
/// The issuer is repeated as a parameter as well as the label prefix, because some apps read one and
/// some the other.
pub fn otpauth_uri(issuer: &str, account: &str, secret: &[u8]) -> String {
    let label = format!("{}:{}", percent_encode(issuer), percent_encode(account));
    format!(
        "otpauth://totp/{label}?secret={}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECONDS}",
        base32_encode(secret),
        percent_encode(issuer)
    )
}

/// RFC 4648 base32 without padding, the form authenticator apps take a typed key in.
pub fn base32_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer: u32 = 0;
    let mut bits = 0;

    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

/// The same key in groups of four, which is how people copy it by hand without losing their place.
pub fn grouped(key: &str) -> String {
    key.as_bytes()
        .chunks(4)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read a base32 key back, the way an authenticator app does when one is typed in. Only tests need it:
/// they play the part of the phone.
#[cfg(test)]
pub fn base32_decode(key: &str) -> Vec<u8> {
    const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = Vec::new();
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for c in key.chars().filter(|c| !c.is_whitespace()) {
        let Some(value) = ALPHABET.find(c.to_ascii_uppercase()) else {
            continue;
        };
        buffer = (buffer << 5) | value as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
}

fn percent_encode(raw: &str) -> String {
    raw.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'@' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared secret of the RFC 6238 appendix B test vectors for SHA-1.
    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn matches_the_rfc_6238_test_vectors() {
        // The RFC lists 8 digit codes; these are their last 6 digits, which is what a 6 digit code is.
        let vectors = [
            (59, 287_082),
            (1_111_111_109, 81_804),
            (1_111_111_111, 50_471),
            (1_234_567_890, 5_924),
            (2_000_000_000, 279_037),
            (20_000_000_000, 353_130),
        ];
        for (seconds, expected) in vectors {
            assert_eq!(
                code_at(RFC_SECRET, step_at(seconds * 1000)),
                expected,
                "at {seconds}s"
            );
        }
    }

    #[test]
    fn a_code_is_accepted_one_step_either_side_and_no_further() {
        let now_ms = 1_111_111_111_000;
        let now = step_at(now_ms);
        let format = |step: i64| format!("{:06}", code_at(RFC_SECRET, step));

        assert_eq!(verify(RFC_SECRET, &format(now), now_ms, None), Some(now));
        assert_eq!(
            verify(RFC_SECRET, &format(now - 1), now_ms, None),
            Some(now - 1)
        );
        assert_eq!(
            verify(RFC_SECRET, &format(now + 1), now_ms, None),
            Some(now + 1)
        );
        assert_eq!(verify(RFC_SECRET, &format(now - 2), now_ms, None), None);
        assert_eq!(verify(RFC_SECRET, &format(now + 2), now_ms, None), None);
    }

    #[test]
    fn a_used_code_cannot_be_replayed() {
        let now_ms = 1_234_567_890_000;
        let code = format!("{:06}", code_at(RFC_SECRET, step_at(now_ms)));

        let used = verify(RFC_SECRET, &code, now_ms, None).expect("first use");
        assert_eq!(verify(RFC_SECRET, &code, now_ms, Some(used)), None);
    }

    #[test]
    fn leading_zeros_and_spaces_are_handled() {
        // 005924 is a real code with two leading zeros, from the vectors above.
        let now_ms = 1_234_567_890_000;
        assert!(verify(RFC_SECRET, "005924", now_ms, None).is_some());
        assert!(verify(RFC_SECRET, "005 924", now_ms, None).is_some());
        assert!(verify(RFC_SECRET, "5924", now_ms, None).is_none());
        assert!(verify(RFC_SECRET, "00592a", now_ms, None).is_none());
    }

    #[test]
    fn base32_matches_rfc_4648() {
        assert_eq!(base32_encode(b""), "");
        assert_eq!(base32_encode(b"f"), "MY");
        assert_eq!(base32_encode(b"foo"), "MZXW6");
        assert_eq!(base32_encode(b"foobar"), "MZXW6YTBOI");
        assert_eq!(
            base32_encode(RFC_SECRET),
            "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"
        );
    }

    #[test]
    fn a_grouped_key_reads_back_to_the_same_secret() {
        let secret = generate_secret();
        assert_eq!(base32_decode(&grouped(&base32_encode(&secret))), secret);
    }

    #[test]
    fn the_uri_is_what_authenticator_apps_expect() {
        let uri = otpauth_uri("On Air Record", "owner@example.com", RFC_SECRET);
        assert_eq!(
            uri,
            "otpauth://totp/On%20Air%20Record:owner@example.com?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=On%20Air%20Record&algorithm=SHA1&digits=6&period=30"
        );
    }

    #[test]
    fn secrets_are_random_and_full_length() {
        let first = generate_secret();
        assert_eq!(first.len(), SECRET_BYTES);
        assert_ne!(first, generate_secret());
        assert_eq!(grouped("ABCDEFGHIJ"), "ABCD EFGH IJ");
    }

    #[test]
    fn steps_before_1970_are_thirty_seconds_long_too() {
        // Found by `every_step_lasts_thirty_seconds`: truncating division folded the thirty seconds either
        // side of the epoch into step zero, a sixty second step, where RFC 6238 floors.
        assert_eq!(step_at(-1_000), -1);
        assert_eq!(step_at(-30_000), -1);
        assert_eq!(step_at(-30_001), -2);
        assert_eq!(step_at(29_999), 0);
    }

    mod props {
        use super::*;
        use proptest::prelude::*;

        /// Any moment from 1900 to 2200.
        fn moment() -> impl Strategy<Value = i64> {
            -2_208_988_800_000i64..7_258_118_400_000
        }

        fn secret() -> impl Strategy<Value = Vec<u8>> {
            proptest::collection::vec(any::<u8>(), SECRET_BYTES..=SECRET_BYTES)
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

            /// Whatever the key and the step, the code prints as exactly six ASCII digits, which is all an
            /// authenticator app can show and all `verify` will take.
            #[test]
            fn every_code_is_six_digits(key in proptest::collection::vec(any::<u8>(), 0..80), step in any::<i64>()) {
                let printed = format!("{:06}", code_at(&key, step));
                prop_assert_eq!(printed.len(), 6);
                prop_assert!(printed.bytes().all(|byte| byte.is_ascii_digit()));
            }

            /// Steps tile time: each lasts exactly thirty seconds and the next begins where it ends, before
            /// the epoch as after it. The replay guard compares steps, so a step twice as long anywhere
            /// would let a code be used for a minute, and a gap would make one refuse a valid code.
            #[test]
            fn every_step_lasts_thirty_seconds(now_ms in any::<i64>().prop_map(|value| value / 4)) {
                let step = step_at(now_ms);
                let start_ms = step * STEP_SECONDS * 1000;
                prop_assert!(start_ms <= now_ms && now_ms < start_ms + STEP_SECONDS * 1000, "{} is not in step {}", now_ms, step);
                prop_assert_eq!(step_at(now_ms + STEP_SECONDS * 1000), step + 1);
            }

            /// The code shown for a moment is accepted at that moment, typed with or without spaces, and
            /// matches a step no more than one away whose code it really is. A phone and a recorder that
            /// agree on the time must always agree on the code.
            #[test]
            fn the_current_code_is_accepted(key in secret(), now_ms in moment(), space_at in 0usize..7) {
                let code = format!("{:06}", code_at(&key, step_at(now_ms)));
                let mut typed = code.clone();
                typed.insert(space_at.min(6), ' ');
                for attempt in [code.clone(), typed] {
                    let step = verify(&key, &attempt, now_ms, None);
                    prop_assert!(step.is_some(), "{:?} refused", attempt);
                    let step = step.expect("checked");
                    prop_assert!((step - step_at(now_ms)).abs() <= 1);
                    prop_assert_eq!(format!("{:06}", code_at(&key, step)), code.clone());
                }
            }

            /// Once a code's step has been used, that code is never accepted again for that step or an
            /// earlier one, whatever the time, which is the whole of the replay protection.
            #[test]
            fn a_used_step_is_never_accepted_again(key in secret(), now_ms in moment(), later_ms in 0i64..120_000) {
                let code = format!("{:06}", code_at(&key, step_at(now_ms)));
                let used = verify(&key, &code, now_ms, None).expect("accepted the first time");
                if let Some(again) = verify(&key, &code, now_ms + later_ms, Some(used)) {
                    prop_assert!(again > used, "step {} accepted again as {}", used, again);
                }
            }

            /// A code from further than one step away is refused, unless by chance it is also the code of
            /// a step inside the window, one time in about a million per step.
            #[test]
            fn a_code_from_further_away_is_refused(key in secret(), now_ms in moment(), away in prop_oneof![-50i64..=-2, 2i64..=50]) {
                let now = step_at(now_ms);
                let stale = code_at(&key, now + away);
                prop_assume!((now - 1..=now + 1).all(|step| code_at(&key, step) != stale));
                prop_assert_eq!(verify(&key, &format!("{stale:06}"), now_ms, None), None);
            }

            /// Anything typed at all is either refused or accepted as a code, never a panic, and nothing
            /// but six digits (spaces aside) is ever accepted.
            #[test]
            fn any_typing_is_answered(key in secret(), now_ms in moment(), typed in ".{0,12}") {
                if verify(&key, &typed, now_ms, None).is_some() {
                    let digits: String = typed.chars().filter(|c| !c.is_whitespace()).collect();
                    prop_assert!(digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_digit()));
                }
            }

            /// The key shown for typing in by hand reads back to exactly the secret, grouped or not, and is
            /// as long as RFC 4648 says: an authenticator app that decoded a different key would show codes
            /// the recorder never accepts, and nobody could finish turning on two factor sign in.
            #[test]
            fn the_typed_key_reads_back_to_the_secret(key in proptest::collection::vec(any::<u8>(), 0..64)) {
                let encoded = base32_encode(&key);
                prop_assert_eq!(encoded.len(), (key.len() * 8).div_ceil(5));
                prop_assert_eq!(base32_decode(&encoded), key.clone());
                prop_assert_eq!(base32_decode(&grouped(&encoded)), key);
                prop_assert!(grouped(&encoded).split(' ').all(|group| group.len() <= 4));
            }

            /// Whatever the issuer and account are called, they cannot break out of the label: the URI
            /// keeps one `?`, the label one `:`, and the same five parameters. An account name holding `&`
            /// or `?` must not be able to set the secret or the period an app reads.
            #[test]
            fn names_cannot_escape_the_label(issuer in ".{0,16}", account in ".{0,24}", key in secret()) {
                let uri = otpauth_uri(&issuer, &account, &key);
                let rest = uri.strip_prefix("otpauth://totp/").expect("the scheme and type");
                let (label, query) = rest.split_once('?').expect("a query");
                prop_assert!(!query.contains('?') && !label.contains('/') && !label.contains('#'));
                prop_assert_eq!(label.matches(':').count(), 1);
                let names: Vec<&str> = query.split('&').map(|pair| pair.split('=').next().unwrap_or("")).collect();
                prop_assert_eq!(names, vec!["secret", "issuer", "algorithm", "digits", "period"]);
                prop_assert!(uri.is_ascii());
            }
        }
    }
}
