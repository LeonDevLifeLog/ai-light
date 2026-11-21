//! AgentCore-Light USB HID V1.0. Only the scene BODY is shared with BLE V0.4.

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::protocol::{self, OutputScene};

pub(crate) const VERSION: u8 = 0x01;
pub(crate) const SIGNATURE: &[u8; 4] = b"ACLU";
pub(crate) const REPORT_SIZE: usize = 64;
const FRAGMENT_SIZE: usize = REPORT_SIZE - 4;
const MAX_FRAME_SIZE: usize = 244;
const FRAME_OVERHEAD: usize = 9;
const START: u8 = 1;
const END: u8 = 2;

pub(crate) const RESPONSE_TIMEOUT: Duration = Duration::from_millis(500);
pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_millis(1000);
pub(crate) const LEASE_TIMEOUT: Duration = Duration::from_millis(3500);

pub(crate) const GET_DEVICE_INFO: u8 = 0x02;
pub(crate) const GET_RUNTIME_STATUS: u8 = 0x03;
pub(crate) const GET_CAPABILITIES: u8 = 0x04;
pub(crate) const RESET_OUTPUTS: u8 = 0x05;
pub(crate) const OPEN_SESSION: u8 = 0x10;
pub(crate) const HEARTBEAT: u8 = 0x11;
pub(crate) const CLOSE_SESSION: u8 = 0x12;
pub(crate) const SET_SCENE: u8 = 0x20;
pub(crate) const GET_OUTPUT_STATUS: u8 = 0x21;

pub(crate) type Report = [u8; REPORT_SIZE];

pub(crate) fn encode_frame(cmd: u8, seq: u16, session_id: u32, body: &[u8]) -> Vec<u8> {
    let data_len = 4 + body.len();
    assert!(data_len <= MAX_FRAME_SIZE - FRAME_OVERHEAD);
    let mut frame = Vec::with_capacity(FRAME_OVERHEAD + data_len);
    frame.extend_from_slice(&protocol::FRAME_HEADER);
    frame.push(VERSION);
    frame.extend_from_slice(&seq.to_be_bytes());
    frame.push(cmd);
    frame.extend_from_slice(&(data_len as u16).to_be_bytes());
    frame.extend_from_slice(&session_id.to_be_bytes());
    frame.extend_from_slice(body);
    frame.push(protocol::calc_checksum(&frame));
    frame
}

pub(crate) fn fragment(frame: &[u8]) -> Vec<Report> {
    let parts = frame.chunks(FRAGMENT_SIZE);
    let last = parts.len() - 1;
    parts
        .enumerate()
        .map(|(index, payload)| {
            let mut report = [0; REPORT_SIZE];
            report[0] = u8::from(index == 0) * START | u8::from(index == last) * END;
            report[1] = frame[4];
            report[2] = index as u8;
            report[3] = payload.len() as u8;
            report[4..4 + payload.len()].copy_from_slice(payload);
            report
        })
        .collect()
}

#[derive(Debug)]
pub(crate) struct Frame {
    version: u8,
    pub(crate) seq: u16,
    pub(crate) cmd: u8,
    pub(crate) session_id: u32,
    pub(crate) payload: Vec<u8>,
}

impl Frame {
    fn decode(raw: &[u8]) -> Option<Self> {
        let (checksum, bytes) = raw.split_last()?;
        if !(FRAME_OVERHEAD + 4..=MAX_FRAME_SIZE).contains(&raw.len())
            || !raw.starts_with(&protocol::FRAME_HEADER)
            || usize::from(be16(raw, 6)) + FRAME_OVERHEAD != raw.len()
            || protocol::calc_checksum(bytes) != *checksum
        {
            return None;
        }
        Some(Self {
            version: raw[2],
            seq: be16(raw, 3),
            cmd: raw[5],
            session_id: be32(raw, 8),
            payload: bytes[12..].to_vec(),
        })
    }

