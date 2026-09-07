//! The versioned digest scalar, and what a contract family may do with it.
//!
//! These values outlive the code that wrote them. Every test here is about that
//! gap: a reader years later has the scalar and nothing else, so the scalar has
//! to say which calculation produced it, and the build reading it has to be able
//! to distinguish "this is not a digest" from "this is a digest I cannot check".

use engr::digest::{Family, Support, Versioned, MAX_CONTRACT_VERSION};

/// The canonical spelling is one spelling.
///
/// Every rejection here exists so that two persisted values which mean the same
/// thing cannot look different — because the enclosing structure gets hashed,
/// and a second spelling would be a second hash for one fact.
#[test]
fn a_persisted_scalar_has_exactly_one_canonical_spelling() {
    let value = Versioned::parse("1:abc123").expect("canonical");
    assert_eq!(value.version(), 1);
    assert_eq!(value.digest(), "abc123");
    assert_eq!(value.to_string(), "1:abc123");

    for (bad, expected) in [
        ("abc123", "not a versioned digest"),
        ("1abc123", "not a versioned digest"),
        (":abc123", "no decimal contract version"),
        ("v1:abc123", "no decimal contract version"),
        ("01:abc123", "leading zero"),
        ("0:abc123", "permanently reserved"),
        ("1:", "no digest after the colon"),
        ("1:ABC123", "not lowercase hexadecimal"),
        ("1:xyz", "not lowercase hexadecimal"),
        ("4294967296:abc123", "outside 1..4294967295"),
    ] {
        let error = Versioned::parse(bad).expect_err(&format!("{bad:?} is refused"));
        assert!(
            error.message.contains(expected),
            "{bad:?} should say {expected:?}, said {:?}",
            error.message
        );
    }

    // The boundary is usable from both ends.
    assert_eq!(Versioned::parse("1:ff").expect("one").version(), 1);
    assert_eq!(
        Versioned::parse(&format!("{MAX_CONTRACT_VERSION}:ff"))
            .expect("max")
            .version(),
        MAX_CONTRACT_VERSION
    );
}

/// Ordering is by the parsed tuple, not the string.
///
/// `"10:"` sorts before `"2:"` as text, which is backwards, and these values
/// participate in canonical set ordering — so the wrong comparison would put a
/// hash contract's output in the wrong order and change an enclosing digest.
#[test]
fn versions_order_numerically_and_not_lexically() {
    let mut values: Vec<Versioned> = ["10:aa", "2:aa", "1:ff", "1:aa"]
        .iter()
        .map(|value| Versioned::parse(value).expect("parse"))
        .collect();
    values.sort();
    assert_eq!(
        values
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>(),
        vec!["1:aa", "1:ff", "2:aa", "10:aa"],
    );
    assert!(
        "10:aa" < "2:aa",
        "and raw string order really is the wrong one, which is why this matters"
    );
}

/// Zero is reserved, and reserving it is the point.
///
/// Every versioned scheme grows an escape hatch unless one is refused up front.
/// A legacy value has a legacy compatibility path; synthesizing `0:` for it
/// would claim it was written under a contract that never existed.
#[test]
fn zero_is_never_a_version_and_never_means_legacy() {
    Versioned::parse("0:abc").expect_err("0 is refused on read");
    Versioned::new(0, "abc").expect_err("and on construction");
    assert!(
        Versioned::new(1, "abc").is_ok(),
        "while 1 is the first real contract"
    );
}

/// A value cannot enter the type by a route that storage would have refused.
#[test]
fn construction_refuses_what_parsing_refuses() {
    Versioned::new(1, "ABC").expect_err("uppercase");
    Versioned::new(1, "").expect_err("empty");
    Versioned::new(1, "xyz").expect_err("not hex");
}

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// A family with one live version, one retired, and one that only verifies.
fn family() -> Family {
    Family {
        name: "TestDigestContract",
        current: 3,
        versions: &[
            (
                1,
                Support {
                    emit: false,
                    verify: true,
                },
                64,
            ),
            (
                3,
                Support {
                    emit: true,
                    verify: true,
                },
                64,
            ),
        ],
    }
}

/// Emitting and verifying are separate permissions, and they retire apart.
///
/// A contract stops being what new values are written under long before it stops
/// having to read what was already written under it. One boolean would force a
/// choice between refusing history and emitting under several contracts at once.
#[test]
fn a_retired_contract_still_verifies_what_it_wrote() {
    let family = family();

    // Version 1 no longer emits, and still reads its own history.
    let historical = format!("1:{SHA}");
    let read = family.verify(&historical).expect("history stays readable");
    assert_eq!(read.version(), 1);

    // New values come out under the one current emitter, not the oldest known.
    let emitted = family.emit(SHA).expect("emit");
    assert_eq!(emitted.version(), 3, "one current emission version");
    family.verify(&emitted.to_string()).expect("and verifies");

    // Version 2 was never published: skipping numbers is allowed, and an
    // unknown one is not silently treated as adjacent to the ones that exist.
    family
        .verify(&format!("2:{SHA}"))
        .expect_err("2 is not a version this build knows");
}

