// Copyright (C) 2019-2020  Pierre Krieger
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Implements the time interface.

use futures::{channel::mpsc, lock::Mutex, prelude::*, stream::FuturesUnordered};
use futures_timer::Delay;
use redshirt_core::native::{
    DummyMessageIdWrite, NativeProgramEvent, NativeProgramMessageIdWrite, NativeProgramRef,
};
use redshirt_core::{Decode as _, Encode as _, EncodedMessage, InterfaceHash, MessageId, Pid};
use redshirt_system_time_interface::ffi as system_time_ffi;
use redshirt_time_interface::ffi as time_ffi;
use std::{
    convert::TryFrom,
    pin::Pin,
    sync::atomic,
    time::{Duration, Instant, SystemTime},
};

/// State machine for `time` interface messages handling.
pub struct TimerHandler {
    /// If true, we have sent the time interface registration message.
    time_registered: atomic::AtomicBool,
    /// If true, we have sent the system-time interface registration message.
    system_time_registered: atomic::AtomicBool,
    /// Accessed only by `next_event`.
    inner: Mutex<TimerHandlerInner>,
    /// Send on this channel the received interface messages.
    messages_tx: mpsc::UnboundedSender<(Message, MessageId)>,
}

/// Separate struct behind a mutex.
struct TimerHandlerInner {
    /// Stream of message IDs to answer.
    timers: FuturesUnordered<Pin<Box<dyn Future<Output = MessageId> + Send>>>, // TODO: meh for boxing
    /// Receiving side of [`TimerHandler::messages_tx`].
    messages_rx: mpsc::UnboundedReceiver<(Message, MessageId)>,
}

enum Message {
    Time(time_ffi::TimeMessage),
    SystemTime(system_time_ffi::TimeMessage),
}

impl TimerHandler {
    /// Initializes the new state machine for timers.
    pub fn new() -> Self {
        let (messages_tx, messages_rx) = mpsc::unbounded();

        TimerHandler {
            time_registered: atomic::AtomicBool::new(false),
            system_time_registered: atomic::AtomicBool::new(false),
            inner: Mutex::new(TimerHandlerInner {
                timers: {
                    let timers =
                        FuturesUnordered::<Pin<Box<dyn Future<Output = MessageId> + Send>>>::new();
                    // TODO: ugh; pushing a never-ending future, otherwise we get a permanent `None` when polling
                    timers.push(Box::pin(async move {
                        loop {
                            futures::pending!()
                        }
                    }));
                    timers
                },
                messages_rx,
            }),
            messages_tx,
        }
    }
}

impl<'a> NativeProgramRef<'a> for &'a TimerHandler {
    type Future =
        Pin<Box<dyn Future<Output = NativeProgramEvent<Self::MessageIdWrite>> + Send + 'a>>;
    type MessageIdWrite = DummyMessageIdWrite;

    fn next_event(self) -> Self::Future {
        Box::pin(async move {
            if !self.time_registered.swap(true, atomic::Ordering::Relaxed) {
                return NativeProgramEvent::Emit {
                    interface: redshirt_interface_interface::ffi::INTERFACE,
                    message_id_write: None,
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        time_ffi::INTERFACE,
                    )
                    .encode(),
                };
            }

            if !self
                .system_time_registered
                .swap(true, atomic::Ordering::Relaxed)
            {
                return NativeProgramEvent::Emit {
                    interface: redshirt_interface_interface::ffi::INTERFACE,
                    message_id_write: None,
                    message: redshirt_interface_interface::ffi::InterfaceMessage::Register(
                        system_time_ffi::INTERFACE,
                    )
                    .encode(),
                };
            }

            let mut inner = self.inner.lock().await;
            let inner = &mut *inner;

            loop {
                match future::select(inner.timers.next(), inner.messages_rx.next()).await {
                    future::Either::Left((Some(message_id), _)) => {
                        return NativeProgramEvent::Answer {
                            message_id,
                            answer: Ok(().encode()),
                        };
                    }
                    future::Either::Right((Some((Message::Time(time_message), message_id)), _)) => {
                        match time_message {
                            time_ffi::TimeMessage::GetMonotonic => {
                                return NativeProgramEvent::Answer {
                                    message_id,
                                    answer: Ok(monotonic_clock().encode()),
                                };
                            }
                            time_ffi::TimeMessage::WaitMonotonic(until) => {
                                match until.checked_sub(monotonic_clock()) {
                                    None => {
                                        return NativeProgramEvent::Answer {
                                            message_id,
                                            answer: Ok(().encode()),
                                        }
                                    }
                                    Some(dur_from_now) => {
                                        // If `dur_from_now` is larger than a `u64`, we simply don't insert any timer.
                                        // We assume that we will never reach this time ever.
                                        if let Ok(dur) = u64::try_from(dur_from_now) {
                                            let delay = Delay::new(Duration::from_nanos(dur));
                                            inner.timers.push(Box::pin(async move {
                                                delay.await;
                                                message_id
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    future::Either::Right((
                        Some((Message::SystemTime(time_message), message_id)),
                        _,
                    )) => match time_message {
                        system_time_ffi::TimeMessage::GetSystem => {
                            return NativeProgramEvent::Answer {
                                message_id,
                                answer: Ok(system_clock().encode()),
                            };
                        }
                    },
                    future::Either::Left((None, _)) => unreachable!(),
                    future::Either::Right((None, _)) => unreachable!(),
                }
            }
        })
    }

    fn interface_message(
        self,
        interface: InterfaceHash,
        message_id: Option<MessageId>,
        emitter_pid: Pid,
        message: EncodedMessage,
    ) {
        if interface == time_ffi::INTERFACE {
            match time_ffi::TimeMessage::decode(message) {
                Ok(msg) => {
                    self.messages_tx
                        .unbounded_send((Message::Time(msg), message_id.unwrap()))
                        .unwrap();
                }
                Err(_) => {}
            }
        } else if interface == system_time_ffi::INTERFACE {
            match system_time_ffi::TimeMessage::decode(message) {
                Ok(msg) => {
                    self.messages_tx
                        .unbounded_send((Message::SystemTime(msg), message_id.unwrap()))
                        .unwrap();
                }
                Err(_) => {}
            }
        } else {
            unreachable!()
        }
    }

    fn process_destroyed(self, _: Pid) {}

    fn message_response(self, _: MessageId, _: Result<EncodedMessage, ()>) {
        unreachable!()
    }
}

fn monotonic_clock() -> u128 {
    lazy_static::lazy_static! {
        static ref CLOCK_START: Instant = Instant::now();
    }
    let start = *CLOCK_START;
    duration_to_u128(start.elapsed())
}

fn system_clock() -> u128 {
    duration_to_u128(
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap(),
    )
}

fn duration_to_u128(duration: Duration) -> u128 {
    u128::from(duration.as_secs() * 1_000_000_000) + u128::from(duration.subsec_nanos())
}
