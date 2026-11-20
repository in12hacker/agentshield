//! Interpret one native-owned UDP/53 send snapshot; never read the caller's memory again.
use as_types::{
    dispatch_event::{DnsQueryAnalysis, DnsSendSnapshot, EventData, EventPayload},
    main_ports::ModuleError,
};
use domain::base::Message;

pub(crate) fn interpret(event: &mut EventData) -> Result<(), ModuleError> {
    let EventPayload::Network(network) = &mut event.payload else {
        return Ok(());
    };
    let Some(snapshot) = &mut network.dns_snapshot else {
        return Ok(());
    };
    if snapshot.analysis.is_some() {
        return Ok(());
    }
    let (analysis, name) = analyze(snapshot);
    if let Some(name) = name {
        if network
            .target_domain
            .as_ref()
            .is_some_and(|original| *original != name)
        {
            return Err(ModuleError::Conflict);
        }
        network.target_domain.get_or_insert(name);
    }
    snapshot.analysis = Some(analysis);
    Ok(())
}

fn analyze(snapshot: &DnsSendSnapshot) -> (DnsQueryAnalysis, Option<Vec<u8>>) {
    if snapshot.data.len() < snapshot.attempted_bytes as usize {
        return (DnsQueryAnalysis::ShortRead, None);
    }
    if u64::from(snapshot.attempted_bytes) < snapshot.requested_bytes {
        return (DnsQueryAnalysis::CaptureLimited, None);
    }
    let message = match Message::from_octets(snapshot.data.as_slice()) {
        Ok(message) => message,
        Err(_) => return (DnsQueryAnalysis::NotDns, None),
    };
    let header = message.header();
    if header.qr() {
        return (DnsQueryAnalysis::Response, None);
    }
    let opcode = header.opcode().to_int();
    if opcode != 0 {
        return (DnsQueryAnalysis::OtherOpcode { opcode }, None);
    }
    match message.header_counts().qdcount() {
        0 => return (DnsQueryAnalysis::NoQuestion, None),
        1 => {}
        count => return (DnsQueryAnalysis::MultipleQuestions { count }, None),
    }
    match message.sole_question() {
        Ok(question) => (
            DnsQueryAnalysis::Question {
                qtype: question.qtype().to_int(),
                qclass: question.qclass().to_int(),
            },
            Some(question.qname().to_string().into_bytes()),
        ),
        Err(error) => (
            DnsQueryAnalysis::Malformed {
                reason: error.to_string(),
            },
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(data: Vec<u8>) -> DnsSendSnapshot {
        DnsSendSnapshot {
            requested_bytes: data.len() as u64,
            attempted_bytes: data.len() as u32,
            send_flags: 0,
            data,
            analysis: None,
        }
    }
    fn query() -> Vec<u8> {
        b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x05MiXeD\x07Example\x00\x00\x01\x00\x01"
            .to_vec()
    }
    #[test]
    fn mature_query_parser_preserves_case_and_the_original_wire() {
        let input = snapshot(query());
        let original = input.data.clone();
        assert_eq!(
            analyze(&input),
            (
                DnsQueryAnalysis::Question {
                    qtype: 1,
                    qclass: 1
                },
                Some(b"MiXeD.Example".to_vec()),
            )
        );
        assert_eq!(input.data, original);
        let root = snapshot(
            b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x01\x00\x01".to_vec(),
        );
        assert_eq!(
            analyze(&root).1,
            Some(Vec::new()),
            "obtained empty root name is not missing"
        );
    }
    #[test]
    fn non_query_and_incomplete_observations_keep_their_own_status() {
        let mut input = snapshot(query());
        input.data[2] |= 0x80;
        assert_eq!(analyze(&input), (DnsQueryAnalysis::Response, None));
        input.data[2] = 0x08;
        assert_eq!(
            analyze(&input),
            (DnsQueryAnalysis::OtherOpcode { opcode: 1 }, None)
        );
        input.data[2] = 0;
        input.data[5] = 0;
        assert_eq!(analyze(&input), (DnsQueryAnalysis::NoQuestion, None));
        input.data[5] = 2;
        assert_eq!(
            analyze(&input),
            (DnsQueryAnalysis::MultipleQuestions { count: 2 }, None)
        );
        assert_eq!(
            analyze(&snapshot(vec![0, 1])),
            (DnsQueryAnalysis::NotDns, None)
        );
        let malformed = snapshot(
            b"\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\xc0\x0c\x00\x01\x00\x01".to_vec(),
        );
        assert!(matches!(
            analyze(&malformed),
            (DnsQueryAnalysis::Malformed { .. }, None)
        ));
        let mut short = snapshot(query());
        short.data.pop();
        assert_eq!(analyze(&short), (DnsQueryAnalysis::ShortRead, None));
        let mut limited = snapshot(query());
        limited.requested_bytes += 1;
        assert_eq!(analyze(&limited), (DnsQueryAnalysis::CaptureLimited, None));
    }
}
