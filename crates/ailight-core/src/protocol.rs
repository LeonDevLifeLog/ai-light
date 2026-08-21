//! V0.4 蓝牙通信协议编解码层（纯逻辑，无 IO）
//!
//! 依据：《AgentCore-Light 蓝牙通信协议规范 V0.4》
//! - 帧格式 §3：55 AA | ver | seq(u16 BE) | cmd | len(u16 BE) | data | checksum
//! - 命令表 §16；SCENE 结构 §7/§8；能力结构 §6
//! - 本层只做字节级编解码与帧级校验（版本/长度/校验和），语义校验在 theme 层

use std::fmt;

/// 线协议版本字节
pub const PROTOCOL_VERSION: u8 = 0x04;
/// 帧头
pub const FRAME_HEADER: [u8; 2] = [0x55, 0xAA];
/// 协议数据区上限
pub const MAX_DATA_LEN: u16 = 235;
/// 帧最小长度（帧头+版本+seq+cmd+len+checksum）
pub const FRAME_MIN_LEN: usize = 9;
/// PC 应答超时建议（ms）
pub const RESPONSE_TIMEOUT_MS: u64 = 500;
/// 最大重发次数（协议 §3.5）
pub const MAX_RETRIES: u8 = 2;

// ---- 命令字（§16） ----
pub const CMD_PING: u8 = 0x01;
pub const CMD_GET_DEVICE_INFO: u8 = 0x02;
pub const CMD_GET_RUNTIME_STATUS: u8 = 0x03;
pub const CMD_GET_CAPABILITIES: u8 = 0x04;
pub const CMD_RESET_OUTPUTS: u8 = 0x05;
pub const CMD_SET_SCENE: u8 = 0x20;
pub const CMD_GET_OUTPUT_STATUS: u8 = 0x21;
pub const CMD_LED_STREAM_FRAME: u8 = 0x22;
pub const CMD_GET_POWER_STATUS: u8 = 0x50;
pub const CMD_POWER_OFF: u8 = 0x51;

// ---- 设备主动事件（§11） ----
pub const EVT_DEVICE_READY: u8 = 0xE0;
pub const EVT_POWER_CHANGED: u8 = 0xE2;
pub const EVT_BUTTON_EVENT: u8 = 0xE3;
pub const EVT_FAULT_EVENT: u8 = 0xEF;

/// 应答命令字 = 请求 | 0x80
pub fn response_cmd(cmd: u8) -> u8 {
    cmd | 0x80
}

// ---- 曲线枚举（§7.2） ----
pub const CURVE_CONSTANT: u8 = 0x00;
pub const CURVE_SQUARE: u8 = 0x01;
pub const CURVE_TRIANGLE: u8 = 0x02;
pub const CURVE_SAW_UP: u8 = 0x03;
pub const CURVE_SAW_DOWN: u8 = 0x04;
/// SINE 预留不实现（§7.2 D-012）
pub const CURVE_SINE_RESERVED: u8 = 0x05;

/// 相位归一化：角度 → 0~65535（360° 回绕为 0）
pub fn phase_from_deg(deg: u16) -> u16 {
    (((deg as u32 * 65536) / 360) % 65536) as u16
}

// ---- 结果码（§3.6） ----
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultCode {
    Ok,
    InvalidLength,
    InvalidParameter,
    UnsupportedCommand,
    Busy,
    InvalidState,
    VersionMismatch,
    NotReady,
    LowBattery,
    InternalError,
    NotSupported,
    Unknown(u8),
}

impl ResultCode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x00 => ResultCode::Ok,
            0x01 => ResultCode::InvalidLength,
            0x02 => ResultCode::InvalidParameter,
            0x03 => ResultCode::UnsupportedCommand,
            0x04 => ResultCode::Busy,
            0x05 => ResultCode::InvalidState,
            0x06 => ResultCode::VersionMismatch,
            0x07 => ResultCode::NotReady,
            0x09 => ResultCode::LowBattery,
            0x0A => ResultCode::InternalError,
            0x0B => ResultCode::NotSupported,
            other => ResultCode::Unknown(other),
        }
    }
    pub fn as_u8(&self) -> u8 {
        match self {
            ResultCode::Ok => 0x00,
            ResultCode::InvalidLength => 0x01,
            ResultCode::InvalidParameter => 0x02,
            ResultCode::UnsupportedCommand => 0x03,
            ResultCode::Busy => 0x04,
            ResultCode::InvalidState => 0x05,
            ResultCode::VersionMismatch => 0x06,
            ResultCode::NotReady => 0x07,
            ResultCode::LowBattery => 0x09,
            ResultCode::InternalError => 0x0A,
            ResultCode::NotSupported => 0x0B,
            ResultCode::Unknown(v) => *v,
        }
    }
}

impl fmt::Display for ResultCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ResultCode::Ok => "OK",
            ResultCode::InvalidLength => "INVALID_LENGTH",
            ResultCode::InvalidParameter => "INVALID_PARAMETER",
            ResultCode::UnsupportedCommand => "UNSUPPORTED_COMMAND",
            ResultCode::Busy => "BUSY",
            ResultCode::InvalidState => "INVALID_STATE",
            ResultCode::VersionMismatch => "VERSION_MISMATCH",
            ResultCode::NotReady => "NOT_READY",
            ResultCode::LowBattery => "LOW_BATTERY",
            ResultCode::InternalError => "INTERNAL_ERROR",
            ResultCode::NotSupported => "NOT_SUPPORTED",
            ResultCode::Unknown(v) => return write!(f, "UNKNOWN(0x{v:02X})"),
        };
        write!(f, "{name}")
    }
}

