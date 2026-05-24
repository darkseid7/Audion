// SlimProto binary codec — encode server commands, decode client messages.
//
// Wire format (asymmetric):
//   Server→Client: [u16 BE length][4-byte tag][data]   (length includes tag+data)
//   Client→Server: [4-byte tag][u32 BE payload length][payload]

use std::fmt;

// ── Server → Client commands ────────────────────────────────────────────────

/// Build a complete server→client frame: [u16 BE len][tag][data].
fn server_frame(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let frame_len = (4 + data.len()) as u16;
    let mut buf = Vec::with_capacity(2 + frame_len as usize);
    buf.extend_from_slice(&frame_len.to_be_bytes());
    buf.extend_from_slice(tag);
    buf.extend_from_slice(data);
    buf
}

/// `strm` sub-commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrmCommand {
    Start,      // 's'
    Pause,      // 'p'
    Unpause,    // 'u'
    Stop,       // 'q'
    Flush,      // 'f'
    Status,     // 't'
    Skip,       // 'a'
}

impl StrmCommand {
    fn as_byte(self) -> u8 {
        match self {
            Self::Start => b's',
            Self::Pause => b'p',
            Self::Unpause => b'u',
            Self::Stop => b'q',
            Self::Flush => b'f',
            Self::Status => b't',
            Self::Skip => b'a',
        }
    }
}

/// Audio format hint sent to the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Pcm,
    Mp3,
    Flac,
    Wma,
    Ogg,
    Aac,
    Alac,
}

impl AudioFormat {
    pub fn as_byte(self) -> u8 {
        match self {
            Self::Pcm => b'p',
            Self::Mp3 => b'm',
            Self::Flac => b'f',
            Self::Wma => b'w',
            Self::Ogg => b'o',
            Self::Aac => b'a',
            Self::Alac => b'l',
        }
    }

    /// Infer from file extension.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_ascii_lowercase().as_str() {
            "flac" => Self::Flac,
            "mp3" => Self::Mp3,
            "ogg" | "oga" => Self::Ogg,
            "aac" | "m4a" => Self::Aac,
            "wav" | "wave" | "aif" | "aiff" => Self::Pcm,
            "wma" => Self::Wma,
            "alac" => Self::Alac,
            _ => Self::Pcm,
        }
    }
}

/// Parameters for `strm 's'` (start streaming).
pub struct StrmStartParams {
    pub format: AudioFormat,
    pub server_port: u16,
    pub server_ip: [u8; 4],      // 0.0.0.0 = use control connection IP
    pub http_request: String,     // e.g. "GET /stream?player=MAC HTTP/1.0\r\n\r\n"
    pub replay_gain: f64,         // 1.0 = unity
    pub flags: u8,                // 0x40 = NO_RESTART_DECODER (gapless)
    pub transition_type: u8,      // 0=none
    pub transition_period: u8,    // seconds
    pub threshold_kb: u8,         // buffer threshold in KB
    pub output_threshold_ds: u8,  // output buffer threshold in tenths of second
}

/// Encode `strm` command → complete server frame.
pub fn encode_strm_start(p: &StrmStartParams) -> Vec<u8> {
    // 24 fixed bytes + variable http_request
    let req_bytes = p.http_request.as_bytes();
    let mut data = Vec::with_capacity(24 + req_bytes.len());

    data.push(b's');                          // command
    data.push(b'1');                          // autostart = Auto
    data.push(p.format.as_byte());            // format
    data.push(b'?');                          // pcmsamplesize = self-describing
    data.push(b'?');                          // pcmsamplerate = self-describing
    data.push(b'?');                          // pcmchannels = self-describing
    data.push(b'?');                          // pcmendian = self-describing
    data.push(p.threshold_kb);                // threshold (KB)
    data.push(0x00);                          // spdif_enable = auto
    data.push(p.transition_period);           // transition period
    data.push(p.transition_type);             // transition type
    data.push(p.flags);                       // flags
    data.push(p.output_threshold_ds);         // output threshold
    data.push(0x00);                          // reserved

    // replay_gain as 16.16 fixed-point
    let rg_fixed = (p.replay_gain * 65536.0) as u32;
    data.extend_from_slice(&rg_fixed.to_be_bytes());

    data.extend_from_slice(&p.server_port.to_be_bytes());

    // server IP as 4 bytes big-endian
    data.extend_from_slice(&p.server_ip);

    // HTTP request string appended
    data.extend_from_slice(req_bytes);

    server_frame(b"strm", &data)
}

