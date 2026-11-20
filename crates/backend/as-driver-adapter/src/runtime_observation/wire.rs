//! Decode only the native runtime observation contract, not target JSON or private objects.
use as_types::{
    driver::{NativeRequestOperation, NativeSyncThread},
    main_ports::ModuleError,
    process_tree::{EventKey, ProcessKey},
    runtime_observation::{
        NativeApiSite, NativeCaptureStatus, NativeEventBinding, NativeLibraryCall,
        NativeProjectionError, NativeRequestHandoff, NativeRequestPhase, NativeStreamTryWriteEntry,
        NativeStreamWriteEntry,
    },
};

use bytes::Bytes;

struct Input<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Input<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], ModuleError> {
        let end = self
            .at
            .checked_add(count)
            .ok_or(ModuleError::InvalidInput)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(ModuleError::InvalidInput)?;
        self.at = end;
        Ok(value)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N], ModuleError> {
        self.take(N)?
            .try_into()
            .map_err(|_| ModuleError::InvalidInput)
    }
    fn u32(&mut self) -> Result<u32, ModuleError> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64, ModuleError> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    fn key(&mut self) -> Result<EventKey, ModuleError> {
        let key = EventKey {
            source_instance: self.array()?,
            sequence_domain: self.u64()?,
            sequence: self.u64()?,
        };
        if key.source_instance == [0; 16] || key.sequence_domain == 0 || key.sequence == 0 {
            return Err(ModuleError::InvalidInput);
        }
        Ok(key)
    }
    fn end(self) -> Result<(), ModuleError> {
        if self.at == self.bytes.len() {
            Ok(())
        } else {
            Err(ModuleError::InvalidInput)
        }
    }
}