// ---- 帧 ----
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub seq: u16,
    pub cmd: u8,
    pub data: Vec<u8>,
}

/// 校验和：帧头至数据区末尾逐字节求和，对 256 取余（§3.2）
pub fn calc_checksum(bytes: &[u8]) -> u8 {
    let sum: u32 = bytes.iter().map(|&b| b as u32).sum();
    (sum & 0xFF) as u8
}

/// 构造一帧（含校验和）
pub fn build_frame(cmd: u8, seq: u16, data: &[u8]) -> Vec<u8> {
    assert!(data.len() as u16 <= MAX_DATA_LEN, "data too long");
    let mut frame = Vec::with_capacity(FRAME_MIN_LEN + data.len());
    frame.extend_from_slice(&FRAME_HEADER);
    frame.push(PROTOCOL_VERSION);
    frame.extend_from_slice(&seq.to_be_bytes());
    frame.push(cmd);
    frame.extend_from_slice(&(data.len() as u16).to_be_bytes());
    frame.extend_from_slice(data);
    let checksum = calc_checksum(&frame);
    frame.push(checksum);
    frame
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// 数据不足一帧，等待更多数据
    NeedMore,
    /// 协议版本不匹配
    VersionMismatch(u8),
    /// 数据长度超限
    LengthExceeded(u16),
    /// 校验和失败
    ChecksumMismatch { expect: u8, actual: u8 },
}

/// 从缓冲起始解析一帧；成功返回 (帧, 消耗字节数)
pub fn parse_frame(buf: &[u8]) -> Result<(Frame, usize), ParseError> {
    if buf.len() < FRAME_MIN_LEN {
        return Err(ParseError::NeedMore);
    }
    if buf[0] != FRAME_HEADER[0] || buf[1] != FRAME_HEADER[1] {
        // 帧头不匹配由 FrameParser 负责跳过
        return Err(ParseError::NeedMore);
    }
    if buf[2] != PROTOCOL_VERSION {
        return Err(ParseError::VersionMismatch(buf[2]));
    }
    let len = u16::from_be_bytes([buf[6], buf[7]]);
    if len > MAX_DATA_LEN {
        return Err(ParseError::LengthExceeded(len));
    }
    let total = FRAME_MIN_LEN + len as usize;
    if buf.len() < total {
        return Err(ParseError::NeedMore);
    }
    let expect = buf[total - 1];
    let actual = calc_checksum(&buf[..total - 1]);
    if expect != actual {
        return Err(ParseError::ChecksumMismatch { expect, actual });
    }
    Ok((
        Frame {
            seq: u16::from_be_bytes([buf[3], buf[4]]),
            cmd: buf[5],
            data: buf[FRAME_MIN_LEN - 1..total - 1].to_vec(),
        },
        total,
    ))
}

/// 流式组帧器（§4 接收流程：跨块缓存、粘包、坏帧跳过）
#[derive(Debug, Default)]
pub struct FrameParser {
    buf: Vec<u8>,
}

impl FrameParser {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(512),
        }
    }

    pub fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
    }

    /// 尝试取出一帧；无完整帧返回 None
    pub fn next_frame(&mut self) -> Option<Frame> {
        loop {
            // 1. 搜索帧头
            if self.buf.len() < 2 {
                return None;
            }
            if self.buf[0] != FRAME_HEADER[0] || self.buf[1] != FRAME_HEADER[1] {
                self.buf.remove(0);
                continue;
            }
            // 2. 头部不足
            if self.buf.len() < FRAME_MIN_LEN - 1 {
                return None;
            }
            // 3. 版本校验
            if self.buf[2] != PROTOCOL_VERSION {
                self.buf.remove(0);
                continue;
            }
            // 4. 长度校验
            let len = u16::from_be_bytes([self.buf[6], self.buf[7]]);
            if len > MAX_DATA_LEN {
                self.buf.remove(0);
                continue;
            }
            let total = FRAME_MIN_LEN + len as usize;
            if self.buf.len() < total {
                return None;
            }
            // 5. 校验和
            let expect = self.buf[total - 1];
            let actual = calc_checksum(&self.buf[..total - 1]);
            if expect != actual {
                self.buf.remove(0);
                continue;
            }
            // 6. 取出
            let frame = Frame {
                seq: u16::from_be_bytes([self.buf[3], self.buf[4]]),
                cmd: self.buf[5],
                data: self.buf[FRAME_MIN_LEN - 1..total - 1].to_vec(),
            };
            self.buf.drain(..total);
            return Some(frame);
        }
    }
}

// ---- SCENE 数据模型（§7/§8） ----
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn from_hex(hex: &str) -> Option<Rgb> {
        let h = hex.trim_start_matches('#');
        if h.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&h[0..2], 16).ok()?;
        let g = u8::from_str_radix(&h[2..4], 16).ok()?;
        let b = u8::from_str_radix(&h[4..6], 16).ok()?;
        Some(Rgb(r, g, b))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedTrack {
    pub curve: u8,
    pub low: Rgb,
    pub high: Rgb,
    pub brightness: u8,
    pub period_ms: u16,
    pub phase: u16,
    pub duty_percent: u8,
    pub repeat_count: u16,
    pub end_level: u8,
}

