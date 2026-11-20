//! Framing for the observed HTTP/1.1 request/response streams.
use std::{borrow::Cow, str};

pub(super) const MESSAGE_MAX: usize = 2 * 1024 * 1024;
const HEAD_MAX: usize = 16 * 1024;

pub(super) struct Message<'a> {
    pub raw: &'a [u8],
    body: Body<'a>,
    pub traceparent: Option<&'a str>,
    pub encoding: Option<&'a str>,
    pub host: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub upgrade: Option<&'a str>,
    pub connection: Option<&'a str>,
    pub websocket_key: Option<&'a str>,
    pub websocket_accept: Option<&'a str>,
    pub websocket_version: Option<&'a str>,
    pub websocket_extensions: Option<&'a str>,
}

impl Message<'_> {
    /// Protocol consumers see the logical body. Ownership and storage use the full wire message.
    pub(super) fn body(&self) -> Result<Cow<'_, [u8]>, ()> {
        match self.body {
            Body::Identity(data) => Ok(Cow::Borrowed(data)),
            Body::Chunked(data) => {
                let mut body = Vec::new();
                visit_chunks(data, |chunk| body.extend_from_slice(chunk))?.ok_or(())?;
                Ok(Cow::Owned(body))
            }
        }
    }
}

enum Body<'a> {
    Identity(&'a [u8]),
    Chunked(&'a [u8]),
}

/// Walk the original chunk framing without allocating a partial logical body.
fn visit_chunks(data: &[u8], mut emit: impl FnMut(&[u8])) -> Result<Option<usize>, ()> {
    let mut cursor = 0;
    loop {
        let remaining = &data[cursor..];
        if remaining
            .first()
            .is_some_and(|byte| !byte.is_ascii_hexdigit())
        {
            return Err(());
        }
        let httparse::Status::Complete((line, size)) =
            httparse::parse_chunk_size(remaining.get(..HEAD_MAX).unwrap_or(remaining))
                .map_err(|_| ())?
        else {
            return if remaining.len() < HEAD_MAX {
                Ok(None)
            } else {
                Err(())
            };
        };
        if remaining[..line - 2].contains(&b'\n') {
            return Err(());
        }
        cursor += line;
        if size == 0 {
            let mut trailers = [httparse::EMPTY_HEADER; 64];
            let remaining = &data[cursor..];
            let httparse::Status::Complete((end, _)) = httparse::parse_headers(
                remaining.get(..HEAD_MAX).unwrap_or(remaining),
                &mut trailers,
            )
            .map_err(|_| ())?
            else {
                return if remaining.len() < HEAD_MAX {
                    Ok(None)
                } else {
                    Err(())
                };
            };
            if remaining[..end]
                .split_inclusive(|byte| *byte == b'\n')
                .any(|line| !line.ends_with(b"\r\n"))
            {
                return Err(());
            }
            return Ok(Some(cursor + end));
        }
        let end = cursor
            .checked_add(usize::try_from(size).map_err(|_| ())?)
            .ok_or(())?;
        let next = end
            .checked_add(2)
            .filter(|next| *next <= MESSAGE_MAX)
            .ok_or(())?;
        let Some(delimiter) = data.get(end..next) else {
            return Ok(None);
        };
        if delimiter != b"\r\n" {
            return Err(());
        }
        emit(&data[cursor..end]);
        cursor = next;
    }
}

pub(super) enum Start<'a> {
    Request { method: &'a str, target: &'a str },
    Response(u16),
}