/// Encode simple strm sub-commands (pause/unpause/stop/flush/status).
pub fn encode_strm_simple(cmd: StrmCommand, timestamp: u32) -> Vec<u8> {
    let mut data = vec![0u8; 24];
    data[0] = cmd.as_byte();
    // For 'p', 'u', 'a', 't': replay_gain field at offset 14 carries timestamp
    let ts_bytes = timestamp.to_be_bytes();
    data[14] = ts_bytes[0];
    data[15] = ts_bytes[1];
    data[16] = ts_bytes[2];
    data[17] = ts_bytes[3];
    server_frame(b"strm", &data)
}

/// Encode `audg` (volume) → server frame.
/// Volume is 0.0..1.0 applied to both channels.
pub fn encode_audg(left: f64, right: f64) -> Vec<u8> {
    let mut data = vec![0u8; 18];
    // bytes 0..9 = old gain + dvc + preamp (all zeros)
    // bytes 10..13 = new_left as 16.16 fixed-point
    let l_fixed = (left * 65536.0) as u32;
    let r_fixed = (right * 65536.0) as u32;
    data[10..14].copy_from_slice(&l_fixed.to_be_bytes());
    data[14..18].copy_from_slice(&r_fixed.to_be_bytes());
    server_frame(b"audg", &data)
}

/// Encode `aude` (enable DAC/SPDIF) → server frame.
pub fn encode_aude(spdif: bool, dac: bool) -> Vec<u8> {
    let data = [spdif as u8, dac as u8];
    server_frame(b"aude", &data)
}

/// Encode `vers` (server version string) → server frame.
pub fn encode_vers(version: &str) -> Vec<u8> {
    server_frame(b"vers", version.as_bytes())
}

/// Encode `setd` (query player name, id=0) → server frame.
pub fn encode_setd_query_name() -> Vec<u8> {
    server_frame(b"setd", &[0x00])
}

// ── Client → Server messages ────────────────────────────────────────────────

/// MAC address as 6 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacAddress(pub [u8; 6]);

impl MacAddress {
    pub fn to_string(&self) -> String {
        format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl fmt::Display for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5]
        )
    }
}

impl fmt::Debug for MacAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Parsed HELO message from a player.
#[derive(Debug, Clone)]
pub struct HeloMessage {
    pub device_id: u8,
    pub revision: u8,
    pub mac: MacAddress,
    pub capabilities: String,
}

/// STAT event codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatEvent {
    /// STMt — heartbeat/timer
    Timer,
    /// STMc — connected to streaming endpoint
    Connected,
    /// STMs — track started playing
    TrackStarted,
    /// STMd — decoder ready (input consumed)
    DecoderReady,
    /// STMu — output underrun (track finished normally)
    Underrun,
    /// STMp — paused
    Paused,
    /// STMr — resumed
    Resumed,
    /// STMf — flushed
    Flushed,
    /// STMl — buffer threshold reached
    BufferThreshold,
    /// STMn — format not supported
    NotSupported,
    /// STMo — output buffer underrun (rebuffer)
    OutputUnderrun,
    /// Unknown
    Unknown([u8; 4]),
}

impl StatEvent {
    fn from_bytes(b: &[u8; 4]) -> Self {
        match b {
            b"STMt" => Self::Timer,
            b"STMc" => Self::Connected,
            b"STMs" => Self::TrackStarted,
            b"STMd" => Self::DecoderReady,
            b"STMu" => Self::Underrun,
            b"STMp" => Self::Paused,
            b"STMr" => Self::Resumed,
            b"STMf" => Self::Flushed,
            b"STMl" => Self::BufferThreshold,
            b"STMn" => Self::NotSupported,
            b"STMo" => Self::OutputUnderrun,
            _ => Self::Unknown(*b),
        }
    }
}

/// Parsed STAT message from a player.
#[derive(Debug, Clone)]
pub struct StatMessage {
    pub event: StatEvent,
    pub buffer_size: u32,
    pub buffer_fullness: u32,
    pub bytes_received: u64,
    pub signal_strength: u16,
    pub jiffies: u32,
    pub output_buffer_size: u32,
    pub output_buffer_fullness: u32,
    pub elapsed_seconds: u32,
    pub elapsed_milliseconds: u32,
    pub timestamp: u32,
}

/// All possible client messages.
#[derive(Debug, Clone)]
pub enum ClientMessage {
    Helo(HeloMessage),
    Stat(StatMessage),
    Bye(u8),
    Dsco(u8),
    Resp(Vec<u8>),
    Meta(Vec<u8>),
    Setd(Vec<u8>),
    Butn(ButtonMessage),
    Unknown(String, Vec<u8>),
}

