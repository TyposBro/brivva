use super::errors::ProtocolError;
use super::types::{AudioFrame, ChunkKind, Message, MessageType, Ping, SessionInit, StreamEnd, VideoChunk};

pub fn parse_message(frame: &[u8]) -> Result<Message, ProtocolError> {
    let (&msg_type, body) = frame
        .split_first()
        .ok_or(ProtocolError::FrameTooShort { expected: 1, actual: 0 })?;

    match msg_type {
        x if x == MessageType::SessionInit as u8 => parse_session_init(body).map(Message::SessionInit),
        x if x == MessageType::AudioFrame as u8 => parse_audio_frame(body).map(Message::AudioFrame),
        x if x == MessageType::VideoChunk as u8 => parse_video_chunk(body).map(Message::VideoChunk),
        x if x == MessageType::StreamEnd as u8 => parse_stream_end(body).map(Message::StreamEnd),
        x if x == MessageType::Ping as u8 => parse_ping(body).map(Message::Ping),
        other => Err(ProtocolError::UnknownType(other)),
    }
}

fn parse_session_init(body: &[u8]) -> Result<SessionInit, ProtocolError> {
    require_len(body, 24)?;
    let metadata = &body[24..];
    let metadata_json = std::str::from_utf8(metadata)
        .map_err(|_| ProtocolError::InvalidUtf8)?
        .to_string();
    serde_json::from_str::<serde_json::Value>(&metadata_json).map_err(|_| ProtocolError::InvalidJson)?;

    Ok(SessionInit {
        version: read_u16(body, 0),
        flags: read_u16(body, 2),
        audio_sample_rate: read_u32(body, 4),
        audio_channels: read_u16(body, 8),
        audio_frame_duration_ms: read_u16(body, 10),
        video_timescale: read_u16(body, 12),
        session_start_unix_ms: read_u64(body, 16),
        metadata_json,
    })
}

fn parse_audio_frame(body: &[u8]) -> Result<AudioFrame, ProtocolError> {
    require_len(body, 24)?;
    let payload_size = read_u32(body, 20) as usize;
    ensure_payload_len(body, 24, payload_size)?;

    Ok(AudioFrame {
        seq: read_u64(body, 0),
        capture_ts_ms: read_u64(body, 8),
        duration_ms: read_u32(body, 16),
        pcm: body[24..].to_vec(),
    })
}

fn parse_video_chunk(body: &[u8]) -> Result<VideoChunk, ProtocolError> {
    require_len(body, 28)?;
    let payload_size = read_u32(body, 24) as usize;
    ensure_payload_len(body, 28, payload_size)?;
    let chunk_kind = match body[21] {
        0 => ChunkKind::Init,
        1 => ChunkKind::Media,
        other => return Err(ProtocolError::InvalidChunkKind(other)),
    };

    Ok(VideoChunk {
        seq: read_u64(body, 0),
        capture_ts_ms: read_u64(body, 8),
        duration_ms: read_u32(body, 16),
        is_keyframe: body[20] != 0,
        chunk_kind,
        bytes: body[28..].to_vec(),
    })
}

fn parse_stream_end(body: &[u8]) -> Result<StreamEnd, ProtocolError> {
    require_len(body, 8)?;
    Ok(StreamEnd {
        reason_code: read_u32(body, 0),
    })
}

fn parse_ping(body: &[u8]) -> Result<Ping, ProtocolError> {
    require_len(body, 8)?;
    Ok(Ping {
        client_time_ms: read_u64(body, 0),
    })
}

fn require_len(bytes: &[u8], expected: usize) -> Result<(), ProtocolError> {
    if bytes.len() < expected {
        return Err(ProtocolError::FrameTooShort {
            expected,
            actual: bytes.len(),
        });
    }
    Ok(())
}

