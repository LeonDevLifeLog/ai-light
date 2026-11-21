use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::ble::BleIo;
use crate::protocol::{self, OutputScene};
use crate::transport::{Transport, TransportError};
use crate::usb;

/// The engine dispatches scene operations. Each device owns its wire transactions.
#[async_trait]
pub trait SceneOutput: Send + Sync + 'static {
    async fn apply_scene(&self, scene: &OutputScene) -> Result<(), TransportError>;
    async fn clear_outputs(&self) -> Result<(), TransportError>;

    fn coalesce_scenes(&self) -> bool {
        false
    }
}

#[async_trait]
impl SceneOutput for Transport {
    async fn apply_scene(&self, scene: &OutputScene) -> Result<(), TransportError> {
        let mut wire_scene = scene.clone();
        for led in &mut wire_scene.leds {
            // BLE firmware expects a positive brightness even for black tracks.
            led.brightness = led.brightness.max(1);
        }
        for segment in &mut wire_scene.buzzer.segments {
            segment.volume = if segment.frequency_hz == 0 {
                0
            } else {
                segment.volume.max(1)
            };
        }
        let frame = self.set_scene(&wire_scene).await?;
        if let Ok((result, _)) = protocol::parse_set_scene_response(&frame.data) {
            if result != protocol::ResultCode::Ok {
                tracing::warn!(event = "scene_rejected", result_code = %result, "SET_SCENE 被设备拒绝");
            }
        }
        Ok(())
    }

    async fn clear_outputs(&self) -> Result<(), TransportError> {
        self.reset_outputs().await.map(|_| ())
    }
}

#[derive(Clone)]
pub enum ConnectedDevice {
    Ble {
        io: Arc<BleIo>,
        transport: Transport,
    },
    Usb(usb::Session),
}

/// A transaction snapshots the connection before awaiting its response.
#[derive(Default)]
pub struct DeviceSlot(RwLock<Option<ConnectedDevice>>);

impl DeviceSlot {
    pub fn current(&self) -> Option<ConnectedDevice> {
        self.0.read().unwrap().clone()
    }

    pub fn set(&self, device: Option<ConnectedDevice>) {
        *self.0.write().unwrap() = device;
    }

    pub fn take(&self) -> Option<ConnectedDevice> {
        self.0.write().unwrap().take()
    }
}

#[async_trait]
impl SceneOutput for DeviceSlot {
    async fn apply_scene(&self, scene: &OutputScene) -> Result<(), TransportError> {
        match self.current() {
            Some(ConnectedDevice::Ble { transport, .. }) => transport.apply_scene(scene).await,
            Some(ConnectedDevice::Usb(session)) => session
                .set_scene(scene)
                .await
                .map(|_| ())
                .map_err(TransportError::Usb),
            None => Err(TransportError::Io("设备未连接".into())),
        }
    }

    async fn clear_outputs(&self) -> Result<(), TransportError> {
        match self.current() {
            Some(ConnectedDevice::Ble { transport, .. }) => transport.clear_outputs().await,
            Some(ConnectedDevice::Usb(session)) => {
                session.reset_outputs().await.map_err(TransportError::Usb)
            }
            None => Err(TransportError::Io("设备未连接".into())),
        }
    }

    fn coalesce_scenes(&self) -> bool {
        matches!(*self.0.read().unwrap(), Some(ConnectedDevice::Usb(_)))
    }
}