    pub(crate) fn response(
        self,
        cmd: u8,
        seq: u16,
        session_id: u32,
        request: &[u8],
    ) -> Option<Response> {
        if self.version != VERSION || self.seq != seq || self.cmd != protocol::response_cmd(cmd) {
            return None;
        }
        let (&result, body) = self.payload.split_first()?;
        if result != 0 {
            if self.session_id != session_id || !body.is_empty() {
                return None;
            }
        } else {
            let expected = match cmd {
                GET_DEVICE_INFO => 28,
                GET_CAPABILITIES => 26,
                OPEN_SESSION => 8,
                HEARTBEAT => 10,
                GET_RUNTIME_STATUS => 16,
                GET_OUTPUT_STATUS => 20,
                SET_SCENE => 3,
                RESET_OUTPUTS | CLOSE_SESSION => 0,
                _ => return None,
            };
            if body.len() != expected {
                return None;
            }
            if cmd == OPEN_SESSION {
                if self.session_id == 0
                    || body.get(..4) != request.get(6..10)
                    || u128::from(be16(body, 4)) != HEARTBEAT_INTERVAL.as_millis()
                    || u128::from(be16(body, 6)) != LEASE_TIMEOUT.as_millis()
                {
                    return None;
                }
            } else if self.session_id != session_id {
                return None;
            }
            if cmd == HEARTBEAT
                && (body.get(..4) != Some(request)
                    || be16(body, 4) == 0
                    || u128::from(be16(body, 4)) > LEASE_TIMEOUT.as_millis())
            {
                return None;
            }
            if cmd == SET_SCENE && body[0] > 1 {
                return None;
            }
        }
        Some(Response {
            session_id: self.session_id,
            result,
            body: body.to_vec(),
        })
    }
}

#[derive(Debug)]
pub(crate) struct Response {
    pub(crate) session_id: u32,
    pub(crate) result: u8,
    pub(crate) body: Vec<u8>,
}

struct Assembly {
    id: u8,
    start: Instant,
    bytes: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct Decoder {
    pending: Option<Assembly>,
}

impl Decoder {
    pub(crate) fn push(&mut self, report: &[u8], now: Instant) -> Option<Frame> {
        // A bad report discards the entire partial frame. Recovery starts at the next START.
        let previous = self.pending.take();
        let report: &Report = report.try_into().ok()?;
        let flags = report[0];
        let id = report[1];
        let index = usize::from(report[2]);
        let len = usize::from(report[3]);
        let end = flags & END != 0;
        if flags & !(START | END) != 0
            || !(1..=FRAGMENT_SIZE).contains(&len)
            || index > 4
            || (!end && (len != FRAGMENT_SIZE || index == 4))
        {
            return None;
        }
        let mut assembly = if flags & START != 0 {
            Assembly {
                id,
                start: now,
                bytes: Vec::with_capacity(MAX_FRAME_SIZE),
            }
        } else {
            previous?
        };
        if now.duration_since(assembly.start) >= RESPONSE_TIMEOUT
            || assembly.id != id
            || assembly.bytes.len() != index * FRAGMENT_SIZE
            || assembly.bytes.len() + len > MAX_FRAME_SIZE
        {
            return None;
        }
        assembly.bytes.extend_from_slice(&report[4..4 + len]);
        if !end {
            self.pending = Some(assembly);
            return None;
        }
        Frame::decode(&assembly.bytes).filter(|frame| frame.seq as u8 == id)
    }
}

pub(crate) fn be16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
}

fn be32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub uid: String,
    pub firmware: (u8, u8, u8),
    pub hardware_revision: u16,
}