pub(crate) fn handoff(
    bytes: Vec<u8>,
    boot_id: [u8; 16],
) -> Result<NativeRequestHandoff, ModuleError> {
    let bytes = Bytes::from(bytes);
    let mut input = Input {
        bytes: &bytes,
        at: 0,
    };
    let observation = input.key()?;
    if observation.sequence_domain != 2 {
        return Err(ModuleError::InvalidInput);
    }
    let actor = NativeSyncThread {
        tgid: input.u32()?,
        tid: input.u32()?,
        birth_boottime_ns: input.u64()?,
    };
    let observed_boottime_ns = input.u64()?;
    let presence = input.u32()?;
    let stage = input.u32()?;
    let operation = input.u32()?;
    if actor.tgid == 0 || actor.tid == 0 || presence & !3 != 0 || input.u32()? != 0 {
        return Err(ModuleError::InvalidInput);
    }
    let operation = if presence & 2 != 0 {
        Some(match operation {
            1 => NativeRequestOperation::FileOpen,
            2 => NativeRequestOperation::Execute,
            3 => NativeRequestOperation::Connect,
            4 => NativeRequestOperation::Send,
            _ => return Err(ModuleError::InvalidInput),
        })
    } else {
        if operation != 0 {
            return Err(ModuleError::InvalidInput);
        }
        None
    };
    let request = if presence & 1 != 0 {
        let key = input.key()?;
        if key.sequence_domain != 2 || key.source_instance != observation.source_instance {
            return Err(ModuleError::InvalidInput);
        }
        Some(key)
    } else {
        None
    };
    let phase = match stage {
        1 => {
            let api = input.u32()?;
            let projection_size = input.u32()? as usize;
            let projection_failure = input.u32()?;
            let registers = input.u32()?;
            if registers & !3 != 0
                || projection_failure > 1
                || (projection_size == 0) != (projection_failure == 1)
            {
                return Err(ModuleError::InvalidInput);
            }
            let process = ProcessKey {
                boot_id,
                host_tgid: actor.tgid,
                birth_boottime_ns: input.u64()?,
            };
            let exec_cookie = input.u64()?;
            let instruction_pointer = input.u64()?;
            let stack_pointer = input.u64()?;
            let link_register = if registers & 1 != 0 {
                Some(input.u64()?)
            } else {
                None
            };
            let frame_pointer = if registers & 2 != 0 {
                Some(input.u64()?)
            } else {
                None
            };
            let site = NativeApiSite {
                process,
                exec_cookie,
                instruction_pointer,
                stack_pointer,
                link_register,
                frame_pointer,
            };
            let call = match api {
                1 => {
                    let request_address = input.u64()?;
                    let loop_address = input.u64()?;
                    let path_address = input.u64()?;
                    let flags = i32::from_le_bytes(input.array()?);
                    let mode = i32::from_le_bytes(input.array()?);
                    let callback_address = input.u64()?;
                    let status = input.u32()?;
                    let native_error = i32::from_le_bytes(input.array()?);
                    let length = input.u32()? as usize;
                    let start = input.at;
                    input.take(length)?;
                    let path = bytes.slice(start..input.at);
                    let path_status = match status {
                        0 if native_error == 0 => NativeCaptureStatus::Complete,
                        1 if native_error < 0 => NativeCaptureStatus::ReadFault { native_error },
                        2 if native_error == 0 => NativeCaptureStatus::Capacity,
                        _ => return Err(ModuleError::InvalidInput),
                    };
                    NativeLibraryCall::FileOpen {
                        request_address,
                        loop_address,
                        path_address,
                        path,
                        path_status,
                        flags,
                        mode,
                        callback_address,
                    }
                }
                2 => NativeLibraryCall::Spawn {
                    loop_address: input.u64()?,
                    process_address: input.u64()?,
                    options_address: input.u64()?,
                },
                3 => NativeLibraryCall::TcpConnect {
                    request_address: input.u64()?,
                    stream_address: input.u64()?,
                    address: input.u64()?,
                    callback_address: input.u64()?,
                },
                4 | 6 => {
                    let request_address = input.u64()?;
                    let stream_address = input.u64()?;
                    let buffers_address = input.u64()?;
                    let buffer_count = input.u32()?;
                    if input.u32()? != 0 {
                        return Err(ModuleError::InvalidInput);
                    }
                    let send_stream_address = if api == 4 { Some(input.u64()?) } else { None };
                    let callback_address = input.u64()?;
                    NativeLibraryCall::StreamWrite {
                        entry: if api == 4 {
                            NativeStreamWriteEntry::Write2
                        } else {
                            NativeStreamWriteEntry::Write
                        },
                        request_address,
                        stream_address,
                        buffers_address,
                        buffer_count,
                        send_stream_address,
                        callback_address,
                    }
                }
                7 | 8 => {
                    let stream_address = input.u64()?;
                    let buffers_address = input.u64()?;
                    let buffer_count = input.u32()?;
                    if input.u32()? != 0 {
                        return Err(ModuleError::InvalidInput);
                    }
                    NativeLibraryCall::StreamTryWrite {
                        entry: if api == 7 {
                            NativeStreamTryWriteEntry::TryWrite
                        } else {
                            NativeStreamTryWriteEntry::TryWrite2
                        },
                        stream_address,
                        buffers_address,
                        buffer_count,
                        send_stream_address: if api == 8 { Some(input.u64()?) } else { None },
                    }
                }
                5 => {
                    let request_address = input.u64()?;
                    let stream_address = input.u64()?;
                    let buffers_address = input.u64()?;
                    let buffer_count = input.u32()?;
                    if input.u32()? != 0 {
                        return Err(ModuleError::InvalidInput);
                    }
                    NativeLibraryCall::UdpSend {
                        request_address,
                        stream_address,
                        buffers_address,
                        buffer_count,
                        address: input.u64()?,
                        callback_address: input.u64()?,
                    }
                }
                _ => return Err(ModuleError::InvalidInput),
            };
            let start = input.at;
            input.take(projection_size)?;
            let projection = (projection_size != 0).then(|| bytes.slice(start..input.at));
            let mut projection_error =
                (projection_failure == 1).then_some(NativeProjectionError::Allocation);
            let analysis = if let Some(data) = &projection {
                match super::capture::decode(data, boot_id) {
                    Ok(analysis) => {
                        if analysis
                            .candidates
                            .iter()
                            .any(|candidate| candidate.context.process != site.process)
                        {
                            projection_error = Some(NativeProjectionError::OwnerMismatch);
                        }
                        Some(analysis)
                    }
                    Err(_) => {
                        projection_error = Some(NativeProjectionError::Malformed);
                        None
                    }
                }
            } else {
                None
            };
            NativeRequestPhase::Submission {
                site,
                call,
                projection,
                analysis,
                projection_error,
            }
        }
        2 => NativeRequestPhase::Submit {
            loop_address: input.u64()?,
            work_address: input.u64()?,
            work_callback: input.u64()?,
            done_callback: input.u64()?,
        },
        3 => NativeRequestPhase::WorkEnter {
            work_address: input.u64()?,
        },
        4 => NativeRequestPhase::WorkLeave {
            work_address: input.u64()?,
        },
        5 => {
            let work_address = input.u64()?;
            let present = input.u32()?;
            if present > 2 || input.u32()? != 0 {
                return Err(ModuleError::InvalidInput);
            }
            let result = if present == 1 {
                Some(i64::from_le_bytes(input.array()?))
            } else {
                None
            };
            let result_read_error = if present == 2 {
                let error = i32::from_le_bytes(input.array()?);
                if error >= 0 {
                    return Err(ModuleError::InvalidInput);
                }
                Some(error)
            } else {
                None
            };
            NativeRequestPhase::Done {
                work_address,
                result,
                result_read_error,
            }
        }
        6 => NativeRequestPhase::Cleanup {
            request_address: input.u64()?,
        },
        7 => {
            let event = input.key()?;
            if event.sequence_domain != 1 {
                return Err(ModuleError::InvalidInput);
            }
            let flags = input.u32()?;
            if flags & !3 != 0 || input.u32()? != 0 {
                return Err(ModuleError::InvalidInput);
            }
            let binding = if flags & 1 != 0 {
                NativeEventBinding::ControllerWait {
                    wait_id: input.u64()?,
                }
            } else {
                NativeEventBinding::DirectCheck
            };
            let enclosing_call = if flags & 2 != 0 {
                let key = input.key()?;
                if key.sequence_domain != 2 || key.source_instance != observation.source_instance {
                    return Err(ModuleError::InvalidInput);
                }
                Some(key)
            } else {
                None
            };
            NativeRequestPhase::EventBound {
                event,
                binding,
                enclosing_call,
            }
        }
        8 => {
            let result = i32::from_le_bytes(input.array()?);
            if input.u32()? != 0 {
                return Err(ModuleError::InvalidInput);
            }
            NativeRequestPhase::ApiReturned { result }
        }
        9 => {
            let host_tgid = input.u32()?;
            if host_tgid == 0 || input.u32()? != 0 {
                return Err(ModuleError::InvalidInput);
            }
            NativeRequestPhase::ChildCreated {
                child: ProcessKey {
                    boot_id,
                    host_tgid,
                    birth_boottime_ns: input.u64()?,
                },
            }
        }
        _ => return Err(ModuleError::InvalidInput),
    };
    input.end()?;
    Ok(NativeRequestHandoff {
        observation,
        request,
        operation,
        actor,
        observed_boottime_ns,
        phase,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(output: &mut Vec<u8>, domain: u64, sequence: u64) {
        output.extend([7; 16]);
        output.extend(domain.to_le_bytes());
        output.extend(sequence.to_le_bytes());
    }
    fn header(stage: u32, associated: bool) -> Vec<u8> {
        let mut output = Vec::new();
        key(&mut output, 2, 2);
        output.extend(158_u32.to_le_bytes());
        output.extend(167_u32.to_le_bytes());
        output.extend(0_u64.to_le_bytes());
        output.extend(0_u64.to_le_bytes());
        output.extend(u32::from(associated).to_le_bytes());
        output.extend(stage.to_le_bytes());
        output.extend(0_u32.to_le_bytes());
        output.extend(0_u32.to_le_bytes());
        if associated {
            key(&mut output, 2, 1);
        }
        output
    }
    #[test]
    fn immediate_stream_calls_have_no_invented_request_or_callback_arguments() {
        for api in [7_u32, 8] {
            let mut bytes = header(1, false);
            for value in [api, 0, 1, 0] {
                bytes.extend(value.to_le_bytes());
            }
            for _ in 0..4 {
                bytes.extend(0_u64.to_le_bytes());
            }
            bytes.extend(0_u64.to_le_bytes()); // actual stream
            bytes.extend(0_u64.to_le_bytes()); // actual buffers
            bytes.extend(0_u32.to_le_bytes()); // actual count
            bytes.extend(0_u32.to_le_bytes()); // reserved
            if api == 8 {
                bytes.extend(0_u64.to_le_bytes());
            }
            let fact = handoff(bytes.clone(), [1; 16]).unwrap();
            let NativeRequestPhase::Submission { call, .. } = fact.phase else {
                panic!("submission");
            };
            assert!(matches!(call, NativeLibraryCall::StreamTryWrite {
                stream_address: 0, buffers_address: 0, buffer_count: 0,
                send_stream_address, ..
            } if send_stream_address == (api == 8).then_some(0)));
            let json = serde_json::to_value(call).unwrap();
            assert!(json.get("request_address").is_none());
            assert!(json.get("callback_address").is_none());
            bytes.extend(0_u64.to_le_bytes());
            assert!(handoff(bytes, [1; 16]).is_err());
        }
    }
    #[test]
    fn partial_submission_keeps_bytes_zero_registers_and_allocation_failure() {
        let mut bytes = header(1, false);
        for value in [1_u32, 0, 1, 2] {
            bytes.extend(value.to_le_bytes());
        }
        for value in [0_u64, 0, 0x18eadd0, 0x7fff0, 0] {
            bytes.extend(value.to_le_bytes());
        }
        for value in [0x12b1a4c8_u64, 0, 0x7ff0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend(0_i32.to_le_bytes());
        bytes.extend(0_u64.to_le_bytes());
        let prefix = b"/tmp/\xff";
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend((-14_i32).to_le_bytes());
        bytes.extend((prefix.len() as u32).to_le_bytes());
        let path_start = bytes.len();
        bytes.extend(prefix);
        let truncated = bytes[..bytes.len() - 1].to_vec();
        let original_allocation = bytes.as_ptr();
        let fact = handoff(bytes, [9; 16]).expect("partial original");
        assert!(fact.request.is_none());
        assert!(fact.operation.is_none());
        assert_eq!(fact.actor.birth_boottime_ns, 0);
        assert_eq!(fact.observed_boottime_ns, 0);
        let NativeRequestPhase::Submission {
            site,
            call:
                NativeLibraryCall::FileOpen {
                    path,
                    path_status,
                    flags,
                    mode,
                    loop_address,
                    callback_address,
                    ..
                },
            projection,
            analysis,
            projection_error,
        } = fact.phase
        else {
            panic!("submission");
        };
        assert_eq!(path.as_ref(), prefix);
        assert_eq!(path.as_ptr(), original_allocation.wrapping_add(path_start));
        assert_eq!(
            path_status,
            NativeCaptureStatus::ReadFault { native_error: -14 }
        );
        assert_eq!((flags, mode, loop_address, callback_address), (0, 0, 0, 0));
        assert_eq!(site.link_register, None);
        assert_eq!(site.frame_pointer, Some(0));
        assert!(projection.is_none() && analysis.is_none());
        assert_eq!(projection_error, Some(NativeProjectionError::Allocation));
        assert!(handoff(truncated, [9; 16]).is_err());
    }
    #[test]
    fn actual_done_zero_differs_from_unacquired_and_event_bound_keeps_both_domains() {
        let mut none = header(5, true);
        none.extend(0x12b1a618_u64.to_le_bytes());
        none.extend(0_u32.to_le_bytes());
        none.extend(0_u32.to_le_bytes());
        assert!(matches!(
            handoff(none.clone(), [9; 16])
                .expect("result not acquired")
                .phase,
            NativeRequestPhase::Done { result: None, .. }
        ));
        let mut read_error = none.clone();
        let present = read_error.len() - 8;
        read_error[present..present + 4].copy_from_slice(&2_u32.to_le_bytes());
        read_error.extend((-14_i32).to_le_bytes());
        assert!(matches!(
            handoff(read_error, [9; 16])
                .expect("original read failure retained")
                .phase,
            NativeRequestPhase::Done {
                result: None,
                result_read_error: Some(-14),
                ..
            }
        ));
        let mut zero = none;
        let present = zero.len() - 8;
        zero[present..present + 4].copy_from_slice(&1_u32.to_le_bytes());
        zero.extend(0_i64.to_le_bytes());
        assert!(matches!(
            handoff(zero, [9; 16]).expect("actual zero result").phase,
            NativeRequestPhase::Done {
                result: Some(0),
                ..
            }
        ));
        let mut bound = header(7, true);
        key(&mut bound, 1, 163);
        bound.extend(1_u32.to_le_bytes());
        bound.extend(0_u32.to_le_bytes());
        bound.extend(1_u64.to_le_bytes());
        let fact = handoff(bound, [9; 16]).expect("actual request to LSM handoff");
        assert_eq!(fact.observation.sequence_domain, 2);
        assert_eq!(fact.request.expect("original lifetime").sequence_domain, 2);
        assert!(
            matches!(fact.phase,NativeRequestPhase::EventBound{event,binding:NativeEventBinding::ControllerWait{wait_id:1},..}if event.sequence_domain==1&&event.sequence==163)
        );
    }
}
