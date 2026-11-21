//! HID discovery and a single blocking worker for each USB application session.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use hidapi::{BusType, HidApi, HidDevice};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot, watch};

use crate::protocol::OutputScene;
use crate::usb_protocol::{
    self as wire, Decoder, Frame, Report, Response, CLOSE_SESSION, GET_CAPABILITIES,
    GET_DEVICE_INFO, GET_OUTPUT_STATUS, GET_RUNTIME_STATUS, HEARTBEAT, HEARTBEAT_INTERVAL,
    LEASE_TIMEOUT, OPEN_SESSION, REPORT_SIZE, RESET_OUTPUTS, RESPONSE_TIMEOUT, SET_SCENE,
};

pub use crate::usb_protocol::{Capabilities, DeviceInfo, OutputStatus, RuntimeStatus};

const ATTEMPTS: u32 = 3;
const IDLE_READ: Duration = Duration::from_millis(25);
const FAULT_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub enum Error {
    NotFound,
    Io(String),
    Protocol(&'static str),
    InvalidScene(String),
    Rejected { command: u8, result: u8 },
    Timeout,
    Closed,
}

impl Error {
    fn ends_session(&self) -> bool {
        !matches!(
            self,
            Self::Rejected {
                result: 0x01..=0x05 | 0x07..=0x0b,
                ..
            }
        )
    }

    pub fn is_scene_rejection(&self) -> bool {
        matches!(self, Self::InvalidScene(_))
            || matches!(
                self,
                Self::Rejected {
                    command: SET_SCENE,
                    ..
                }
            ) && !self.ends_session()
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "未找到指定的 USB 灯牌"),
            Self::Io(error) => write!(f, "USB HID 访问失败：{error}"),
            Self::Protocol(message) => f.write_str(message),
            Self::InvalidScene(message) => f.write_str(message),
            Self::Rejected {
                command: OPEN_SESSION,
                result: 0x04,
            } => {
                write!(f, "USB 灯牌已被其他会话使用，请断开原控制端后重试")
            }
            Self::Rejected { command, result } => {
                write!(f, "USB 设备拒绝命令 0x{command:02X}，结果码 0x{result:02X}")
            }
            Self::Timeout => write!(f, "等待 USB 灯牌应答超时"),
            Self::Closed => write!(f, "USB 会话已关闭"),
        }
    }
}

impl std::error::Error for Error {}

