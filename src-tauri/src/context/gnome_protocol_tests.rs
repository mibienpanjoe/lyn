use super::*;
use uuid::Uuid;

fn correlation() -> Correlation {
    Correlation {
        generation: Uuid::from_u128(1),
        request_id: Uuid::from_u128(2),
        window_id: 0x12345678,
    }
}

fn candidate() -> Candidate {
    Candidate {
        source_id: 17,
        pty_number: 0,
        foreground_pgid: 42,
        registered_us: 100,
        changed_us: 150,
        revision: 3,
    }
}

fn reply() -> Frame {
    Frame::Reply {
        correlation: correlation(),
        observed_us: 220,
        outcome: Outcome::Candidate(candidate()),
    }
}

fn pending() -> PendingQuery {
    PendingQuery::new(correlation(), 200).unwrap()
}

#[test]
fn all_frame_kinds_roundtrip_with_exact_size() {
    for frame in [
        Frame::Hello,
        Frame::Bind {
            generation: correlation().generation,
        },
        Frame::Ready {
            generation: correlation().generation,
        },
        Frame::Query {
            correlation: correlation(),
            invoked_us: 200,
        },
        reply(),
        Frame::Reply {
            correlation: correlation(),
            observed_us: 220,
            outcome: Outcome::Missing,
        },
        Frame::Reply {
            correlation: correlation(),
            observed_us: 220,
            outcome: Outcome::Changed,
        },
        Frame::Reply {
            correlation: correlation(),
            observed_us: 220,
            outcome: Outcome::Ambiguous,
        },
        Frame::Reply {
            correlation: correlation(),
            observed_us: 220,
            outcome: Outcome::Unavailable,
        },
    ] {
        let bytes = frame.encode();
        assert_eq!(bytes.len(), 112);
        assert_eq!(Frame::decode(&bytes), Ok(frame));
    }
}

#[test]
fn rust_codec_matches_shared_c_vectors() {
    let mut frames = vec![
        ("hello", Frame::Hello),
        (
            "bind",
            Frame::Bind {
                generation: correlation().generation,
            },
        ),
        (
            "ready",
            Frame::Ready {
                generation: correlation().generation,
            },
        ),
        (
            "query",
            Frame::Query {
                correlation: correlation(),
                invoked_us: 258,
            },
        ),
        ("candidate", reply()),
    ];
    for (name, outcome) in [
        ("missing", Outcome::Missing),
        ("changed", Outcome::Changed),
        ("ambiguous", Outcome::Ambiguous),
        ("unavailable", Outcome::Unavailable),
    ] {
        frames.push((
            name,
            Frame::Reply {
                correlation: correlation(),
                observed_us: 220,
                outcome,
            },
        ));
    }
    frames.push((
        "wide_candidate",
        Frame::Reply {
            correlation: correlation(),
            observed_us: 0x0100000000000200,
            outcome: Outcome::Candidate(Candidate {
                pty_number: 0x01020304,
                foreground_pgid: 0x01020304,
                source_id: 0x0102030405060708,
                revision: 0x0807060504030201,
                registered_us: 0x0100000000000000,
                changed_us: 0x0100000000000100,
            }),
        },
    ));
    let rows: Vec<_> = include_str!("../../../integrations/gnome/wire_vectors.txt")
        .lines()
        .collect();
    assert_eq!(rows.len(), frames.len());
    for (row, (name, frame)) in rows.iter().zip(frames) {
        let (label, hex) = row.split_once(' ').unwrap();
        assert_eq!(label, name);
        assert_eq!(hex.len(), FRAME_BYTES * 2);
        let bytes: Vec<_> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        assert_eq!(frame.encode().as_slice(), bytes, "{name}");
        assert_eq!(Frame::decode(&bytes), Ok(frame), "{name}");
    }
}

#[test]
fn rust_codec_rejects_shared_c_negative_vectors() {
    let rows: Vec<_> = include_str!("../../../integrations/gnome/wire_rejects.txt")
        .lines()
        .collect();
    assert_eq!(rows.len(), 22);
    for row in rows {
        let (name, hex) = row.split_once(' ').unwrap();
        assert_eq!(hex.len(), FRAME_BYTES * 2);
        let bytes: Vec<_> = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect();
        assert!(Frame::decode(&bytes).is_err(), "{name}");
    }
}

#[test]
fn query_has_independent_network_order_golden_vector() {
    let mut expected = [0; 112];
    expected[0..8].copy_from_slice(&[b'L', b'Y', b'G', b'N', 0, 1, 0, 4]);
    expected[23] = 1;
    expected[39] = 2;
    expected[40..44].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    expected[66..68].copy_from_slice(&[0x01, 0x02]);
    let frame = Frame::Query {
        correlation: correlation(),
        invoked_us: 258,
    };
    assert_eq!(frame.encode(), expected);
    assert_eq!(Frame::decode(&expected), Ok(frame));
}