fn ensure_payload_len(bytes: &[u8], header_len: usize, payload_size: usize) -> Result<(), ProtocolError> {
    let actual = bytes.len().saturating_sub(header_len);
    if actual != payload_size {
        return Err(ProtocolError::PayloadSizeMismatch {
            declared: payload_size,
            actual,
        });
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
        bytes[offset + 4],
        bytes[offset + 5],
        bytes[offset + 6],
        bytes[offset + 7],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_session_init() {
        let metadata = br#"{"video_codec":"h264"}"#;
        let mut frame = vec![MessageType::SessionInit as u8];
        frame.extend_from_slice(&1u16.to_le_bytes());
        frame.extend_from_slice(&3u16.to_le_bytes());
        frame.extend_from_slice(&44100u32.to_le_bytes());
        frame.extend_from_slice(&1u16.to_le_bytes());
        frame.extend_from_slice(&20u16.to_le_bytes());
        frame.extend_from_slice(&1000u16.to_le_bytes());
        frame.extend_from_slice(&0u16.to_le_bytes());
        frame.extend_from_slice(&123u64.to_le_bytes());
        frame.extend_from_slice(metadata);

        let parsed = parse_message(&frame).unwrap();

        assert_eq!(
            parsed,
            Message::SessionInit(SessionInit {
                version: 1,
                flags: 3,
                audio_sample_rate: 44100,
                audio_channels: 1,
                audio_frame_duration_ms: 20,
                video_timescale: 1000,
                session_start_unix_ms: 123,
                metadata_json: String::from_utf8(metadata.to_vec()).unwrap(),
            })
        );
    }

    #[test]
    fn parses_audio_frame() {
        let payload = vec![7u8; 1764];
        let mut frame = vec![MessageType::AudioFrame as u8];
        frame.extend_from_slice(&9u64.to_le_bytes());
        frame.extend_from_slice(&40u64.to_le_bytes());
        frame.extend_from_slice(&20u32.to_le_bytes());
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&payload);

        let parsed = parse_message(&frame).unwrap();

        assert_eq!(
            parsed,
            Message::AudioFrame(AudioFrame {
                seq: 9,
                capture_ts_ms: 40,
                duration_ms: 20,
                pcm: payload,
            })
        );
    }

    #[test]
    fn parses_video_chunk() {
        let payload = vec![1, 2, 3, 4];
        let mut frame = vec![MessageType::VideoChunk as u8];
        frame.extend_from_slice(&11u64.to_le_bytes());
        frame.extend_from_slice(&60u64.to_le_bytes());
        frame.extend_from_slice(&33u32.to_le_bytes());
        frame.push(1);
        frame.push(1);
        frame.extend_from_slice(&0u16.to_le_bytes());
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&payload);

        let parsed = parse_message(&frame).unwrap();

        assert_eq!(
            parsed,
            Message::VideoChunk(VideoChunk {
                seq: 11,
                capture_ts_ms: 60,
                duration_ms: 33,
                is_keyframe: true,
                chunk_kind: ChunkKind::Media,
                bytes: payload,
            })
        );
    }

    #[test]
    fn rejects_unknown_type() {
        let err = parse_message(&[0xff]).unwrap_err();
        assert_eq!(err, ProtocolError::UnknownType(0xff));
    }

    #[test]
    fn rejects_payload_size_mismatch() {
        let mut frame = vec![MessageType::AudioFrame as u8];
        frame.extend_from_slice(&1u64.to_le_bytes());
        frame.extend_from_slice(&20u64.to_le_bytes());
        frame.extend_from_slice(&20u32.to_le_bytes());
        frame.extend_from_slice(&10u32.to_le_bytes());
        frame.extend_from_slice(&[1, 2, 3]);

        let err = parse_message(&frame).unwrap_err();
        assert_eq!(
            err,
            ProtocolError::PayloadSizeMismatch {
                declared: 10,
                actual: 3,
            }
        );
    }

    #[test]
    fn rejects_invalid_chunk_kind() {
        let mut frame = vec![MessageType::VideoChunk as u8];
        frame.extend_from_slice(&1u64.to_le_bytes());
        frame.extend_from_slice(&0u64.to_le_bytes());
        frame.extend_from_slice(&33u32.to_le_bytes());
        frame.push(0);
        frame.push(9);
        frame.extend_from_slice(&0u16.to_le_bytes());
        frame.extend_from_slice(&0u32.to_le_bytes());

        let err = parse_message(&frame).unwrap_err();
        assert_eq!(err, ProtocolError::InvalidChunkKind(9));
    }
}
