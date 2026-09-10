use super::*;

/// Deterministic backoff. The host may add bounded jitter before sleeping; it
/// may not shorten the delay. Kept pure so a replayed transfer is reproducible.
pub(crate) fn backoff_ms(stalled_attempts: u32) -> u64 {
    let shift = stalled_attempts.saturating_sub(1).min(5);
    1_000u64.saturating_mul(1u64 << shift).min(30_000)
}

fn align_chunk(requested: Option<u64>, total: u64) -> u64 {
    let raw = requested
        .unwrap_or(DEFAULT_CHUNK_BYTES)
        .clamp(CHUNK_ALIGNMENT_BYTES, MAX_CHUNK_BYTES);
    // Every non-final resumable chunk must be a multiple of 256 KiB. Rounding
    // DOWN keeps that true; rounding up could exceed the host's memory bound.
    let aligned = (raw / CHUNK_ALIGNMENT_BYTES) * CHUNK_ALIGNMENT_BYTES;
    let aligned = aligned.max(CHUNK_ALIGNMENT_BYTES);
    if total < aligned {
        total.max(1)
    } else {
        aligned
    }
}

impl TransferState {
    fn new(plan: &TransferPlan) -> Self {
        TransferState {
            schema: TRANSFER_SCHEMA.to_string(),
            total_bytes: plan.total_bytes,
            chunk_bytes: align_chunk(plan.chunk_bytes, plan.total_bytes),
            committed: 0,
            sent: 0,
            stage: Stage::Probing,
            stalled_attempts: 0,
            inconclusive_probes: 0,
            last_cause: None,
            retry_at: 0,
        }
    }

    /// The server proved it holds `committed` bytes. This is the only writer of
    /// that field, and it is the only place progress can be recognised.
    fn commit(&mut self, committed: u64) {
        if committed > self.committed {
            self.committed = committed;
            // Real progress clears every stall counter. A transfer that is
            // moving is never killed by a bound meant for one that is not.
            self.stalled_attempts = 0;
            self.inconclusive_probes = 0;
            self.last_cause = None;
        }
        // The host's optimistic counter is always re-anchored to the server's
        // number, so a truncated or rejected write can never look like progress.
        self.sent = self.committed;
    }

    fn stall(&mut self, cause: Cause, now: u64) -> Action {
        self.stalled_attempts = self.stalled_attempts.saturating_add(1);
        self.last_cause = Some(cause.clone());
        if self.stalled_attempts >= MAX_STALLED_ATTEMPTS {
            self.stage = Stage::Failed;
            let retryable = cause.retryable();
            return Action::Fail { cause, retryable };
        }
        self.stage = Stage::Waiting;
        self.retry_at = now.saturating_add(backoff_ms(self.stalled_attempts));
        Action::Wait {
            until: self.retry_at,
            cause,
        }
    }

    /// A probe that established nothing. The session is preserved: the whole
    /// point of this branch is that we must NOT restart, reopen, or re-send.
    fn inconclusive(&mut self, cause: Cause, now: u64) -> Action {
        self.inconclusive_probes = self.inconclusive_probes.saturating_add(1);
        self.last_cause = Some(cause.clone());
        if self.inconclusive_probes >= MAX_INCONCLUSIVE_PROBES {
            self.stage = Stage::Failed;
            // Retryable at the row level: a later drain may probe again. The
            // durable session_uri stays, so nothing is re-uploaded from zero.
            return Action::Fail {
                cause,
                retryable: true,
            };
        }
        self.stage = Stage::Probing;
        self.retry_at = now.saturating_add(backoff_ms(self.inconclusive_probes));
        Action::Wait {
            until: self.retry_at,
            cause,
        }
    }

    /// Next step once the committed prefix is known and trusted.
    fn advance(&mut self) -> Action {
        if self.committed >= self.total_bytes {
            self.stage = Stage::Done;
            return Action::Done {
                committed: self.total_bytes,
            };
        }
        self.stage = Stage::Sending;
        let remaining = self.total_bytes - self.committed;
        let length = remaining.min(self.chunk_bytes);
        Action::SendChunk {
            offset: self.committed,
            length,
        }
    }
}

/// Interpret a status + range observation from either a probe or a chunk PUT.
///
/// `wrote` is how many bytes the host put on the socket for THIS request; it is
/// zero for a probe. It is used only to detect truncation, never to advance.
fn interpret(
    state: &mut TransferState,
    status: u16,
    range: RangeObservation,
    wrote: u64,
    now: u64,
) -> Action {
    match status {
        // The object is complete and immutable. Never re-send, never re-probe.
        200 | 201 => {
            state.commit(state.total_bytes);
            state.stage = Stage::Done;
            Action::Done {
                committed: state.total_bytes,
            }
        }
        308 => match range {
            RangeObservation::Value { end } => {
                // `Range: bytes=0-END` is inclusive, so END+1 bytes are held.
                let committed = end.saturating_add(1);
                if committed > state.total_bytes {
                    // The server claims more than the artifact has. Refuse
                    // rather than "resume" past the end of the file.
                    state.stage = Stage::Failed;
                    return Action::Fail {
                        cause: Cause::LengthMismatch,
                        retryable: false,
                    };
                }
                let truncated = wrote > 0 && committed < state.committed + wrote;
                state.commit(committed);
                if truncated {
                    // The server took less than we wrote. That is a fact, not a
                    // failure: resume from the server's number. Recording the
                    // cause keeps the reason visible without inventing an error.
                    state.last_cause = Some(Cause::Truncated);
                }
                state.advance()
            }
            // A resumable session that answers 308 with no Range header is
            // stating that it holds ZERO bytes. That is a real answer.
            RangeObservation::Absent => {
                state.committed = 0;
                state.sent = 0;
                state.inconclusive_probes = 0;
                state.advance()
            }
            RangeObservation::Malformed => state.inconclusive(Cause::RangeMalformed, now),
            // The host cannot see headers, so it learned nothing. Probing again
            // is the only safe move: re-sending could duplicate committed bytes
            // and restarting could discard them.
            RangeObservation::Unobservable => state.inconclusive(Cause::Inconclusive, now),
        },
        // Proven gone. The caller may mint a new session within its own budget.
        404 | 410 => {
            state.stage = Stage::Failed;
            Action::Reopen {
                cause: Cause::SessionExpired,
            }
        }
        // The session credential itself was rejected. Retrying the same URI
        // cannot help, and reopening is the caller's decision, not ours.
        401 | 403 => {
            state.stage = Stage::Failed;
            Action::Fail {
                cause: Cause::HttpStatus { status },
                retryable: false,
            }
        }
        408 | 425 | 429 => state.stall(Cause::HttpStatus { status }, now),
        s if s >= 500 => state.stall(Cause::HttpStatus { status: s }, now),
        s => {
            state.stage = Stage::Failed;
            Action::Fail {
                cause: Cause::HttpStatus { status: s },
                retryable: false,
            }
        }
    }
}