/// Parsed BUTN message from a player (button press).
#[derive(Debug, Clone)]
pub struct ButtonMessage {
    pub timestamp: u32,
    pub button_code: u32,
}

/// Parse one complete client→server message from `[tag(4)][len(4)][payload(len)]`.
/// Caller must have already read the full frame.
pub fn parse_client_message(tag: &[u8; 4], payload: &[u8]) -> ClientMessage {
    match tag {
        b"HELO" => parse_helo(payload),
        b"STAT" => parse_stat(payload),
        b"BYE!" => {
            let val = payload.first().copied().unwrap_or(0);
            ClientMessage::Bye(val)
        }
        b"DSCO" => {
            let val = payload.first().copied().unwrap_or(0);
            ClientMessage::Dsco(val)
        }
        b"RESP" => ClientMessage::Resp(payload.to_vec()),
        b"META" => ClientMessage::Meta(payload.to_vec()),
        b"SETD" => ClientMessage::Setd(payload.to_vec()),
        b"BUTN" => parse_butn(payload),
        b"IR  " => parse_butn(payload),  // IR uses same format
        _ => {
            let tag_str = String::from_utf8_lossy(tag).to_string();
            ClientMessage::Unknown(tag_str, payload.to_vec())
        }
    }
}

fn parse_helo(data: &[u8]) -> ClientMessage {
    if data.len() < 8 {
        return ClientMessage::Unknown("HELO".into(), data.to_vec());
    }
    let device_id = data[0];
    let revision = data[1];
    let mut mac = [0u8; 6];
    mac.copy_from_slice(&data[2..8]);

    // Capabilities string starts at byte 36 (after UUID + wlan + bytes_received + language)
    let capabilities = if data.len() > 36 {
        String::from_utf8_lossy(&data[36..]).to_string()
    } else {
        String::new()
    };

    ClientMessage::Helo(HeloMessage {
        device_id,
        revision,
        mac: MacAddress(mac),
        capabilities,
    })
}

fn read_u16_be(data: &[u8], offset: usize) -> u16 {
    if offset + 2 > data.len() {
        return 0;
    }
    u16::from_be_bytes([data[offset], data[offset + 1]])
}

fn read_u32_be(data: &[u8], offset: usize) -> u32 {
    if offset + 4 > data.len() {
        return 0;
    }
    u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]])
}

fn read_u64_be(data: &[u8], offset: usize) -> u64 {
    if offset + 8 > data.len() {
        return 0;
    }
    u64::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ])
}

fn parse_stat(data: &[u8]) -> ClientMessage {
    // Minimum STAT payload is ~47 bytes
    if data.len() < 4 {
        return ClientMessage::Unknown("STAT".into(), data.to_vec());
    }

    let mut event_code = [0u8; 4];
    event_code.copy_from_slice(&data[0..4]);

    ClientMessage::Stat(StatMessage {
        event: StatEvent::from_bytes(&event_code),
        // bytes 4..6 = crlf, mas_init, mas_mode (skip)
        buffer_size: read_u32_be(data, 7),
        buffer_fullness: read_u32_be(data, 11),
        bytes_received: read_u64_be(data, 15),
        signal_strength: read_u16_be(data, 23),
        jiffies: read_u32_be(data, 25),
        output_buffer_size: read_u32_be(data, 29),
        output_buffer_fullness: read_u32_be(data, 33),
        elapsed_seconds: read_u32_be(data, 37),
        // byte 41-42 = voltage (skip)
        elapsed_milliseconds: read_u32_be(data, 43),
        timestamp: read_u32_be(data, 47),
    })
}

fn parse_butn(data: &[u8]) -> ClientMessage {
    // IR frame format from Squeezer/SlimProto:
    //   Offset 0-3: timestamp (u32 BE)
    //   Offset 4:   format (u8) — 0xFF = NEC, 0x02 = JVC
    //   Offset 5:   number of bits (u8)
    //   Offset 6-9: IR code (u32 BE)
    //
    // BUTN frame format:
    //   Offset 0-3: timestamp (u32 BE)
    //   Offset 4-7: button code (u32 BE)
    let timestamp = read_u32_be(data, 0);
    let button_code = if data.len() >= 10 {
        // IR frame: code is at offset 6
        read_u32_be(data, 6)
    } else {
        // Short BUTN frame: code at offset 4
        read_u32_be(data, 4)
    };
    ClientMessage::Butn(ButtonMessage {
        timestamp,
        button_code,
    })
}