impl From<hidapi::HidError> for Error {
    fn from(error: hidapi::HidError) -> Self {
        Self::Io(error.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub usb_uid: String,
    pub name: String,
}

type HidJob = Box<dyn FnOnce() + Send>;

async fn on_hid_manager<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    // HIDAPI's macOS IOHIDManager keeps the run loop of its initializing thread.
    // Keep that thread alive for the process and serialize enumeration and open there.
    static JOBS: OnceLock<Result<std::sync::mpsc::Sender<HidJob>, Error>> = OnceLock::new();
    let jobs = JOBS
        .get_or_init(|| {
            let (jobs, incoming) = std::sync::mpsc::channel::<HidJob>();
            std::thread::Builder::new()
                .name("ailight-hid-manager".into())
                .spawn(move || {
                    for job in incoming {
                        job();
                    }
                })
                .map_err(|error| Error::Io(error.to_string()))?;
            Ok(jobs)
        })
        .as_ref()
        .map_err(Clone::clone)?;
    let (reply, done) = oneshot::channel();
    jobs.send(Box::new(move || {
        if !reply.is_closed() {
            let _ = reply.send(operation());
        }
    }))
    .map_err(|_| Error::Closed)?;
    done.await.map_err(|_| Error::Closed)?
}

fn candidate(device: &hidapi::DeviceInfo, vid: u16, pid: u16) -> Option<Candidate> {
    if device.vendor_id() != vid
        || device.product_id() != pid
        || !matches!(device.bus_type(), BusType::Usb | BusType::Unknown)
        || !matches!(device.interface_number(), 0 | -1)
        || !matches!((device.usage_page(), device.usage()), (0xff00, 1) | (0, 0))
    {
        return None;
    }
    let serial = device.serial_number()?;
    if serial.len() != 24 || !serial.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(Candidate {
        usb_uid: serial.to_ascii_uppercase(),
        name: device
            .product_string()
            .unwrap_or("AgentCore-Light USB")
            .to_owned(),
    })
}

/// Find descriptor candidates using an explicitly configured VID/PID pair.
pub async fn scan(vid: u16, pid: u16) -> Result<Vec<Candidate>, Error> {
    on_hid_manager(move || {
        let api = HidApi::new()?;
        let mut devices: Vec<_> = api
            .device_list()
            .filter_map(|device| candidate(device, vid, pid))
            .collect();
        devices.sort_by(|left, right| left.usb_uid.cmp(&right.usb_uid));
        devices.dedup_by(|left, right| left.usb_uid == right.usb_uid);
        Ok(devices)
    })
    .await
}

#[derive(Debug, Clone)]
pub struct Handshake {
    pub device: DeviceInfo,
    pub capabilities: Capabilities,
    pub name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub runtime: RuntimeStatus,
    pub output: OutputStatus,
}

struct Request {
    command: u8,
    body: Vec<u8>,
    reply: oneshot::Sender<Result<Response, Error>>,
}

#[derive(Clone)]
pub struct Session {
    pub handshake: Handshake,
    requests: mpsc::Sender<Request>,
    closing: Arc<AtomicBool>,
    finished: watch::Receiver<Option<Result<(), Error>>>,
    fault_flags: watch::Receiver<u16>,
}

impl Session {
    pub async fn open(vid: u16, pid: u16, uid: String) -> Result<Self, Error> {
        let (hid, candidate) = on_hid_manager(move || open_candidate(vid, pid, &uid)).await?;
        let (requests, mut incoming) = mpsc::channel(8);
        let (ready, waiting) = oneshot::channel();
        let (finished, completion) = watch::channel(None);
        let (fault_updates, fault_flags) = watch::channel(0);
        let closing = Arc::new(AtomicBool::new(false));
        let worker_closing = closing.clone();
        std::thread::Builder::new()
            .name("ailight-usb".into())
            .spawn(move || {
                let (mut device, info, opened_at) = match handshake(hid, candidate) {
                    Ok(opened) => opened,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let result = if ready.send(Ok(info)).is_ok() {
                    device.serve(&mut incoming, &worker_closing, &fault_updates, opened_at)
                } else {
                    Ok(())
                };
                let result = result.and_then(|()| {
                    device.request(CLOSE_SESSION, &[])?;
                    tracing::debug!(event = "usb_session_closed", "USB 会话已关闭");
                    Ok(())
                });
                drop(device);
                finished.send_replace(Some(result));
            })
            .map_err(|error| Error::Io(error.to_string()))?;
        let handshake = waiting.await.map_err(|_| Error::Closed)??;
        Ok(Self {
            handshake,
            requests,
            closing,
            finished: completion,
            fault_flags,
        })
    }

    async fn request(&self, command: u8, body: Vec<u8>) -> Result<Response, Error> {
        if self.closing.load(Ordering::Acquire) {
            return Err(Error::Closed);
        }
        let (reply, done) = oneshot::channel();
        self.requests
            .send(Request {
                command,
                body,
                reply,
            })
            .await
            .map_err(|_| Error::Closed)?;
        done.await.map_err(|_| Error::Closed)?
    }

    pub async fn set_scene(&self, scene: &OutputScene) -> Result<u16, Error> {
        let body = self
            .handshake
            .capabilities
            .scene_body(scene)
            .map_err(Error::InvalidScene)?;
        let response = self.request(SET_SCENE, body).await?;
        let digest = wire::be16(&response.body, 1);
        tracing::debug!(
            event = "usb_scene_acknowledged",
            applied = response.body[0] != 0,
            digest
        );
        Ok(digest)
    }

    pub async fn reset_outputs(&self) -> Result<(), Error> {
        self.request(RESET_OUTPUTS, vec![]).await?;
        Ok(())
    }

    pub async fn output_status(&self) -> Result<OutputStatus, Error> {
        let response = self.request(GET_OUTPUT_STATUS, vec![]).await?;
        OutputStatus::decode(&response.body).ok_or(Error::Protocol("USB 输出状态格式错误"))
    }

    pub async fn status(&self) -> Result<Status, Error> {
        let response = self.request(GET_RUNTIME_STATUS, vec![]).await?;
        let runtime = decode_runtime(&response)?;
        let output = self.output_status().await?;
        Ok(Status { runtime, output })
    }

    pub fn fault_updates(&self) -> watch::Receiver<u16> {
        self.fault_flags.clone()
    }

    pub async fn wait_closed(&self) -> Result<(), Error> {
        self.finished
            .clone()
            .wait_for(Option::is_some)
            .await
            .map_err(|_| Error::Closed)?
            .clone()
            .ok_or(Error::Closed)?
    }

    pub async fn close(&self) -> Result<(), Error> {
        self.closing.store(true, Ordering::Release);
        self.wait_closed().await
    }
}

fn decode_runtime(response: &Response) -> Result<RuntimeStatus, Error> {
    let runtime =
        RuntimeStatus::decode(&response.body).ok_or(Error::Protocol("USB 运行状态格式错误"))?;
    if runtime.usb_state != 2 || runtime.active_session_id != response.session_id {
        return Err(Error::Protocol("USB 运行状态与当前会话不匹配"));
    }
    Ok(runtime)
}

fn open_candidate(vid: u16, pid: u16, uid: &str) -> Result<(HidDevice, Candidate), Error> {
    let api = HidApi::new()?;
    let mut matching = api.device_list().filter_map(|device| {
        let candidate = candidate(device, vid, pid)?;
        (candidate.usb_uid == uid).then_some((device, candidate))
    });
    let (descriptor, candidate) = matching.next().ok_or(Error::NotFound)?;
    if matching.any(|(other, _)| other.path() != descriptor.path()) {
        return Err(Error::Protocol("多台 USB 灯牌使用相同的序列号"));
    }
    Ok((api.open_path(descriptor.path())?, candidate))
}

fn handshake(
    hid: HidDevice,
    candidate: Candidate,
) -> Result<(Connection<HidDevice>, Handshake, Instant), Error> {
    let mut connection = Connection::new(hid);
    let info = connection.request(GET_DEVICE_INFO, &[])?;
    let device = DeviceInfo::decode(&info.body, &candidate.usb_uid).map_err(Error::Protocol)?;
    let caps = connection.request(GET_CAPABILITIES, &[])?;
    let capabilities = Capabilities::decode(&caps.body).map_err(Error::Protocol)?;

    let nonce = (uuid::Uuid::new_v4().as_u128() as u32).max(1);
    let mut open = [0; 10];
    open[..4].copy_from_slice(wire::SIGNATURE);
    open[4] = 1;
    open[6..].copy_from_slice(&nonce.to_be_bytes());
    let opened_at = Instant::now();
    connection.session_id = connection.request(OPEN_SESSION, &open)?.session_id;
    tracing::info!(
        event = "usb_session_opened",
        firmware = ?device.firmware,
        capability_bits = capabilities.capability_bits,
        max_rgb_brightness = capabilities.max_rgb_brightness,
        "USB 灯牌握手成功"
    );
    Ok((
        connection,
        Handshake {
            device,
            capabilities,
            name: candidate.name,
        },
        opened_at,
    ))
}

trait ReportIo {
    fn send(&mut self, report: &Report) -> Result<(), Error>;
    fn receive(&mut self, data: &mut [u8], timeout: Duration) -> Result<usize, Error>;
}

impl ReportIo for HidDevice {
    fn send(&mut self, report: &Report) -> Result<(), Error> {
        let mut buffer = [0; REPORT_SIZE + 1];
        buffer[1..].copy_from_slice(report);
        if self.write(&buffer)? != buffer.len() {
            return Err(Error::Io("HID 报告写入长度不足".into()));
        }
        Ok(())
    }