#[test]
fn refuses_truncated_oversized_and_experimental_json_frames() {
    let valid = reply().encode();
    for length in 0..112 {
        assert!(Frame::decode(&valid[..length]).is_err());
    }
    let mut oversized = valid.to_vec();
    oversized.push(0);
    assert!(Frame::decode(&oversized).is_err());
    assert!(Frame::decode(br#"{"version":0,"kind":"snapshot"}"#).is_err());
}

#[test]
fn rejects_unknown_versions_kinds_reserved_and_noncanonical_fields() {
    for (offset, value) in [(0, b'X'), (5, 0), (5, 2), (7, 0), (7, 6), (92, 1), (95, 6)] {
        let mut bytes = reply().encode();
        bytes[offset] = value;
        assert!(Frame::decode(&bytes).is_err(), "offset {offset}");
    }
    for offset in 96..112 {
        let mut bytes = reply().encode();
        bytes[offset] = 1;
        assert!(Frame::decode(&bytes).is_err());
    }
    for offset in [8, 24, 40, 44, 48, 52, 60, 68, 76, 84, 94] {
        let mut bytes = Frame::Hello.encode();
        bytes[offset] = 1;
        assert!(Frame::decode(&bytes).is_err(), "hello offset {offset}");
    }
    for caps in [0_u16, 1, 2, 4, u16::MAX] {
        let mut bytes = Frame::Hello.encode();
        bytes[92..94].copy_from_slice(&caps.to_be_bytes());
        assert!(Frame::decode(&bytes).is_err());
    }
}

#[test]
fn rejects_nil_identities_invalid_processes_and_impossible_clock_order() {
    for range in [
        8..24,
        24..40,
        40..44,
        48..52,
        52..60,
        60..68,
        76..84,
        84..92,
    ] {
        let mut bytes = reply().encode();
        bytes[range].fill(0);
        assert!(Frame::decode(&bytes).is_err());
    }
    let mut bytes = reply().encode();
    bytes[48..52].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(Frame::decode(&bytes).is_err());
    for (registered_us, changed_us) in [(151, 150), (100, 221)] {
        let candidate = Candidate {
            registered_us,
            changed_us,
            ..candidate()
        };
        assert!(
            Frame::decode(
                &Frame::Reply {
                    correlation: correlation(),
                    observed_us: 220,
                    outcome: Outcome::Candidate(candidate)
                }
                .encode()
            )
            .is_err()
        );
    }
}

#[test]
fn accepts_only_one_current_pre_invocation_candidate_even_if_os_focus_is_lost() {
    // No global focus flag: opening Lyn must not invalidate an unchanged tab.
    let mut pending = pending();
    assert_eq!(
        pending.accept(reply(), correlation().generation, 230),
        Ok(candidate())
    );
    assert_eq!(
        pending.accept(reply(), correlation().generation, 231),
        Err(ReplyReject::Consumed)
    );
}

#[test]
fn rejects_tab_changes_and_new_widgets_after_the_invocation() {
    for source in [
        Candidate {
            changed_us: 200,
            ..candidate()
        }, // Same microsecond cannot establish which event happened first.
        Candidate {
            changed_us: 201,
            revision: 5,
            ..candidate()
        }, // Includes A → B → A.
        Candidate {
            registered_us: 201,
            changed_us: 201,
            ..candidate()
        },
    ] {
        let frame = Frame::Reply {
            correlation: correlation(),
            observed_us: 220,
            outcome: Outcome::Candidate(source),
        };
        assert_eq!(
            pending().accept(frame, correlation().generation, 230),
            Err(ReplyReject::Changed)
        );
    }
}

#[test]
fn rejects_other_request_window_generation_and_disconnected_connection() {
    for other in [
        Correlation {
            generation: Uuid::from_u128(3),
            ..correlation()
        },
        Correlation {
            request_id: Uuid::from_u128(3),
            ..correlation()
        },
        Correlation {
            window_id: 7,
            ..correlation()
        },
    ] {
        let frame = Frame::Reply {
            correlation: other,
            observed_us: 220,
            outcome: Outcome::Candidate(candidate()),
        };
        assert_eq!(
            pending().accept(frame, correlation().generation, 230),
            Err(ReplyReject::Correlation)
        );
    }
    assert_eq!(
        pending().accept(reply(), Uuid::from_u128(9), 230),
        Err(ReplyReject::Disconnected)
    );
}

#[test]
fn rejects_stale_future_and_deadline_boundary_samples() {
    for (observed_us, received_us) in [(199, 230), (231, 230), (220, 120_200), (220, 199)] {
        let frame = Frame::Reply {
            correlation: correlation(),
            observed_us,
            outcome: Outcome::Candidate(candidate()),
        };
        assert_eq!(
            pending().accept(frame, correlation().generation, received_us),
            Err(ReplyReject::Timing)
        );
    }
    assert!(PendingQuery::new(correlation(), u64::MAX).is_none());
    assert!(PendingQuery::new(correlation(), 0).is_none());
}

#[test]
fn unavailable_or_wrong_kind_reply_cannot_become_a_late_success() {
    for outcome in [
        Outcome::Missing,
        Outcome::Changed,
        Outcome::Ambiguous,
        Outcome::Unavailable,
    ] {
        let mut pending = pending();
        let frame = Frame::Reply {
            correlation: correlation(),
            observed_us: 220,
            outcome,
        };
        assert_eq!(
            pending.accept(frame, correlation().generation, 230),
            Err(ReplyReject::Unresolved(outcome))
        );
        assert_eq!(
            pending.accept(reply(), correlation().generation, 231),
            Err(ReplyReject::Consumed)
        );
    }
    assert_eq!(
        pending().accept(Frame::Hello, correlation().generation, 230),
        Err(ReplyReject::Correlation)
    );
}