impl DeviceInfo {
    pub(crate) fn decode(body: &[u8], serial: &str) -> Result<Self, &'static str> {
        if body.len() != 28 || !body.starts_with(SIGNATURE) || be16(body, 11) != 1 {
            return Err("USB 设备签名或产品型号不匹配");
        }
        if body[4..6] != [1, 0] || body[25] != 3 {
            return Err("USB 协议版本或 RGB 灯数不匹配");
        }
        let uid_bytes = &body[13..25];
        let uid = uid_bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        if uid_bytes == [0; 12] || uid_bytes == [0xff; 12] || uid != serial {
            return Err("HID 序列号与设备 UID 不匹配");
        }
        Ok(Self {
            uid,
            firmware: (body[6], body[7], body[8]),
            hardware_revision: be16(body, 9),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub capability_bits: u32,
    supported_curves: u16,
    min_period_ms: u16,
    max_period_ms: u16,
    max_transition_ms: u16,
    max_buzzer_segments: u8,
    min_frequency_hz: u16,
    max_frequency_hz: u16,
    max_volume: u8,
    render_tick_ms: u16,
    pub max_rgb_brightness: u8,
}

impl Capabilities {
    pub(crate) fn decode(body: &[u8]) -> Result<Self, &'static str> {
        const REQUIRED_BITS: u32 = 0x1805;
        if body.len() != 26 || body[0] != 1 || body[5] != 3 {
            return Err("USB 设备能力结构或灯数不匹配");
        }
        let caps = Self {
            capability_bits: be32(body, 1),
            supported_curves: be16(body, 6),
            min_period_ms: be16(body, 8),
            max_period_ms: be16(body, 10),
            max_transition_ms: be16(body, 12),
            max_buzzer_segments: body[14],
            min_frequency_hz: be16(body, 15),
            max_frequency_hz: be16(body, 17),
            max_volume: body[19],
            render_tick_ms: be16(body, 22),
            max_rgb_brightness: body[24],
        };
        if caps.capability_bits & !protocol::CAP_PASSIVE_BUZZER != REQUIRED_BITS
            || caps.supported_curves & 0x1f != 0x1f
            || caps.min_period_ms != 40
            || caps.max_period_ms != u16::MAX
            || caps.max_transition_ms != 5000
            || caps.render_tick_ms != 20
            || !(1..=100).contains(&caps.max_rgb_brightness)
        {
            return Err("USB 设备声明的灯轨能力不符合 V1.0");
        }
        let buzzer_valid = if caps.supports_buzzer() {
            (1..=16).contains(&caps.max_buzzer_segments)
                && (1..=caps.max_frequency_hz).contains(&caps.min_frequency_hz)
                && (1..=100).contains(&caps.max_volume)
        } else {
            caps.max_buzzer_segments == 0
                && caps.min_frequency_hz == 0
                && caps.max_frequency_hz == 0
                && caps.max_volume == 0
        };
        if !buzzer_valid {
            return Err("USB 设备声明的蜂鸣能力不符合 V1.0");
        }
        Ok(caps)
    }

    pub fn supports_buzzer(&self) -> bool {
        self.capability_bits & protocol::CAP_PASSIVE_BUZZER != 0
    }

    pub(crate) fn scene_body(&self, scene: &OutputScene) -> Result<Vec<u8>, String> {
        let mut scene = scene.clone();
        if scene.transition_ms > self.max_transition_ms {
            return Err(format!(
                "transition_ms 超过 USB 最大值 {}",
                self.max_transition_ms
            ));
        }
        for (index, led) in scene.leds.iter_mut().enumerate() {
            led.brightness = led.brightness.min(self.max_rgb_brightness);
            if led.curve > protocol::CURVE_SAW_DOWN {
                return Err(format!("leds[{index}].curve 不受 USB 设备支持"));
            }
            if led.curve == protocol::CURVE_CONSTANT {
                continue;
            }
            if !(self.min_period_ms..=self.max_period_ms).contains(&led.period_ms) {
                return Err(format!("leds[{index}].period_ms 超出 USB 设备范围"));
            }
            if led.curve == protocol::CURVE_SQUARE {
                let duty = u32::from(led.duty_percent);
                if !(1..=99).contains(&duty)
                    || u32::from(led.period_ms) * duty.min(100 - duty)
                        < 100 * u32::from(self.render_tick_ms)
                {
                    return Err(format!(
                        "leds[{index}] 方波的亮和灭均须至少 {} ms",
                        self.render_tick_ms
                    ));
                }
            }
        }
        if !self.supports_buzzer() {
            scene.buzzer.segments.clear();
        }
        if scene.buzzer.segments.len() > usize::from(self.max_buzzer_segments) {
            return Err("buzzer.segments 超过 USB 设备支持的段数".into());
        }
        for (index, segment) in scene.buzzer.segments.iter().enumerate() {
            if segment.duration_ms == 0 {
                return Err(format!("buzzer.segments[{index}].duration_ms 必须大于零"));
            }
            if segment.volume > self.max_volume {
                return Err(format!("buzzer.segments[{index}].volume 超出 USB 设备范围"));
            }
            if segment.frequency_hz != 0
                && !(self.min_frequency_hz..=self.max_frequency_hz).contains(&segment.frequency_hz)
            {
                return Err(format!(
                    "buzzer.segments[{index}].frequency_hz 超出 USB 设备范围"
                ));
            }
        }
        Ok(scene.encode_data())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub usb_state: u8,
    pub status_led1_mode: u8,
    pub status_led2_mode: u8,
    pub active_session_id: u32,
    pub lease_remaining_ms: u16,
    pub uptime_ms: u32,
    pub scene_valid: bool,
    pub fault_flags: u16,
}

impl RuntimeStatus {
    pub(crate) fn decode(body: &[u8]) -> Option<Self> {
        if body.len() != 16 || body[13] > 1 {
            return None;
        }
        Some(Self {
            usb_state: body[0],
            status_led1_mode: body[1],
            status_led2_mode: body[2],
            active_session_id: be32(body, 3),
            lease_remaining_ms: be16(body, 7),
            uptime_ms: be32(body, 9),
            scene_valid: body[13] == 1,
            fault_flags: be16(body, 14),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputStatus {
    pub scene_digest: u16,
    pub scene_state: u8,
    pub rgb: [[u8; 3]; 3],
    pub buzzer_state: u8,
    pub buzzer_frequency_hz: u16,
    pub buzzer_volume: u8,
    pub scene_uptime_ms: u32,
}

impl OutputStatus {
    pub(crate) fn decode(body: &[u8]) -> Option<Self> {
        if body.len() != 20 || body[2] > 3 || body[12] > 1 {
            return None;
        }
        Some(Self {
            scene_digest: be16(body, 0),
            scene_state: body[2],
            rgb: [
                body[3..6].try_into().ok()?,
                body[6..9].try_into().ok()?,
                body[9..12].try_into().ok()?,
            ],
            buzzer_state: body[12],
            buzzer_frequency_hz: be16(body, 13),
            buzzer_volume: body[15],
            scene_uptime_ms: be32(body, 16),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{BuzzerSegment, Rgb};

    fn hex(line: &str) -> Vec<u8> {
        line.split_whitespace()
            .map(|byte| u8::from_str_radix(byte, 16).unwrap())
            .collect()
    }

    fn fixture_response(raw: &str, cmd: u8, seq: u16, sid: u32, request: &[u8]) -> Response {
        Frame::decode(&hex(raw))
            .unwrap()
            .response(cmd, seq, sid, request)
            .unwrap()
    }

    #[test]
    fn handshake_and_session_requests_match_appendix_b() {
        assert_eq!(
            encode_frame(GET_DEVICE_INFO, 0, 0, &[]),
            hex("55 AA 01 00 00 02 00 04 00 00 00 00 06")
        );
        assert_eq!(
            encode_frame(GET_CAPABILITIES, 1, 0, &[]),
            hex("55 AA 01 00 01 04 00 04 00 00 00 00 09")
        );
        let open = hex("41 43 4C 55 01 00 12 34 56 78");
        assert_eq!(
            encode_frame(OPEN_SESSION, 2, 0, &open),
            hex("55 AA 01 00 02 10 00 0E 00 00 00 00 41 43 4C 55 01 00 12 34 56 78 5A")
        );
        let opened = fixture_response(
            "55 AA 01 00 02 90 00 0D 00 00 00 01 00 12 34 56 78 03 E8 0D AC 58",
            OPEN_SESSION,
            2,
            0,
            &open,
        );
        assert_eq!(opened.session_id, 1);
        let heartbeat = [0, 0, 0, 1];
        assert_eq!(
            encode_frame(HEARTBEAT, 4, 1, &heartbeat),
            hex("55 AA 01 00 04 11 00 08 00 00 00 01 00 00 00 01 1F")
        );
        let renewed = fixture_response(
            "55 AA 01 00 04 91 00 0F 00 00 00 01 00 00 00 00 01 0D AC 00 00 2E E0 6D",
            HEARTBEAT,
            4,
            1,
            &heartbeat,
        );
        assert_eq!(be16(&renewed.body, 4), 3500);
        assert_eq!(
            encode_frame(RESET_OUTPUTS, 6, 1, &[]),
            hex("55 AA 01 00 06 05 00 04 00 00 00 01 10")
        );
        assert_eq!(
            encode_frame(CLOSE_SESSION, 7, 1, &[]),
            hex("55 AA 01 00 07 12 00 04 00 00 00 01 1E")
        );
        assert_eq!(
            fixture_response(
                "55 AA 01 00 07 92 00 05 00 00 00 01 00 9F",
                CLOSE_SESSION,
                7,
                1,
                &[]
            )
            .result,
            0
        );
    }

    fn capabilities() -> Capabilities {
        let response = fixture_response(
            "55 AA 01 00 01 84 00 1F 00 00 00 00 00 01 00 00 18 07 03 00 1F 00 28 FF FF 13 88 10 00 64 27 10 64 00 00 00 14 64 00 2E",
            GET_CAPABILITIES, 1, 0, &[],
        );
        Capabilities::decode(&response.body).unwrap()
    }

    #[test]
    fn identity_and_scene_match_appendix_b() {
        let uid = "00112233445566778899AABB";
        let response = fixture_response(
            "55 AA 01 00 00 82 00 21 00 00 00 00 00 41 43 4C 55 01 00 00 01 04 01 00 00 01 00 11 22 33 44 55 66 77 88 99 AA BB 03 00 00 35",
            GET_DEVICE_INFO, 0, 0, &[],
        );
        let device = DeviceInfo::decode(&response.body, uid).unwrap();
        assert_eq!(device.firmware, (0, 1, 4));
        assert_eq!(device.hardware_revision, 0x0100);
        assert!(DeviceInfo::decode(&response.body, "wrong").is_err());

        let mut scene = OutputScene::none();
        scene.leds[1].high = Rgb(0, 0, 255);
        scene.leds[1].brightness = 20;
        let frame = encode_frame(SET_SCENE, 3, 1, &capabilities().scene_body(&scene).unwrap());
        assert_eq!(frame.len(), 72);
        assert_eq!(frame[71], 0x78);
        let reports = fragment(&frame);
        assert_eq!(
            reports[0],
            hex("01 03 00 3C 55 AA 01 00 03 20 00 3F 00 00 00 01
             01 01 00 00 00 00 00 00 00 00 00 00 00 00 00 00
             00 00 00 00 00 00 00 00 00 00 00 00 FF 14 00 00
             00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00")
            .as_slice()
        );
        assert_eq!(
            reports[1],
            hex("02 03 01 0C 00 00 00 00 00 00 00 00 00 00 00 78
             00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
             00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00
             00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00")
            .as_slice()
        );
        let ack = fixture_response(
            "55 AA 01 00 03 A0 00 08 00 00 00 01 00 01 01 15 C3",
            SET_SCENE,
            3,
            1,
            &[],
        );
        assert_eq!(ack.body, [1, 1, 0x15]);
    }

    #[test]
    fn reassembly_recovers_at_start_after_invalid_or_stale_fragments() {
        let frame = encode_frame(SET_SCENE, 27, 1, &OutputScene::none().encode_data());
        let reports = fragment(&frame);
        let start = Instant::now();
        let mut decoder = Decoder::default();
        assert!(decoder.push(&reports[1], start).is_none());
        assert!(decoder.push(&reports[0], start).is_none());
        assert!(decoder
            .push(&reports[1], start + RESPONSE_TIMEOUT)
            .is_none());

        let mut wrong_index = reports[1];
        wrong_index[2] = 2;
        assert!(decoder.push(&reports[0], start).is_none());
        assert!(decoder.push(&wrong_index, start).is_none());
        assert!(decoder.push(&reports[1], start).is_none());

        let mut bad_checksum = reports[1];
        bad_checksum[15] ^= 1;
        assert!(decoder.push(&reports[0], start).is_none());
        assert!(decoder.push(&bad_checksum, start).is_none());

        let mut padded = reports[1];
        padded[16..18].copy_from_slice(&[0x55, 0xaa]);
        assert!(decoder.push(&reports[0], start).is_none());
        assert_eq!(decoder.push(&padded, start).unwrap().payload, frame[12..71]);
    }

    #[test]
    fn matching_requires_full_sequence_session_and_open_nonce() {
        let body = [0, 1, 0, 0];
        let frame = encode_frame(0xA0, 0x0102, 7, &body);
        assert!(Frame::decode(&frame)
            .unwrap()
            .response(SET_SCENE, 2, 7, &[])
            .is_none());
        assert!(Frame::decode(&frame)
            .unwrap()
            .response(SET_SCENE, 0x0102, 8, &[])
            .is_none());
        assert!(Frame::decode(&frame)
            .unwrap()
            .response(GET_OUTPUT_STATUS, 0x0102, 7, &[])
            .is_none());
        let open = encode_frame(0x90, 2, 7, &hex("00 12 34 56 78 03 E8 0D AC"));
        let mismatched_nonce = hex("41 43 4C 55 01 00 12 34 56 79");
        assert!(Frame::decode(&open)
            .unwrap()
            .response(OPEN_SESSION, 2, 0, &mismatched_nonce)
            .is_none());

        let expired = fixture_response(
            "55 AA 01 00 08 91 00 05 00 00 00 01 0C AB",
            HEARTBEAT,
            8,
            1,
            &[0, 0, 0, 2],
        );
        assert_eq!(expired.result, 0x0C);
        assert!(expired.body.is_empty());
        let invalid_error = encode_frame(0xA0, 1, 1, &[2, 0, 0, 0]);
        assert!(Frame::decode(&invalid_error)
            .unwrap()
            .response(SET_SCENE, 1, 1, &[])
            .is_none());
    }

    #[test]
    fn usb_scene_limits_preserve_silence_and_zero_brightness() {
        let mut caps = capabilities();
        let mut scene = OutputScene::none();
        scene.leds[1].brightness = 90;
        scene.buzzer.start_delay_ms = 3;
        scene.buzzer.segments = vec![
            BuzzerSegment {
                frequency_hz: 0,
                duration_ms: 5,
                volume: 40
            };
            16
        ];
        caps.max_rgb_brightness = 20;
        let body = caps.scene_body(&scene).unwrap();
        let frame = encode_frame(SET_SCENE, 1, 1, &body);
        assert_eq!(frame.len(), 152);
        assert_eq!(
            fragment(&frame)
                .iter()
                .map(|report| report[3])
                .collect::<Vec<_>>(),
            [60, 60, 32]
        );
        let compiled = OutputScene::decode_data(&body).unwrap();
        assert_eq!(compiled.leds[0].brightness, 0);
        assert_eq!(compiled.leds[1].brightness, 20);
        assert_eq!(compiled.buzzer.segments[0].volume, 40);

        caps.capability_bits &= !protocol::CAP_PASSIVE_BUZZER;
        caps.max_buzzer_segments = 0;
        let compiled = OutputScene::decode_data(&caps.scene_body(&scene).unwrap()).unwrap();
        assert!(compiled.buzzer.segments.is_empty());
        assert_eq!(compiled.buzzer.start_delay_ms, 3);
        scene.leds[0].curve = protocol::CURVE_SQUARE;
        scene.leds[0].period_ms = 100;
        scene.leds[0].duty_percent = 10;
        assert!(caps.scene_body(&scene).unwrap_err().contains("leds[0]"));
    }

    #[test]
    fn status_bodies_decode_with_usb_offsets() {
        let runtime =
            RuntimeStatus::decode(&hex("02 01 01 00 00 00 01 0D AC 00 00 2E E0 01 00 10")).unwrap();
        assert_eq!(runtime.usb_state, 2);
        assert_eq!(runtime.active_session_id, 1);
        assert_eq!(runtime.lease_remaining_ms, 3500);
        assert!(runtime.scene_valid);
        assert_eq!(runtime.fault_flags, 16);
        let output = OutputStatus::decode(&hex(
            "03 3B 01 33 00 00 00 33 00 00 00 33 01 09 C4 1E 00 00 10 CC",
        ))
        .unwrap();
        assert_eq!(output.scene_digest, 0x033B);
        assert_eq!(output.rgb, [[51, 0, 0], [0, 51, 0], [0, 0, 51]]);
        assert_eq!(output.buzzer_state, 1);
        assert_eq!(output.buzzer_frequency_hz, 2500);
        assert_eq!(output.buzzer_volume, 30);
        assert_eq!(output.scene_uptime_ms, 4300);
        assert!(RuntimeStatus::decode(&[0; 15]).is_none());
        assert!(OutputStatus::decode(&[0; 21]).is_none());
    }
}