    fn receive(&mut self, data: &mut [u8], timeout: Duration) -> Result<usize, Error> {
        Ok(self.read_timeout(data, timeout.as_millis().max(1) as i32)?)
    }
}

struct Connection<I> {
    io: I,
    decoder: Decoder,
    seq: u16,
    session_id: u32,
}

impl<I: ReportIo> Connection<I> {
    fn new(io: I) -> Self {
        Self {
            io,
            decoder: Decoder::default(),
            seq: 0,
            session_id: 0,
        }
    }

    fn receive(&mut self, timeout: Duration) -> Result<Option<Frame>, Error> {
        // The extra byte also lets us detect a report with an unexpected ID.
        let mut buffer = [0; REPORT_SIZE + 1];
        let len = self.io.receive(&mut buffer, timeout)?;
        if len == 0 {
            return Ok(None);
        }
        Ok(self.decoder.push(&buffer[..len], Instant::now()))
    }

    fn request(&mut self, command: u8, body: &[u8]) -> Result<Response, Error> {
        let seq = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let reports = wire::fragment(&wire::encode_frame(command, seq, self.session_id, body));
        let mut error = Error::Timeout;
        for _ in 0..ATTEMPTS {
            // Retry starts with the same START report even after a partial write.
            if let Err(write_error) = reports.iter().try_for_each(|report| self.io.send(report)) {
                error = write_error;
                continue;
            }
            error = Error::Timeout;
            let deadline = Instant::now() + RESPONSE_TIMEOUT;
            while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
                let Some(frame) = self.receive(remaining)? else {
                    continue;
                };
                let Some(response) = frame.response(command, seq, self.session_id, body) else {
                    continue;
                };
                if response.result != 0 {
                    return Err(Error::Rejected {
                        command,
                        result: response.result,
                    });
                }
                return Ok(response);
            }
        }
        Err(error)
    }

