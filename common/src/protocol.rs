use anyhow::{Result, bail};
use std::io::{Read, Write};

// --- Client messages ---

pub enum ClientMsg {
    AudioSegment(Vec<i16>), // tag 0x01, payload = raw i16 LE bytes
}

// --- Server messages ---

#[derive(Debug)]
pub enum ServerMsg {
    Ready,        // tag 0x80, length = 0
    Text(String), // tag 0x81, payload = UTF-8
    Error(String), // tag 0x82, payload = UTF-8
}

// --- Wire format: [tag: u8][length: u32 LE][payload] ---

pub fn write_client_msg(w: &mut impl Write, msg: &ClientMsg) -> Result<()> {
    match msg {
        ClientMsg::AudioSegment(samples) => {
            let payload_len = samples.len() * 2; // i16 = 2 bytes
            w.write_all(&[0x01])?;
            w.write_all(&(payload_len as u32).to_le_bytes())?;
            for &s in samples {
                w.write_all(&s.to_le_bytes())?;
            }
            w.flush()?;
        }
    }
    Ok(())
}

pub fn read_client_msg(r: &mut impl Read) -> Result<ClientMsg> {
    let mut tag = [0u8; 1];
    r.read_exact(&mut tag)?;

    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;

    match tag[0] {
        0x01 => {
            if !len.is_multiple_of(2) {
                bail!("AudioSegment payload length {len} is not a multiple of 2");
            }
            let mut payload = vec![0u8; len];
            r.read_exact(&mut payload)?;
            let samples: Vec<i16> = payload
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
            Ok(ClientMsg::AudioSegment(samples))
        }
        other => bail!("Unknown client message tag: 0x{other:02x}"),
    }
}

pub fn write_server_msg(w: &mut impl Write, msg: &ServerMsg) -> Result<()> {
    match msg {
        ServerMsg::Ready => {
            w.write_all(&[0x80])?;
            w.write_all(&0u32.to_le_bytes())?;
            w.flush()?;
        }
        ServerMsg::Text(text) => {
            let payload = text.as_bytes();
            w.write_all(&[0x81])?;
            w.write_all(&(payload.len() as u32).to_le_bytes())?;
            w.write_all(payload)?;
            w.flush()?;
        }
        ServerMsg::Error(text) => {
            let payload = text.as_bytes();
            w.write_all(&[0x82])?;
            w.write_all(&(payload.len() as u32).to_le_bytes())?;
            w.write_all(payload)?;
            w.flush()?;
        }
    }
    Ok(())
}

pub fn read_server_msg(r: &mut impl Read) -> Result<ServerMsg> {
    let mut tag = [0u8; 1];
    r.read_exact(&mut tag)?;

    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;

    match tag[0] {
        0x80 => {
            if len > 0 {
                let mut discard = vec![0u8; len];
                r.read_exact(&mut discard)?;
            }
            Ok(ServerMsg::Ready)
        }
        0x81 => {
            let mut payload = vec![0u8; len];
            r.read_exact(&mut payload)?;
            Ok(ServerMsg::Text(String::from_utf8(payload)?))
        }
        0x82 => {
            let mut payload = vec![0u8; len];
            r.read_exact(&mut payload)?;
            Ok(ServerMsg::Error(String::from_utf8(payload)?))
        }
        other => bail!("Unknown server message tag: 0x{other:02x}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_audio_segment() {
        let samples: Vec<i16> = vec![-32768, -1, 0, 1, 32767];
        let mut buf = Vec::new();
        write_client_msg(&mut buf, &ClientMsg::AudioSegment(samples.clone())).unwrap();

        let mut cursor = Cursor::new(buf);
        let msg = read_client_msg(&mut cursor).unwrap();
        match msg {
            ClientMsg::AudioSegment(decoded) => assert_eq!(decoded, samples),
        }
    }

    #[test]
    fn round_trip_audio_segment_empty() {
        let samples: Vec<i16> = vec![];
        let mut buf = Vec::new();
        write_client_msg(&mut buf, &ClientMsg::AudioSegment(samples.clone())).unwrap();

        let mut cursor = Cursor::new(buf);
        let msg = read_client_msg(&mut cursor).unwrap();
        match msg {
            ClientMsg::AudioSegment(decoded) => assert_eq!(decoded, samples),
        }
    }

    #[test]
    fn round_trip_ready() {
        let mut buf = Vec::new();
        write_server_msg(&mut buf, &ServerMsg::Ready).unwrap();

        let mut cursor = Cursor::new(buf);
        let msg = read_server_msg(&mut cursor).unwrap();
        assert!(matches!(msg, ServerMsg::Ready));
    }

    #[test]
    fn round_trip_text() {
        let text = "Bonjour, ça va bien !".to_string();
        let mut buf = Vec::new();
        write_server_msg(&mut buf, &ServerMsg::Text(text.clone())).unwrap();

        let mut cursor = Cursor::new(buf);
        let msg = read_server_msg(&mut cursor).unwrap();
        match msg {
            ServerMsg::Text(decoded) => assert_eq!(decoded, text),
            other => panic!("Expected Text, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_error() {
        let text = "model not found".to_string();
        let mut buf = Vec::new();
        write_server_msg(&mut buf, &ServerMsg::Error(text.clone())).unwrap();

        let mut cursor = Cursor::new(buf);
        let msg = read_server_msg(&mut cursor).unwrap();
        match msg {
            ServerMsg::Error(decoded) => assert_eq!(decoded, text),
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_text_empty() {
        let mut buf = Vec::new();
        write_server_msg(&mut buf, &ServerMsg::Text(String::new())).unwrap();

        let mut cursor = Cursor::new(buf);
        let msg = read_server_msg(&mut cursor).unwrap();
        match msg {
            ServerMsg::Text(decoded) => assert_eq!(decoded, ""),
            other => panic!("Expected Text, got {other:?}"),
        }
    }

    #[test]
    fn multiple_messages_in_stream() {
        let mut buf = Vec::new();
        write_server_msg(&mut buf, &ServerMsg::Ready).unwrap();
        write_server_msg(&mut buf, &ServerMsg::Text("hello".into())).unwrap();
        write_server_msg(&mut buf, &ServerMsg::Error("oops".into())).unwrap();

        let mut cursor = Cursor::new(buf);
        assert!(matches!(read_server_msg(&mut cursor).unwrap(), ServerMsg::Ready));
        match read_server_msg(&mut cursor).unwrap() {
            ServerMsg::Text(t) => assert_eq!(t, "hello"),
            other => panic!("Expected Text, got {other:?}"),
        }
        match read_server_msg(&mut cursor).unwrap() {
            ServerMsg::Error(e) => assert_eq!(e, "oops"),
            other => panic!("Expected Error, got {other:?}"),
        }
    }

    #[test]
    fn unknown_client_tag_errors() {
        let buf = vec![0xFF, 0, 0, 0, 0]; // unknown tag, length 0
        let mut cursor = Cursor::new(buf);
        assert!(read_client_msg(&mut cursor).is_err());
    }

    #[test]
    fn unknown_server_tag_errors() {
        let buf = vec![0xFF, 0, 0, 0, 0];
        let mut cursor = Cursor::new(buf);
        assert!(read_server_msg(&mut cursor).is_err());
    }
}