/// end_level 取值（§7.4）
pub const END_OFF: u8 = 0x00;
pub const END_LOW: u8 = 0x01;
pub const END_HIGH: u8 = 0x02;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuzzerSegment {
    pub frequency_hz: u16,
    pub duration_ms: u16,
    pub volume: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuzzerTrack {
    pub start_delay_ms: u16,
    pub repeat_count: u16,
    pub segments: Vec<BuzzerSegment>,
}

/// apply_mode（§8.4）
pub const APPLY_IF_CHANGED: u8 = 0x00;
pub const RESTART_SCENE: u8 = 0x01;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputScene {
    pub apply_mode: u8,
    pub transition_ms: u16,
    pub leds: [LedTrack; 3],
    pub buzzer: BuzzerTrack,
}

impl Default for LedTrack {
    fn default() -> Self {
        Self {
            curve: CURVE_CONSTANT,
            low: Rgb(0, 0, 0),
            high: Rgb(0, 0, 0),
            brightness: 0,
            period_ms: 0,
            phase: 0,
            duty_percent: 0,
            repeat_count: 0,
            end_level: END_OFF,
        }
    }
}

impl OutputScene {
    /// 全灭场景（三灯黑、静音、立即切换）
    pub fn none() -> Self {
        Self {
            apply_mode: APPLY_IF_CHANGED,
            transition_ms: 0,
            leds: [LedTrack::default(); 3],
            buzzer: BuzzerTrack {
                start_delay_ms: 0,
                repeat_count: 0,
                segments: vec![],
            },
        }
    }

    /// 编码为 SET_SCENE 数据区（59 + 5×N 字节，§8.1）
    pub fn encode_data(&self) -> Vec<u8> {
        let mut d = Vec::with_capacity(59 + 5 * self.buzzer.segments.len());
        d.push(1); // format_version
        d.push(0x01); // scene_kind = TRACKS
        d.push(self.apply_mode);
        d.extend_from_slice(&self.transition_ms.to_be_bytes());
        d.push(0); // reserved
        for led in &self.leds {
            d.push(led.curve);
            d.extend_from_slice(&[led.low.0, led.low.1, led.low.2]);
            d.extend_from_slice(&[led.high.0, led.high.1, led.high.2]);
            d.push(led.brightness);
            d.extend_from_slice(&led.period_ms.to_be_bytes());
            d.extend_from_slice(&led.phase.to_be_bytes());
            d.push(led.duty_percent);
            d.extend_from_slice(&led.repeat_count.to_be_bytes());
            d.push(led.end_level);
        }
        d.extend_from_slice(&self.buzzer.start_delay_ms.to_be_bytes());
        d.extend_from_slice(&self.buzzer.repeat_count.to_be_bytes());
        d.push(self.buzzer.segments.len() as u8);
        for seg in &self.buzzer.segments {
            d.extend_from_slice(&seg.frequency_hz.to_be_bytes());
            d.extend_from_slice(&seg.duration_ms.to_be_bytes());
            d.push(seg.volume);
        }
        d
    }

    /// 从 SET_SCENE 数据区解码
    pub fn decode_data(data: &[u8]) -> Option<Self> {
        let base = 59usize;
        if data.len() < base {
            return None;
        }
        if data[0] != 1 || data[1] != 0x01 {
            return None;
        }
        let n = data[58] as usize; // buzzer segment_count（偏移 58）
                                   // 实际布局：6 头 + 48 LED + 5 buzzer 头 = 59；segments 在 59..59+5n
        if data.len() != base + 5 * n {
            return None;
        }
        let apply_mode = data[2];
        let transition_ms = u16::from_be_bytes([data[3], data[4]]);
        let mut leds = [LedTrack::default(); 3];
        for (i, led) in leds.iter_mut().enumerate() {
            let o = 6 + i * 16;
            led.curve = data[o];
            led.low = Rgb(data[o + 1], data[o + 2], data[o + 3]);
            led.high = Rgb(data[o + 4], data[o + 5], data[o + 6]);
            led.brightness = data[o + 7];
            led.period_ms = u16::from_be_bytes([data[o + 8], data[o + 9]]);
            led.phase = u16::from_be_bytes([data[o + 10], data[o + 11]]);
            led.duty_percent = data[o + 12];
            led.repeat_count = u16::from_be_bytes([data[o + 13], data[o + 14]]);
            led.end_level = data[o + 15];
        }
        let start_delay_ms = u16::from_be_bytes([data[54], data[55]]);
        let repeat_count = u16::from_be_bytes([data[56], data[57]]);
        let mut segments = Vec::with_capacity(n);
        for i in 0..n {
            let o = 59 + i * 5;
            segments.push(BuzzerSegment {
                frequency_hz: u16::from_be_bytes([data[o], data[o + 1]]),
                duration_ms: u16::from_be_bytes([data[o + 2], data[o + 3]]),
                volume: data[o + 4],
            });
        }
        Some(Self {
            apply_mode,
            transition_ms,
            leds,
            buzzer: BuzzerTrack {
                start_delay_ms,
                repeat_count,
                segments,
            },
        })
    }
}

// ---- 请求构造 ----
pub fn ping(seq: u16) -> Vec<u8> {
    build_frame(CMD_PING, seq, &[])
}
pub fn get_device_info(seq: u16) -> Vec<u8> {
    build_frame(CMD_GET_DEVICE_INFO, seq, &[])
}
pub fn get_runtime_status(seq: u16) -> Vec<u8> {
    build_frame(CMD_GET_RUNTIME_STATUS, seq, &[])
}
pub fn get_capabilities(seq: u16) -> Vec<u8> {
    build_frame(CMD_GET_CAPABILITIES, seq, &[])
}
pub fn reset_outputs(seq: u16) -> Vec<u8> {
    build_frame(CMD_RESET_OUTPUTS, seq, &[])
}
pub fn get_output_status(seq: u16) -> Vec<u8> {
    build_frame(CMD_GET_OUTPUT_STATUS, seq, &[])
}
pub fn get_power_status(seq: u16) -> Vec<u8> {
    build_frame(CMD_GET_POWER_STATUS, seq, &[])
}
pub fn power_off(seq: u16) -> Vec<u8> {
    build_frame(CMD_POWER_OFF, seq, &[])
}
pub fn set_scene(seq: u16, scene: &OutputScene) -> Vec<u8> {
    build_frame(CMD_SET_SCENE, seq, &scene.encode_data())
}
pub fn led_stream_frame(seq: u16, brightness: u8, transition_ms: u16, pixels: [Rgb; 3]) -> Vec<u8> {
    let mut d = Vec::with_capacity(13);
    d.push(brightness);
    d.extend_from_slice(&transition_ms.to_be_bytes());
    for p in pixels {
        d.extend_from_slice(&[p.0, p.1, p.2]);
    }
    d.push(0); // reserved
    build_frame(CMD_LED_STREAM_FRAME, seq, &d)
}

// ---- 应答解析 ----
pub fn parse_result(data: &[u8]) -> ResultCode {
    if data.is_empty() {
        return ResultCode::InvalidLength;
    }
    ResultCode::from_u8(data[0])
}

#[derive(Debug, Clone, PartialEq)]
pub struct PingResponse {
    pub uptime_s: u32,
}
pub fn parse_ping_response(data: &[u8]) -> Result<(ResultCode, PingResponse), ParseError> {
    if data.len() < 5 {
        return Err(ParseError::NeedMore);
    }
    let uptime_s = u32::from_be_bytes([data[1], data[2], data[3], data[4]]);
    Ok((parse_result(data), PingResponse { uptime_s }))
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub protocol_min: u8,
    pub protocol_max: u8,
    pub fw: (u8, u8, u8),
    pub hardware_revision: u8,
    pub hardware_variant: u8,
    pub product_id: u16,
    pub device_id: [u8; 6],
}
pub fn parse_device_info_response(data: &[u8]) -> Result<(ResultCode, DeviceInfo), ParseError> {
    // 应答 = result(1) + 15 字节 = 16 字节（§12.2）
    if data.len() < 16 {
        return Err(ParseError::NeedMore);
    }
    let mut device_id = [0u8; 6];
    device_id.copy_from_slice(&data[10..16]);
    Ok((
        parse_result(data),
        DeviceInfo {
            protocol_min: data[1],
            protocol_max: data[2],
            fw: (data[3], data[4], data[5]),
            hardware_revision: data[6],
            hardware_variant: data[7],
            product_id: u16::from_be_bytes([data[8], data[9]]),
            device_id,
        },
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Capabilities {
    pub schema_version: u8,
    pub capability_bits: u32,
    pub led_count: u8,
    pub supported_curves: u16,
    pub min_period_ms: u16,
    pub max_period_ms: u16,
    pub max_transition_ms: u16,
    pub max_buzzer_segments: u8,
    pub min_frequency_hz: u16,
    pub max_frequency_hz: u16,
    pub max_volume: u8,
}
/// 能力位（§6.3）
pub const CAP_RGB_LED: u32 = 1 << 0;
pub const CAP_PASSIVE_BUZZER: u32 = 1 << 1;
pub const CAP_LED_TRACKS: u32 = 1 << 2;
pub const CAP_LED_STREAM: u32 = 1 << 3;
pub const CAP_BATTERY_PRESENT: u32 = 1 << 4;
pub const CAP_BATTERY_ADC: u32 = 1 << 5;
pub const CAP_CHARGE_STATUS: u32 = 1 << 6;
pub const CAP_EXTERNAL_POWER_DETECT: u32 = 1 << 7;
pub const CAP_SOFTWARE_POWER_OFF: u32 = 1 << 8;
pub const CAP_STANDARD_BAS: u32 = 1 << 9;
pub const CAP_BUTTON: u32 = 1 << 10;

pub fn parse_capabilities_response(data: &[u8]) -> Result<(ResultCode, Capabilities), ParseError> {
    if data.len() < 22 {
        return Err(ParseError::NeedMore);
    }
    Ok((
        parse_result(data),
        Capabilities {
            schema_version: data[1],
            capability_bits: u32::from_be_bytes([data[2], data[3], data[4], data[5]]),
            led_count: data[6],
            supported_curves: u16::from_be_bytes([data[7], data[8]]),
            min_period_ms: u16::from_be_bytes([data[9], data[10]]),
            max_period_ms: u16::from_be_bytes([data[11], data[12]]),
            max_transition_ms: u16::from_be_bytes([data[13], data[14]]),
            max_buzzer_segments: data[15],
            min_frequency_hz: u16::from_be_bytes([data[16], data[17]]),
            max_frequency_hz: u16::from_be_bytes([data[18], data[19]]),
            max_volume: data[20],
        },
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetSceneResponse {
    pub applied: bool,
    pub scene_digest: u16,
}
pub fn parse_set_scene_response(data: &[u8]) -> Result<(ResultCode, SetSceneResponse), ParseError> {
    // 帧示例（§17.8）实测为 3 字节 [applied, digest_hi, digest_low]（不含 result）；
    // §8.5 文档描述为 4 字节 [result, applied, digest]。兼容两种布局。
    if data.len() == 3 {
        Ok((
            ResultCode::Ok,
            SetSceneResponse {
                applied: data[0] == 1,
                scene_digest: u16::from_be_bytes([data[1], data[2]]),
            },
        ))
    } else if data.len() >= 4 {
        Ok((
            parse_result(data),
            SetSceneResponse {
                applied: data[1] == 1,
                scene_digest: u16::from_be_bytes([data[2], data[3]]),
            },
        ))
    } else {
        Err(ParseError::NeedMore)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PowerStatus {
    pub power_source: u8,
    pub power_flags: u16,
    pub battery_mv: u16,
    pub battery_percent: u8,
    pub charge_state: u8,
}
pub fn parse_power_status_response(data: &[u8]) -> Result<(ResultCode, PowerStatus), ParseError> {
    if data.len() < 8 {
        return Err(ParseError::NeedMore);
    }
    Ok((
        parse_result(data),
        PowerStatus {
            power_source: data[1],
            power_flags: u16::from_be_bytes([data[2], data[3]]),
            battery_mv: u16::from_be_bytes([data[4], data[5]]),
            battery_percent: data[6],
            charge_state: data[7],
        },
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutputStatus {
    pub scene_digest: u16,
    pub scene_state: u8,
    pub leds: [Rgb; 3],
    pub buzzer_state: u8,
    pub buzzer_frequency_hz: u16,
    pub buzzer_volume: u8,
    pub scene_uptime_ms: u32,
}
pub fn parse_output_status_response(data: &[u8]) -> Result<(ResultCode, OutputStatus), ParseError> {
    // 应答 = result(1) + 20 字节 = 21 字节（§12.5）
    if data.len() < 21 {
        return Err(ParseError::NeedMore);
    }
    Ok((
        parse_result(data),
        OutputStatus {
            scene_digest: u16::from_be_bytes([data[1], data[2]]),
            scene_state: data[3],
            leds: [
                Rgb(data[4], data[5], data[6]),
                Rgb(data[7], data[8], data[9]),
                Rgb(data[10], data[11], data[12]),
            ],
            buzzer_state: data[13],
            buzzer_frequency_hz: u16::from_be_bytes([data[14], data[15]]),
            buzzer_volume: data[16],
            scene_uptime_ms: u32::from_be_bytes([data[17], data[18], data[19], data[20]]),
        },
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeStatus {
    pub device_state: u8,
    pub runtime_flags: u16,
    pub active_scene_digest: u16,
    pub scene_uptime_ms: u32,
    pub fault_flags: u16,
}
pub fn parse_runtime_status_response(
    data: &[u8],
) -> Result<(ResultCode, RuntimeStatus), ParseError> {
    if data.len() < 11 {
        return Err(ParseError::NeedMore);
    }
    Ok((
        parse_result(data),
        RuntimeStatus {
            device_state: data[1],
            runtime_flags: u16::from_be_bytes([data[2], data[3]]),
            active_scene_digest: u16::from_be_bytes([data[4], data[5]]),
            scene_uptime_ms: u32::from_be_bytes([data[6], data[7], data[8], data[9]]),
            fault_flags: u16::from_be_bytes([data[10], data[11]]),
        },
    ))
}

// ---- 设备主动事件解析（§11） ----
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceReady {
    pub protocol_version: u8,
    pub fw: (u8, u8, u8),
    pub hardware_variant: u8,
    pub boot_reason: u8,
}
pub fn parse_device_ready(data: &[u8]) -> Option<DeviceReady> {
    if data.len() < 6 {
        return None;
    }
    Some(DeviceReady {
        protocol_version: data[0],
        fw: (data[1], data[2], data[3]),
        hardware_variant: data[4],
        boot_reason: data[5],
    })
}

pub fn parse_power_changed(data: &[u8]) -> Option<PowerStatus> {
    if data.len() < 7 {
        return None;
    }
    Some(PowerStatus {
        power_source: data[0],
        power_flags: u16::from_be_bytes([data[1], data[2]]),
        battery_mv: u16::from_be_bytes([data[3], data[4]]),
        battery_percent: data[5],
        charge_state: data[6],
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct ButtonEvent {
    pub event: u8,
    pub duration_ms: u16,
}
pub fn parse_button_event(data: &[u8]) -> Option<ButtonEvent> {
    if data.len() < 3 {
        return None;
    }
    Some(ButtonEvent {
        event: data[0],
        duration_ms: u16::from_be_bytes([data[1], data[2]]),
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct FaultEvent {
    pub source: u8,
    pub code: u8,
    pub context: u16,
}
pub fn parse_fault_event(data: &[u8]) -> Option<FaultEvent> {
    if data.len() < 4 {
        return None;
    }
    Some(FaultEvent {
        source: data[0],
        code: data[1],
        context: u16::from_be_bytes([data[2], data[3]]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        s.split_whitespace()
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect()
    }

    /// 黄金测试：协议 §17 帧示例（全部经脚本生成并验证）
    mod golden {
        use super::*;

        #[test]
        fn g17_1_none_scene() {
            // 全灭场景：三灯 CONSTANT 黑、蜂鸣静音、立即切换
            let scene = OutputScene::none();
            let frame = set_scene(1, &scene);
            assert_eq!(
                frame,
                hex(
                    "55 AA 04 00 01 20 00 3B 01 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00
                     00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
                     00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 61"
                )
            );
        }

        #[test]
        fn g17_2_processing() {
            // 处理中：中间灯 CONSTANT 黄 FF B4 00 亮度 50
            let mut led = LedTrack::default();
            led.curve = CURVE_CONSTANT;
            led.high = Rgb(0xFF, 0xB4, 0x00);
            led.brightness = 50;
            let scene = OutputScene {
                apply_mode: APPLY_IF_CHANGED,
                transition_ms: 0,
                leds: [LedTrack::default(), led, LedTrack::default()],
                buzzer: BuzzerTrack {
                    start_delay_ms: 0,
                    repeat_count: 0,
                    segments: vec![],
                },
            };
            let frame = set_scene(2, &scene);
            // 帧布局：8 帧头 + 6 场景头 + LED0(16B) + LED1(16B, 含 FF B4 00 32) + LED2(16B) + buzzer(5B) + 校验和 47
            assert_eq!(
                frame,
                hex("55 AA 04 00 02 20 00 3B 01 01 00 00 00 00
                     00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
                     00 00 00 00 FF B4 00 32 00 00 00 00 00 00 00 00
                     00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
                     00 00 00 00 00 47")
            );
        }

        #[test]
        fn g17_3_error() {
            // 错误：顶灯 SQUARE 红、400ms、duty 50、重复 5 次后灭；蜂鸣 2000Hz 150ms/静音 150ms 重复 3 次
            let led = LedTrack {
                curve: CURVE_SQUARE,
                low: Rgb(0, 0, 0),
                high: Rgb(0xFF, 0, 0),
                brightness: 60,
                period_ms: 400,
                phase: 0,
                duty_percent: 50,
                repeat_count: 5,
                end_level: END_OFF,
            };
            let scene = OutputScene {
                apply_mode: APPLY_IF_CHANGED,
                transition_ms: 0,
                leds: [led, LedTrack::default(), LedTrack::default()],
                buzzer: BuzzerTrack {
                    start_delay_ms: 0,
                    repeat_count: 3,
                    segments: vec![
                        BuzzerSegment {
                            frequency_hz: 2000,
                            duration_ms: 150,
                            volume: 50,
                        },
                        BuzzerSegment {
                            frequency_hz: 0,
                            duration_ms: 150,
                            volume: 0,
                        },
                    ],
                },
            };
            let frame = set_scene(3, &scene);
            assert_eq!(
                frame,
                hex(
                    "55 AA 04 00 03 20 00 45 01 01 00 00 00 00 01 00 00 00 FF 00 00 3C 01 90
                     00 00 32 00 05 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
                     00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 03 02 07 D0 00 96 32
                     00 00 00 96 00 AB"
                )
            );
        }

        #[test]
        fn g17_4_breath_marquee() {
            // 呼吸跑马：三灯 TRIANGLE 绿、1200ms、相位 0/120/240、持续
            let mk = |phase: u16| LedTrack {
                curve: CURVE_TRIANGLE,
                low: Rgb(0, 0, 0),
                high: Rgb(0, 0xFF, 0),
                brightness: 50,
                period_ms: 1200,
                phase,
                duty_percent: 0,
                repeat_count: 0,
                end_level: END_OFF,
            };
            let scene = OutputScene {
                apply_mode: APPLY_IF_CHANGED,
                transition_ms: 0,
                leds: [mk(0x0000), mk(0x5555), mk(0xAAAA)],
                buzzer: BuzzerTrack {
                    start_delay_ms: 0,
                    repeat_count: 0,
                    segments: vec![],
                },
            };
            let frame = set_scene(4, &scene);
            assert_eq!(
                frame,
                hex(
                    "55 AA 04 00 04 20 00 3B 01 01 00 00 00 00 02 00 00 00 00 FF 00 32 04 B0
                     00 00 00 00 00 00 02 00 00 00 00 FF 00 32 04 B0 55 55 00 00 00 00 02 00
                     00 00 00 FF 00 32 04 B0 AA AA 00 00 00 00 00 00 00 00 00 17"
                )
            );
        }

        #[test]
        fn g17_5_reset_outputs() {
            assert_eq!(reset_outputs(1), hex("55 AA 04 00 01 05 00 00 09"));
        }

        #[test]
        fn g17_6_ping() {
            assert_eq!(ping(1), hex("55 AA 04 00 01 01 00 00 05"));
            // 应答：result OK + uptime 3600s
            let mut p = FrameParser::new();
            p.push(&hex("55 AA 04 00 01 81 00 05 00 00 00 0E 10 A8"));
            let f = p.next_frame().unwrap();
            assert_eq!(f.cmd, response_cmd(CMD_PING));
            let (rc, resp) = parse_ping_response(&f.data).unwrap();
            assert_eq!(rc, ResultCode::Ok);
            assert_eq!(resp.uptime_s, 3600);
        }

        #[test]
        fn g17_7_capabilities() {
            assert_eq!(get_capabilities(1), hex("55 AA 04 00 01 04 00 00 08"));
            let mut p = FrameParser::new();
            p.push(&hex(
                "55 AA 04 00 01 84 00 17 00 01 00 00 07 FF 03 00 1F 00 C8 13 88 09 C4 10
                         00 64 27 10 64 00 00 07",
            ));
            let f = p.next_frame().unwrap();
            let (rc, cap) = parse_capabilities_response(&f.data).unwrap();
            assert_eq!(rc, ResultCode::Ok);
            assert_eq!(cap.capability_bits, 0x0000_07FF);
            assert_eq!(cap.supported_curves, 0x001F);
            assert_eq!(cap.min_period_ms, 200);
            assert_eq!(cap.max_period_ms, 5000);
            assert_eq!(cap.max_transition_ms, 2500);
            assert_eq!(cap.max_buzzer_segments, 16);
            assert_eq!(cap.min_frequency_hz, 100);
            assert_eq!(cap.max_frequency_hz, 10_000);
            assert_eq!(cap.max_volume, 100);
        }

        #[test]
        fn g17_8_set_scene_response() {
            let mut p = FrameParser::new();
            p.push(&hex("55 AA 04 00 02 A0 00 03 01 01 E7 91"));
            let f = p.next_frame().unwrap();
            let (rc, resp) = parse_set_scene_response(&f.data).unwrap();
            assert_eq!(rc, ResultCode::Ok);
            assert!(resp.applied);
            assert_eq!(resp.scene_digest, 0x01E7);

            p.push(&hex("55 AA 04 00 02 A0 00 03 00 01 E7 90"));
            let f = p.next_frame().unwrap();
            let (_, resp) = parse_set_scene_response(&f.data).unwrap();
            assert!(!resp.applied);
        }

        #[test]
        fn g17_9_device_info() {
            let mut p = FrameParser::new();
            p.push(&hex(
                "55 AA 04 00 01 82 00 10 00 04 04 01 00 00 01 01 00 01 AA BB CC DD EE FF 9D",
            ));
            let f = p.next_frame().unwrap();
            let (rc, info) = parse_device_info_response(&f.data).unwrap();
            assert_eq!(rc, ResultCode::Ok);
            assert_eq!((info.protocol_min, info.protocol_max), (4, 4));
            assert_eq!(info.fw, (1, 0, 0));
            assert_eq!(info.hardware_variant, 1); // BATTERY
            assert_eq!(info.product_id, 1);
            assert_eq!(info.device_id, [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF]);
        }

        #[test]
        fn g17_10_output_status() {
            let mut p = FrameParser::new();
            p.push(&hex(
                "55 AA 04 00 01 A1 00 15 00 07 B5 01 00 80 00 00 40 00 00 20 00 00 00
                         00 00 00 00 05 DC 38",
            ));
            let f = p.next_frame().unwrap();
            let (rc, st) = parse_output_status_response(&f.data).unwrap();
            assert_eq!(rc, ResultCode::Ok);
            assert_eq!(st.scene_digest, 0x07B5);
            assert_eq!(st.scene_state, 1); // RUNNING
            assert_eq!(st.leds, [Rgb(0, 0x80, 0), Rgb(0, 0x40, 0), Rgb(0, 0x20, 0)]);
            assert_eq!(st.buzzer_state, 0); // IDLE
            assert_eq!(st.buzzer_frequency_hz, 0); // 无蜂鸣
            assert_eq!(st.scene_uptime_ms, 1500);
        }

        #[test]
        fn g17_11_restart_scene() {
            // RESTART_SCENE（内容同 17.1，apply_mode=1）
            let mut scene = OutputScene::none();
            scene.apply_mode = RESTART_SCENE;
            assert_eq!(
                set_scene(5, &scene),
                hex(
                    "55 AA 04 00 05 20 00 3B 01 01 01 00 00 00 00 00 00 00 00 00 00 00 00 00
                     00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
                     00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 66"
                )
            );
        }

        #[test]
        fn g17_12_power_status() {
            let mut p = FrameParser::new();
            p.push(&hex("55 AA 04 00 01 D0 00 08 00 03 00 07 0F 3C 4B 03 7F"));
            let f = p.next_frame().unwrap();
            let (rc, ps) = parse_power_status_response(&f.data).unwrap();
            assert_eq!(rc, ResultCode::Ok);
            assert_eq!(ps.power_source, 3); // EXTERNAL_AND_BATTERY
            assert_eq!(ps.power_flags, 0x0007);
            assert_eq!(ps.battery_mv, 3900);
            assert_eq!(ps.battery_percent, 75);
            assert_eq!(ps.charge_state, 3); // CHARGING
        }

        #[test]
        fn g17_13_events() {
            let mut p = FrameParser::new();
            // DEVICE_READY
            p.push(&hex("55 AA 04 00 01 E0 00 06 04 01 00 00 01 01 F1"));
            let f = p.next_frame().unwrap();
            let ready = parse_device_ready(&f.data).unwrap();
            assert_eq!(ready.protocol_version, 4);
            assert_eq!(ready.fw, (1, 0, 0));
            assert_eq!(ready.hardware_variant, 1);
            assert_eq!(ready.boot_reason, 1); // 上电

            // POWER_CHANGED
            p.push(&hex("55 AA 04 00 02 E2 00 07 03 00 07 0F 3C 4B 03 91"));
            let f = p.next_frame().unwrap();
            let ps = parse_power_changed(&f.data).unwrap();
            assert_eq!(ps.battery_mv, 3900);
            assert_eq!(ps.battery_percent, 75);

            // BUTTON_EVENT 短按 120ms
            p.push(&hex("55 AA 04 00 03 E3 00 03 01 00 78 65"));
            let f = p.next_frame().unwrap();
            let be = parse_button_event(&f.data).unwrap();
            assert_eq!(be.event, 1);
            assert_eq!(be.duration_ms, 120);

            // FAULT_EVENT
            p.push(&hex("55 AA 04 00 04 EF 00 04 01 02 00 03 00"));
            let f = p.next_frame().unwrap();
            let fe = parse_fault_event(&f.data).unwrap();
            assert_eq!(fe.source, 1);
            assert_eq!(fe.code, 2);
            assert_eq!(fe.context, 3);
        }
    }

    mod frame_layer {
        use super::*;

        #[test]
        fn roundtrip_scene() {
            let led = LedTrack {
                curve: CURVE_TRIANGLE,
                low: Rgb(0x10, 0x20, 0x30),
                high: Rgb(0xA0, 0xB0, 0xC0),
                brightness: 77,
                period_ms: 1234,
                phase: 0x5555,
                duty_percent: 0,
                repeat_count: 0,
                end_level: END_HIGH,
            };
            let scene = OutputScene {
                apply_mode: RESTART_SCENE,
                transition_ms: 800,
                leds: [led; 3],
                buzzer: BuzzerTrack {
                    start_delay_ms: 100,
                    repeat_count: 2,
                    segments: vec![
                        BuzzerSegment {
                            frequency_hz: 440,
                            duration_ms: 200,
                            volume: 60,
                        },
                        BuzzerSegment {
                            frequency_hz: 0,
                            duration_ms: 100,
                            volume: 1,
                        },
                    ],
                },
            };
            let data = scene.encode_data();
            assert_eq!(data.len(), 59 + 5 * 2);
            let decoded = OutputScene::decode_data(&data).unwrap();
            assert_eq!(decoded, scene);
        }

        #[test]
        fn parser_split_and_sticky() {
            // 跨块组帧：一帧拆两半 + 粘包
            let f1 = ping(1);
            let f2 = reset_outputs(2);
            let mut combined = f1.clone();
            combined.extend_from_slice(&f2);
            let mut p = FrameParser::new();
            // 半个
            p.push(&combined[..f1.len() - 3]);
            assert!(p.next_frame().is_none());
            // 剩余
            p.push(&combined[f1.len() - 3..]);
            let got1 = p.next_frame().unwrap();
            assert_eq!((got1.cmd, got1.seq), (CMD_PING, 1));
            let got2 = p.next_frame().unwrap();
            assert_eq!((got2.cmd, got2.seq), (CMD_RESET_OUTPUTS, 2));
        }

        #[test]
        fn parser_skips_garbage_and_bad_checksum() {
            let f = ping(1);
            let mut buf = vec![0x00, 0x11, 0x22];
            buf.extend_from_slice(&f);
            let mut p = FrameParser::new();
            p.push(&buf);
            let got = p.next_frame().unwrap();
            assert_eq!(got.cmd, CMD_PING);

            // 校验和错误 → 跳过整帧
            let mut bad = f.clone();
            let last = bad.len() - 1;
            bad[last] = bad[last].wrapping_add(1);
            let mut p2 = FrameParser::new();
            p2.push(&bad);
            assert!(p2.next_frame().is_none());
        }

        #[test]
        fn parser_rejects_old_version() {
            // 版本字节非 0x04 → 丢弃
            let f = ping(1);
            let mut buf = f.clone();
            buf[2] = 0x03;
            // 修正校验和以通过校验和检查（版本不匹配优先于校验和）
            let last = buf.len() - 1;
            let sum: u32 = buf[..last].iter().map(|&b| b as u32).sum();
            buf[last] = (sum & 0xFF) as u8;
            let mut p = FrameParser::new();
            p.push(&buf);
            assert!(p.next_frame().is_none());
        }

        #[test]
        fn phase_conversion() {
            assert_eq!(phase_from_deg(0), 0);
            assert_eq!(phase_from_deg(120), 0x5555);
            assert_eq!(phase_from_deg(240), 0xAAAA);
            assert_eq!(phase_from_deg(360), 0);
        }

        #[test]
        fn response_parsers_need_more() {
            // 截断输入 → NeedMore（各应答解析器边界）
            assert!(matches!(
                parse_ping_response(&[0x00]),
                Err(ParseError::NeedMore)
            ));
            assert!(matches!(
                parse_device_info_response(&[0x00; 5]),
                Err(ParseError::NeedMore)
            ));
            assert!(matches!(
                parse_capabilities_response(&[0x00; 10]),
                Err(ParseError::NeedMore)
            ));
            assert!(matches!(
                parse_power_status_response(&[0x00; 3]),
                Err(ParseError::NeedMore)
            ));
            assert!(matches!(
                parse_output_status_response(&[0x00; 10]),
                Err(ParseError::NeedMore)
            ));
            assert!(matches!(
                parse_runtime_status_response(&[0x00; 5]),
                Err(ParseError::NeedMore)
            ));
            assert!(matches!(
                parse_set_scene_response(&[0x00; 2]),
                Err(ParseError::NeedMore)
            ));
            // 空数据 → 非法结果码（InvalidLength）
            assert_eq!(parse_result(&[]), ResultCode::InvalidLength);
            // 未知结果码透传
            assert_eq!(parse_result(&[0x7F]), ResultCode::Unknown(0x7F));
        }

        #[test]
        fn event_parsers_truncated() {
            assert!(parse_device_ready(&[4, 1]).is_none());
            assert!(parse_power_changed(&[0; 3]).is_none());
            assert!(parse_button_event(&[1]).is_none());
            assert!(parse_fault_event(&[1, 2]).is_none());
        }
    }
}
