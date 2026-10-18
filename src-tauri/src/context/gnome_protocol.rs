//! Native GNOME metadata contract; see docs/gnome_native_protocol.md.
//! Socket authentication and kernel attestation belong to the broker, not this codec.

use uuid::Uuid;

pub(crate) const FRAME_BYTES: usize = 112;
pub(crate) const REPLY_BUDGET_US: u64 = 120_000;
const CAPABILITIES: u16 = 3; // Native snapshot + monotonic topology transitions.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Correlation {
    pub(crate) generation: Uuid,
    pub(crate) request_id: Uuid,
    pub(crate) window_id: u32,
}

impl Correlation {
    fn valid(self) -> bool {
        !self.generation.is_nil() && !self.request_id.is_nil() && self.window_id != 0
    }
}

/// Untrusted native evidence, NOT a resolved project or a validated process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub(crate) source_id: u64,
    pub(crate) pty_number: u32,
    pub(crate) foreground_pgid: u32,
    pub(crate) registered_us: u64,
    pub(crate) changed_us: u64,
    pub(crate) revision: u64,
}

impl Candidate {
    fn valid(self, observed_us: u64) -> bool {
        self.source_id != 0
            && (1..=i32::MAX as u32).contains(&self.foreground_pgid)
            && self.revision != 0
            && self.registered_us > 0
            && self.registered_us <= self.changed_us
            && self.changed_us <= observed_us
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Candidate(Candidate),
    Missing,
    Changed,
    Ambiguous,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Frame {
    Hello,
    Bind {
        generation: Uuid,
    },
    Ready {
        generation: Uuid,
    },
    Query {
        correlation: Correlation,
        invoked_us: u64,
    },
    Reply {
        correlation: Correlation,
        observed_us: u64,
        outcome: Outcome,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodeReject {
    Length,
    Header,
    Version,
    Kind,
    Fields,
}

impl Frame {
    /// Fixed-width network order, no architecture-dependent C struct layout.
    pub(crate) fn encode(self) -> [u8; FRAME_BYTES] {
        let mut bytes = [0; FRAME_BYTES];
        bytes[..4].copy_from_slice(b"LYGN");
        bytes[4..6].copy_from_slice(&1_u16.to_be_bytes());
        let (kind, correlation, timestamp) = match self {
            Self::Hello => (1_u16, None, 0),
            Self::Bind { generation } | Self::Ready { generation } => {
                bytes[8..24].copy_from_slice(generation.as_bytes());
                (
                    if matches!(self, Self::Bind { .. }) {
                        2
                    } else {
                        3
                    },
                    None,
                    0,
                )
            }
            Self::Query {
                correlation,
                invoked_us,
            } => (4, Some(correlation), invoked_us),
            Self::Reply {
                correlation,
                observed_us,
                outcome,
            } => {
                let status: u16 = match outcome {
                    Outcome::Candidate(source) => {
                        bytes[44..48].copy_from_slice(&source.pty_number.to_be_bytes());
                        bytes[48..52].copy_from_slice(&source.foreground_pgid.to_be_bytes());
                        bytes[52..60].copy_from_slice(&source.source_id.to_be_bytes());
                        bytes[68..76].copy_from_slice(&source.changed_us.to_be_bytes());
                        bytes[76..84].copy_from_slice(&source.revision.to_be_bytes());
                        bytes[84..92].copy_from_slice(&source.registered_us.to_be_bytes());
                        1
                    }
                    Outcome::Missing => 2,
                    Outcome::Changed => 3,
                    Outcome::Ambiguous => 4,
                    Outcome::Unavailable => 5,
                };
                bytes[94..96].copy_from_slice(&status.to_be_bytes());
                (5, Some(correlation), observed_us)
            }
        };
        bytes[6..8].copy_from_slice(&kind.to_be_bytes());
        if kind <= 3 {
            bytes[92..94].copy_from_slice(&CAPABILITIES.to_be_bytes());
        }
        if let Some(correlation) = correlation {
            bytes[8..24].copy_from_slice(correlation.generation.as_bytes());
            bytes[24..40].copy_from_slice(correlation.request_id.as_bytes());
            bytes[40..44].copy_from_slice(&correlation.window_id.to_be_bytes());
        }
        bytes[60..68].copy_from_slice(&timestamp.to_be_bytes());
        bytes
    }

    pub(crate) fn decode(input: &[u8]) -> Result<Self, DecodeReject> {
        let bytes: &[u8; FRAME_BYTES] = input.try_into().map_err(|_| DecodeReject::Length)?;
        if &bytes[..4] != b"LYGN" {
            return Err(DecodeReject::Header);
        }
        if bytes[4..6] != 1_u16.to_be_bytes() {
            return Err(DecodeReject::Version);
        }
        let generation = Uuid::from_bytes(bytes[8..24].try_into().unwrap());
        let correlation = Correlation {
            generation,
            request_id: Uuid::from_bytes(bytes[24..40].try_into().unwrap()),
            window_id: u32::from_be_bytes(bytes[40..44].try_into().unwrap()),
        };
        let timestamp = u64::from_be_bytes(bytes[60..68].try_into().unwrap());
        let kind = u16::from_be_bytes(bytes[6..8].try_into().unwrap());
        let frame = match kind {
            1 => Self::Hello,
            2 | 3 if !generation.is_nil() => {
                if kind == 2 {
                    Self::Bind { generation }
                } else {
                    Self::Ready { generation }
                }
            }
            2 | 3 => return Err(DecodeReject::Fields),
            4 | 5 if !correlation.valid() || timestamp == 0 => return Err(DecodeReject::Fields),
            4 => Self::Query {
                correlation,
                invoked_us: timestamp,
            },
            5 => {
                let outcome = match u16::from_be_bytes(bytes[94..96].try_into().unwrap()) {
                    1 => {
                        let source = Candidate {
                            pty_number: u32::from_be_bytes(bytes[44..48].try_into().unwrap()),
                            foreground_pgid: u32::from_be_bytes(bytes[48..52].try_into().unwrap()),
                            source_id: u64::from_be_bytes(bytes[52..60].try_into().unwrap()),
                            changed_us: u64::from_be_bytes(bytes[68..76].try_into().unwrap()),
                            revision: u64::from_be_bytes(bytes[76..84].try_into().unwrap()),
                            registered_us: u64::from_be_bytes(bytes[84..92].try_into().unwrap()),
                        };
                        if !source.valid(timestamp) {
                            return Err(DecodeReject::Fields);
                        }
                        Outcome::Candidate(source)
                    }
                    2 => Outcome::Missing,
                    3 => Outcome::Changed,
                    4 => Outcome::Ambiguous,
                    5 => Outcome::Unavailable,
                    _ => return Err(DecodeReject::Fields),
                };
                Self::Reply {
                    correlation,
                    observed_us: timestamp,
                    outcome,
                }
            }
            _ => return Err(DecodeReject::Kind),
        };
        // Canonical re-encoding enforces capability bits and ZERO for every field
        // forbidden in this kind, including reserved bytes and failed-reply identity.
        if frame.encode() != *bytes {
            return Err(DecodeReject::Fields);
        }
        Ok(frame)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplyReject {
    Consumed,
    Disconnected,
    Correlation,
    Timing,
    Changed,
    Unresolved(Outcome),
}

pub(crate) struct PendingQuery {
    correlation: Correlation,
    invoked_us: u64,
    deadline_us: u64,
    consumed: bool,
}

impl PendingQuery {
    pub(crate) fn new(correlation: Correlation, invoked_us: u64) -> Option<Self> {
        if !correlation.valid() || invoked_us == 0 {
            return None;
        }
        Some(Self {
            correlation,
            invoked_us,
            deadline_us: invoked_us.checked_add(REPLY_BUDGET_US)?,
            consumed: false,
        })
    }

    /// One attempt, including refusal. Call only with a decoded frame from the
    /// authenticated connection. Success still requires kernel attestation.
    pub(crate) fn accept(
        &mut self,
        frame: Frame,
        live_generation: Uuid,
        received_us: u64,
    ) -> Result<Candidate, ReplyReject> {
        if self.consumed {
            return Err(ReplyReject::Consumed);
        }
        self.consumed = true;
        if live_generation != self.correlation.generation {
            return Err(ReplyReject::Disconnected);
        }
        let Frame::Reply {
            correlation,
            observed_us,
            outcome,
        } = frame
        else {
            return Err(ReplyReject::Correlation);
        };
        if correlation != self.correlation {
            return Err(ReplyReject::Correlation);
        }
        if received_us < self.invoked_us
            || received_us >= self.deadline_us
            || observed_us < self.invoked_us
            || observed_us > received_us
        {
            return Err(ReplyReject::Timing);
        }
        let Outcome::Candidate(source) = outcome else {
            return Err(ReplyReject::Unresolved(outcome));
        };
        // Equality at microsecond precision cannot prove ordering either.
        if source.registered_us >= self.invoked_us || source.changed_us >= self.invoked_us {
            return Err(ReplyReject::Changed);
        }
        Ok(source)
    }
}

#[cfg(test)]
#[path = "gnome_protocol_tests.rs"]
mod tests;
