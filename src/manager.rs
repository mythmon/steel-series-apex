use crate::keyboard::{KeyboardDevice, KeyboardInfo};
use anyhow::{anyhow, bail, Result};
use embedded_graphics::{
    drawable::Drawable,
    fonts::{Font12x16, Text},
    pixelcolor::BinaryColor,
    prelude::{Point, Primitive},
    primitives::Circle,
    style::{PrimitiveStyle, TextStyle},
};
use rusb::{Hotplug, Registration, UsbContext};
use std::{
    fmt,
    sync::mpsc::{channel, Receiver, Sender},
    thread::{self, JoinHandle},
};

pub struct KeyboardManager {
    keyboard_info: KeyboardInfo,
    receiver: Receiver<Message>,
    pub sender: Sender<Message>,
    context: rusb::Context,
    callback_handle: Registration<rusb::Context>,
}

#[derive(Debug, Copy, Clone)]
pub enum Message {
    DeviceArrived,
    DeviceLeft,
    RefreshScreen,
}

impl KeyboardManager {
    pub fn spawn(self) -> Result<JoinHandle<()>> {
        let handle = thread::Builder::new()
            .name(format!(
                "KeyboardManager-{}:{}",
                self.keyboard_info.vendor_id, self.keyboard_info.product_id
            ))
            .spawn(move || loop {
                match self.receiver.recv() {
                    Ok(msg) => self.handle_message(msg),
                    Err(error) => {
                        tracing::error!(%error);
                        break;
                    }
                }
            })?;
        Ok(handle)
    }
}

impl KeyboardManager {
    pub fn new(context: rusb::Context, keyboard_info: KeyboardInfo) -> Result<Self> {
        let (sender, receiver) = channel();
        let callback_handle = context.register_callback(
            Some(keyboard_info.vendor_id),
            Some(keyboard_info.product_id),
            None,
            Box::new(KeyboardWatcher {
                sender: sender.clone(),
            }),
        )?;
        Ok(Self {
            keyboard_info,
            receiver,
            sender,
            context,
            callback_handle,
        })
    }

    #[tracing::instrument(skip(self))]
    fn handle_message(&self, message: Message) {
        tracing::info!("manager message received");
        let res = match message {
            Message::DeviceArrived => self.draw_screen(),
            Message::DeviceLeft => Ok(()),
            Message::RefreshScreen => self.draw_screen(),
        };
        if let Err(error) = res {
            tracing::error!(?error, ?message, "error handling message");
        }
    }

    fn draw_screen(&self) -> Result<()> {
        let mut keyboard = KeyboardDevice::new(&self.context, self.keyboard_info)?;

        Text::new(
            hostname::get()?
                .to_str()
                .ok_or_else(|| anyhow!("Invalid hostname {:?}", hostname::get()))?,
            Point::new(0, 0),
        )
        .into_styled(TextStyle::new(Font12x16, BinaryColor::On))
        .draw(&mut keyboard)?;

        keyboard.flush_screen()?;
        Ok(())
    }
}

impl fmt::Debug for KeyboardManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyboardManager")
            .field("keyboard_info", &self.keyboard_info)
            .field("receiver", &"..")
            .field("sender", &"..")
            .finish()
    }
}

#[derive(Debug)]
pub struct KeyboardWatcher {
    sender: Sender<Message>,
}

impl<C: UsbContext> Hotplug<C> for KeyboardWatcher {
    #[tracing::instrument(skip(_device))]
    fn device_arrived(&mut self, _device: rusb::Device<C>) {
        tracing::debug!("USB device arrived");
        if let Err(error) = self.sender.send(Message::DeviceArrived) {
            tracing::error!(%error, "Error sending DeviceArrived message to manager");
        }
    }

    #[tracing::instrument(skip(_device))]
    fn device_left(&mut self, _device: rusb::Device<C>) {
        tracing::debug!("USB device left");
        if let Err(error) = self.sender.send(Message::DeviceLeft) {
            tracing::error!(%error, "Error sending DeviceLeft message to manager");
        }
    }
}