/// Unsupported is not malformed, and the difference is the whole point.
///
/// A well-formed scalar naming a contract this build does not know is readable
/// data that cannot be checked here — which is what lets an experimental
/// contract be retired without its verifier living forever. Calling it corrupt
/// would accuse the data of something that is true of the reader.
#[test]
fn an_unknown_contract_version_is_unsupported_rather_than_malformed() {
    let family = family();

    let unsupported = family
        .verify(&format!("9:{SHA}"))
        .expect_err("9 is not known here");
    assert!(
        unsupported.message.contains("unsupported contract version")
            && unsupported.message.contains("well formed"),
        "{}",
        unsupported.message
    );
    assert!(
        unsupported.message.contains("TestDigestContract"),
        "and it names which contract does not support it: {}",
        unsupported.message
    );

    // Grammar failures stay grammar failures.
    let malformed = family.verify("nope").expect_err("not a scalar at all");
    assert!(
        !malformed.message.contains("unsupported contract version"),
        "a malformed scalar is not an unsupported version: {}",
        malformed.message
    );
}

/// Length belongs to the contract, not the codec.
///
/// The shared parser deliberately does not know how long a digest should be, so
/// a future contract can change hash algorithm without the scalar grammar
/// moving. The owning family is what refuses a wrong-length payload.
#[test]
fn digest_length_is_the_contracts_business_and_not_the_parsers() {
    // The codec accepts any lowercase hex length.
    Versioned::parse("1:ab").expect("the codec does not police length");

    // The family does not.
    let too_short = family()
        .verify("3:ab")
        .expect_err("the contract knows its own length");
    assert!(
        too_short.message.contains("64-character digest"),
        "{}",
        too_short.message
    );
}

/// The real families this build ships.
#[test]
fn the_shipped_families_emit_and_verify_their_own_current_version() {
    for family in [
        &engr::digest::REVIEW,
        &engr::digest::CHALLENGE,
        &engr::digest::REF,
        &engr::digest::OBJECT,
        &engr::digest::SECTION,
        &engr::digest::EVENT,
    ] {
        let emitted = family.emit(SHA).expect("emit");
        assert_eq!(emitted.version(), 1, "{} starts at contract 1", family.name);
        family.verify(&emitted.to_string()).expect("round trip");

        // Field-local: a family refuses a version it does not define, even one
        // another family might.
        family
            .verify(&format!("2:{SHA}"))
            .expect_err("contract versions are not a shared namespace");
    }
}

/// A historical attestation is recomputed under **its own** contract version.
///
/// This is the difference between carrying a version and using one. A family
/// that only answers "is version 1 still supported" accepts a `1:` attestation
/// and then compares it against a value recomputed under the current version —
/// and those disagree by construction, because if the two calculations agreed
/// there would have been no reason to add a second version. The valid
/// historical proof is then reported as a changed subject, and the lifetime
/// verification promise lives in the support table and nowhere else.
///
/// The two versions here calculate deliberately differently, so routing through
/// the current emitter cannot accidentally pass.
#[test]
fn a_historical_attestation_is_recomputed_under_the_version_it_names() {
    let family = family(); // current = 3, version 1 verify-only
    let v1 = "1".repeat(64);
    let v3 = "3".repeat(64);
    let calculation = |version: u32| match version {
        1 => Ok(v1.clone()),
        3 => Ok(v3.clone()),
        other => Err(engr::Error::new(
            engr::EXIT_SCHEMA,
            format!("no calculation for {other}"),
        )),
    };

    // The historical proof verifies, because it is recomputed as version 1.
    let historical = family
        .recheck(&format!("1:{v1}"), calculation)
        .expect("version 1 is still verifiable");
    assert!(historical.agrees(), "{historical:?}");
    assert_eq!(historical.expected.version(), 1);
    assert_eq!(
        historical.expected.digest(),
        v1,
        "recomputed under 1, not under the current emitter"
    );

    // The same scalar routed through the current version would not have matched,
    // which is what makes the assertion above worth making.
    assert_ne!(v1, v3);

    // A current-version attestation still verifies against the current
    // calculation, so the fix did not simply pin everything to the oldest.
    let current = family
        .recheck(&format!("3:{v3}"), calculation)
        .expect("version 3 verifies");
    assert!(current.agrees());

    // And a genuine mismatch is still a mismatch, reported with the value the
    // caller should have produced under that same version.
    let wrong = family
        .recheck(&format!("1:{}", "a".repeat(64)), calculation)
        .expect("well formed and supported");
    assert!(!wrong.agrees());
    assert_eq!(
        wrong.expected.to_string(),
        format!("1:{v1}"),
        "the expected value is stated under the attested version"
    );
}

/// A version the support table allows but this build cannot compute is refused.
///
/// Being listed as verifiable is a promise about the contract, not evidence that
/// this binary implements it. Serving the current calculation instead would turn
/// a missing implementation into a silent wrong answer.
#[test]
fn a_supported_version_without_a_calculation_is_refused_rather_than_approximated() {
    let family = family();
    let error = family
        .recheck(&format!("1:{}", "b".repeat(64)), |_| {
            Err(engr::Error::new(
                engr::EXIT_SCHEMA,
                "this build cannot compute version 1".to_owned(),
            ))
        })
        .expect_err("no calculation, no verdict");
    assert!(
        error.message.contains("cannot compute"),
        "{}",
        error.message
    );
}