pub(super) fn read(
    data: &[u8],
    response_to_head: bool,
    request: bool,
) -> Result<Option<(Start<'_>, Message<'_>)>, ()> {
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let (length, start, headers) = if request {
        let mut req = httparse::Request::new(&mut headers);
        let httparse::Status::Complete(length) = req.parse(data).map_err(|_| ())? else {
            return if data.len() < HEAD_MAX {
                Ok(None)
            } else {
                Err(())
            };
        };
        if req.version != Some(1) {
            return Err(());
        }
        (
            length,
            Start::Request {
                method: req.method.ok_or(())?,
                target: req.path.ok_or(())?,
            },
            req.headers,
        )
    } else {
        let mut reply = httparse::Response::new(&mut headers);
        let httparse::Status::Complete(length) = reply.parse(data).map_err(|_| ())? else {
            return if data.len() < HEAD_MAX {
                Ok(None)
            } else {
                Err(())
            };
        };
        if reply.version != Some(1) {
            return Err(());
        }
        (
            length,
            Start::Response(reply.code.ok_or(())?),
            reply.headers,
        )
    };
    if length > HEAD_MAX {
        return Err(());
    }
    let header = |name: &str| -> Result<Option<&str>, ()> {
        let mut matches = headers.iter().filter(|h| h.name.eq_ignore_ascii_case(name));
        let first = matches
            .next()
            .map(|h| str::from_utf8(h.value).map(str::trim))
            .transpose()
            .map_err(|_| ())?;
        if matches.next().is_some() {
            return Err(());
        }
        Ok(first)
    };
    let upgrade = header("upgrade")?;
    let transfer = header("transfer-encoding")?;
    let explicit = header("content-length")?
        .map(|n| {
            if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                return Err(());
            }
            n.parse::<usize>().map_err(|_| ())
        })
        .transpose()?;
    if transfer.is_some_and(|value| !value.eq_ignore_ascii_case("chunked"))
        || (transfer.is_some() && explicit.is_some())
    {
        return Err(());
    }
    if upgrade.is_some() && (transfer.is_some() || explicit.is_some_and(|length| length != 0)) {
        return Err(());
    }
    let remaining = &data[length..];
    let chunked = transfer.is_some();
    let wire_limit = MESSAGE_MAX - length;
    let body_len = match start {
        Start::Response(101) if upgrade.is_none() => return Err(()),
        Start::Response(100..=199 | 204) if chunked => return Err(()),
        Start::Response(100..=199 | 204 | 304) => 0,
        Start::Response(_) if response_to_head => 0,
        _ if chunked => {
            match visit_chunks(remaining.get(..wire_limit).unwrap_or(remaining), |_| {})? {
                Some(end) => end,
                None if remaining.len() < wire_limit => return Ok(None),
                None => return Err(()),
            }
        }
        Start::Response(_) => explicit.ok_or(())?,
        Start::Request { .. } => explicit.unwrap_or(0),
    };
    let end = length
        .checked_add(body_len)
        .filter(|end| *end <= MESSAGE_MAX)
        .ok_or(())?;
    let Some(raw) = data.get(..end) else {
        return Ok(None);
    };
    // Duplicated traceparent never supplies a tracing edge. The original bytes remain available.
    let traceparent = header("traceparent").unwrap_or(None);
    Ok(Some((
        start,
        Message {
            raw,
            body: if chunked && body_len != 0 {
                Body::Chunked(&raw[length..])
            } else {
                Body::Identity(&raw[length..])
            },
            traceparent,
            host: header("host")?,
            encoding: header("content-encoding")?,
            content_type: header("content-type")?,
            upgrade,
            connection: if upgrade.is_some() {
                header("connection")?
            } else {
                None
            },
            websocket_key: header("sec-websocket-key")?,
            websocket_accept: header("sec-websocket-accept")?,
            websocket_version: header("sec-websocket-version")?,
            websocket_extensions: header("sec-websocket-extensions")?,
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_message_requires_terminator_and_preserves_wire_boundary() {
        let wire = b"POST /v1/traces HTTP/1.1\r\nTransfer-Encoding: Chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\ntraceparent: ignored\r\n\r\n";
        for end in 0..wire.len() {
            assert!(read(&wire[..end], false, true).unwrap().is_none());
        }
        let next = b"GET / HTTP/1.1\r\n\r\n";
        let pipeline = [wire.as_slice(), next].concat();
        let (_, message) = read(&pipeline, false, true).unwrap().unwrap();
        assert_eq!(message.raw, wire);
        assert_eq!(message.body().unwrap().as_ref(), b"abcde");
        assert_eq!(message.traceparent, None);
        assert_eq!(
            read(&pipeline[wire.len()..], false, true)
                .unwrap()
                .unwrap()
                .1
                .raw,
            next
        );
    }

    #[test]
    fn rejects_conflicting_and_invalid_chunk_framing() {
        let header = b"POST /v1/traces HTTP/1.1\r\nTransfer-Encoding: chunked\r\n";
        for rest in [
            b"Content-Length: 0\r\n\r\n0\r\n\r\n".as_slice(),
            b"Transfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
            b"\r\n\r\n\r\n",
            b"\r\n1\r\naX\n0\r\n\r\n",
            b"\r\n0\r\nfield: value\n\r\n",
            b"\r\n200000\r\n",
        ] {
            assert!(read(&[header.as_slice(), rest].concat(), false, true).is_err());
        }
    }
}