    fn serve(
        &mut self,
        incoming: &mut mpsc::Receiver<Request>,
        closing: &AtomicBool,
        fault_updates: &watch::Sender<u16>,
        opened_at: Instant,
    ) -> Result<(), Error> {
        let mut heartbeat_at = opened_at + HEARTBEAT_INTERVAL;
        let mut lease_deadline = opened_at + LEASE_TIMEOUT;
        let mut poll_at = opened_at + FAULT_POLL_INTERVAL;
        let mut counter = 0u32;
        while !closing.load(Ordering::Acquire) {
            let now = Instant::now();
            if incoming.is_closed() {
                return Ok(());
            }
            if now >= lease_deadline {
                return Err(Error::Timeout);
            }
            if now >= heartbeat_at {
                counter = counter.wrapping_add(1);
                self.request(HEARTBEAT, &counter.to_be_bytes())?;
                // A retried heartbeat can receive a cached reply. Time from its first send.
                heartbeat_at = now + HEARTBEAT_INTERVAL;
                lease_deadline = now + LEASE_TIMEOUT;
                tracing::trace!(event = "usb_heartbeat_acknowledged", counter);
                continue;
            }
            // Leave time for the next heartbeat even if the new request needs every retry.
            if lease_deadline - now > RESPONSE_TIMEOUT * (ATTEMPTS + 1) {
                let request = incoming.try_recv().ok();
                if request
                    .as_ref()
                    .is_some_and(|request| request.reply.is_closed())
                {
                    continue;
                }
                if request.is_some() || now >= poll_at {
                    let (command, body) = request
                        .as_ref()
                        .map_or((GET_RUNTIME_STATUS, &[][..]), |request| {
                            (request.command, request.body.as_slice())
                        });
                    let mut result = self.request(command, body);
                    if command == GET_RUNTIME_STATUS {
                        // USB firmware has no unsolicited fault notification.
                        poll_at = Instant::now() + FAULT_POLL_INTERVAL;
                        result = result.and_then(|response| {
                            let flags = decode_runtime(&response)?.fault_flags;
                            fault_updates.send_if_modified(|previous| {
                                let changed = *previous != flags;
                                *previous = flags;
                                changed
                            });
                            Ok(response)
                        });
                    }
                    let lost = result
                        .as_ref()
                        .err()
                        .filter(|error| error.ends_session())
                        .cloned();
                    if let Some(request) = request {
                        let _ = request.reply.send(result);
                    } else if let Err(error) = result {
                        tracing::debug!(%error, "USB 运行状态轮询失败");
                    }
                    if let Some(error) = lost {
                        return Err(error);
                    }
                    continue;
                }
            }
            self.receive(IDLE_READ.min(heartbeat_at - now))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[tokio::test]
    async fn native_hid_operations_share_a_persistent_thread() {
        let first = on_hid_manager(|| Ok(std::thread::current().id()))
            .await
            .unwrap();
        let second = tokio::spawn(on_hid_manager(|| Ok(std::thread::current().id())))
            .await
            .unwrap()
            .unwrap();
        assert_ne!(first, std::thread::current().id());
        assert_eq!(first, second);
    }

    #[derive(Default)]
    struct MockHid {
        writes: Vec<Report>,
        replies: VecDeque<Report>,
        fail_write: Option<usize>,
        drop_first_reply: bool,
    }

    impl MockHid {
        fn reply(&mut self, command: u8, seq: u16, sid: u32, body: &[u8]) {
            self.replies.extend(wire::fragment(&wire::encode_frame(
                command | 0x80,
                seq,
                sid,
                body,
            )));
        }
    }

    impl ReportIo for MockHid {
        fn send(&mut self, report: &Report) -> Result<(), Error> {
            self.writes.push(*report);
            if self.fail_write == Some(self.writes.len()) {
                return Err(Error::Io("short write".into()));
            }
            Ok(())
        }

        fn receive(&mut self, bytes: &mut [u8], timeout: Duration) -> Result<usize, Error> {
            if std::mem::take(&mut self.drop_first_reply) {
                std::thread::sleep(timeout + Duration::from_millis(1));
                return Ok(0);
            }
            let report = self.replies.pop_front().ok_or(Error::Closed)?;
            bytes[..REPORT_SIZE].copy_from_slice(&report);
            Ok(REPORT_SIZE)
        }
    }

    #[test]
    fn retries_full_request_after_partial_write_and_lost_ack() {
        let mut io = MockHid {
            fail_write: Some(2),
            drop_first_reply: true,
            ..Default::default()
        };
        io.reply(SET_SCENE, 0, 1, &[0, 1, 0, 0]);
        let mut connection = Connection::new(io);
        connection.session_id = 1;
        let mut scene = OutputScene::none();
        scene.apply_mode = crate::protocol::RESTART_SCENE;
        assert_eq!(
            connection
                .request(SET_SCENE, &scene.encode_data())
                .unwrap()
                .body,
            [1, 0, 0]
        );
        let writes = connection.io.writes;
        assert_eq!(writes.len(), 6);
        assert_eq!(writes[0..2], writes[2..4]);
        assert_eq!(writes[0..2], writes[4..6]);
        assert_eq!(connection.seq, 1);
    }

    #[test]
    fn sequence_wrap_ignores_late_replies() {
        let mut io = MockHid::default();
        io.reply(RESET_OUTPUTS, u16::MAX, 1, &[0]);
        io.reply(RESET_OUTPUTS, u16::MAX, 1, &[0]);
        io.reply(RESET_OUTPUTS, 0, 2, &[0]);
        io.reply(RESET_OUTPUTS, 0, 1, &[0]);
        let mut connection = Connection::new(io);
        connection.seq = u16::MAX;
        connection.session_id = 1;
        connection.request(RESET_OUTPUTS, &[]).unwrap();
        connection.request(RESET_OUTPUTS, &[]).unwrap();
        assert_eq!(connection.seq, 1);
        assert!(connection.io.replies.is_empty());
        assert_eq!(
            connection
                .io
                .writes
                .iter()
                .map(|report| report[1])
                .collect::<Vec<_>>(),
            [255, 0]
        );
    }

    #[test]
    fn due_heartbeat_precedes_queued_scene_and_session_loss_stops_the_worker() {
        let mut io = MockHid::default();
        io.reply(HEARTBEAT, 0, 1, &[0, 0, 0, 0, 1, 13, 172, 0, 0, 0, 0]);
        io.reply(SET_SCENE, 1, 1, &[0x0c]);
        let mut connection = Connection::new(io);
        connection.session_id = 1;
        let (tx, mut rx) = mpsc::channel(1);
        let (reply, mut result) = oneshot::channel();
        tx.try_send(Request {
            command: SET_SCENE,
            body: OutputScene::none().encode_data(),
            reply,
        })
        .unwrap();
        let ended = connection.serve(
            &mut rx,
            &AtomicBool::new(false),
            &watch::channel(0).0,
            Instant::now() - HEARTBEAT_INTERVAL,
        );
        assert!(matches!(ended, Err(Error::Rejected { result: 0x0c, .. })));
        assert!(matches!(
            result.try_recv().unwrap(),
            Err(Error::Rejected { result: 0x0c, .. })
        ));
        assert_eq!(connection.io.writes[0][9], HEARTBEAT);
        assert_eq!(connection.io.writes[1][9], SET_SCENE);
    }

    #[test]
    fn reads_fault_flags_after_a_scene_rejection_and_publishes_recovery() {
        let mut io = MockHid::default();
        io.reply(HEARTBEAT, 0, 1, &[0, 0, 0, 0, 1, 13, 172, 0, 0, 0, 0]);
        io.reply(SET_SCENE, 1, 1, &[0x0a]);
        let mut runtime = [0, 2, 1, 1, 0, 0, 0, 1, 13, 172, 0, 0, 0, 0, 0, 0, 0x11];
        io.reply(GET_RUNTIME_STATUS, 2, 1, &runtime);
        let mut connection = Connection::new(io);
        connection.session_id = 1;
        let (tx, mut rx) = mpsc::channel(1);
        let (updates, mut flags) = watch::channel(0);
        let (reply, mut result) = oneshot::channel();
        tx.try_send(Request {
            command: SET_SCENE,
            body: OutputScene::none().encode_data(),
            reply,
        })
        .unwrap();

        let closing = AtomicBool::new(false);
        let ended = connection.serve(
            &mut rx,
            &closing,
            &updates,
            Instant::now() - FAULT_POLL_INTERVAL,
        );
        assert!(matches!(ended, Err(Error::Closed)));
        assert!(result.try_recv().unwrap().unwrap_err().is_scene_rejection());
        assert_eq!(connection.io.writes[0][9], HEARTBEAT);
        assert_eq!(connection.io.writes[1][9], SET_SCENE);
        assert_eq!(connection.io.writes[3][9], GET_RUNTIME_STATUS);
        assert!(flags.has_changed().unwrap());
        assert_eq!(*flags.borrow_and_update(), 0x11);

        runtime[16] = 0;
        connection.io.reply(GET_RUNTIME_STATUS, 3, 1, &runtime);
        let (reply, mut result) = oneshot::channel();
        tx.try_send(Request {
            command: GET_RUNTIME_STATUS,
            body: vec![],
            reply,
        })
        .unwrap();
        let ended = connection.serve(&mut rx, &closing, &updates, Instant::now());
        assert!(matches!(ended, Err(Error::Closed)));
        assert!(result.try_recv().unwrap().is_ok());
        assert!(flags.has_changed().unwrap());
        assert_eq!(*flags.borrow(), 0);
    }
}