pub fn apply(
    state: Option<TransferState>,
    plan: Option<TransferPlan>,
    event: Event,
    now: u64,
) -> Result<Reply, String> {
    let mut state = match (state, &event) {
        (Some(existing), _) => {
            if existing.schema != TRANSFER_SCHEMA {
                return Err("Unsupported transfer state schema.".into());
            }
            if existing.committed > existing.total_bytes || existing.chunk_bytes == 0 {
                return Err("Transfer state is internally inconsistent.".into());
            }
            existing
        }
        (None, Event::Start) => {
            let plan = plan.ok_or("A transfer start requires its plan.")?;
            if plan.total_bytes == 0 {
                return Err("A transfer needs at least one byte.".into());
            }
            TransferState::new(&plan)
        }
        (None, _) => return Err("Transfer event arrived without durable state.".into()),
    };

    if matches!(state.stage, Stage::Done) {
        // Idempotent: a duplicate acknowledgement after completion must not
        // reopen, re-send, or fail a transfer that is already committed.
        return Ok(Reply {
            action: Action::Done {
                committed: state.total_bytes,
            },
            state,
        });
    }

    let action = match event {
        // Start and resume are the same operation: always ask the server what
        // it holds before writing a byte. Persisted `committed` is a hint we
        // deliberately do not trust across a process boundary.
        Event::Start => {
            state.stage = Stage::Probing;
            state.sent = state.committed;
            Action::Probe
        }
        Event::ProbeResponse { status, range } => interpret(&mut state, status, range, 0, now),
        Event::ChunkResponse {
            status,
            range,
            bytes_written,
        } => {
            state.sent = state.committed.saturating_add(bytes_written);
            interpret(&mut state, status, range, bytes_written, now)
        }
        // No usable response. If bytes reached the socket the acknowledgement is
        // ambiguous by definition, so the only truthful next step is to ask the
        // server. Assuming success would lose data; assuming failure would
        // re-send bytes the server may already hold.
        Event::TransportFailure {
            cause,
            phase,
            bytes_written,
        } => {
            state.sent = state.committed.saturating_add(bytes_written);
            match cause {
                Cause::Cancelled => {
                    state.stage = Stage::Failed;
                    state.last_cause = Some(Cause::Cancelled);
                    Action::Fail {
                        cause: Cause::Cancelled,
                        retryable: true,
                    }
                }
                Cause::Offline => {
                    state.stage = Stage::Failed;
                    state.last_cause = Some(Cause::Offline);
                    Action::Fail {
                        cause: Cause::Offline,
                        retryable: true,
                    }
                }
                other => {
                    let ambiguous = bytes_written > 0 || matches!(phase, Phase::Response);
                    if ambiguous {
                        state.last_cause = Some(other);
                        state.stalled_attempts = state.stalled_attempts.saturating_add(1);
                        if state.stalled_attempts >= MAX_STALLED_ATTEMPTS {
                            state.stage = Stage::Failed;
                            let cause = state.last_cause.clone().unwrap_or(Cause::Inconclusive);
                            let retryable = cause.retryable();
                            Action::Fail { cause, retryable }
                        } else {
                            state.stage = Stage::Probing;
                            state.retry_at = now.saturating_add(backoff_ms(state.stalled_attempts));
                            Action::Wait {
                                until: state.retry_at,
                                cause: state.last_cause.clone().unwrap_or(Cause::Inconclusive),
                            }
                        }
                    } else {
                        state.stall(other, now)
                    }
                }
            }
        }
        Event::Cancelled => {
            state.stage = Stage::Failed;
            state.last_cause = Some(Cause::Cancelled);
            Action::Fail {
                cause: Cause::Cancelled,
                retryable: true,
            }
        }
        // The bytes arrived but do not match the sealed declaration. This is a
        // terminal conflict: a retry would transfer the same wrong bytes.
        Event::IntegrityFailure { cause } => {
            state.stage = Stage::Failed;
            state.last_cause = Some(cause.clone());
            Action::Fail {
                cause,
                retryable: false,
            }
        }
    };

    Ok(Reply { state, action })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > 64 * 1024 {
        return Err("Transfer command exceeds its byte bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid transfer command: {error}"))?;
    if request.schema != TRANSFER_SCHEMA {
        return Err("Unsupported transfer command schema.".into());
    }
    let reply = apply(request.state, request.plan, request.event, request.now)?;
    serde_json::to_string(&reply).map_err(|error| error.to_string())
}
