#![allow(dead_code, unused_imports)]

mod keyboard;

use std::{convert::TryInto, env::args, time::Duration};

use anyhow::{anyhow, bail, ensure, Context as AnyhowContext, Result};
use embedded_graphics::{
    drawable::Drawable,
    fonts::{Font12x16, Font24x32, Text},
    pixelcolor::BinaryColor,
    prelude::{Point, Primitive, Size},
    primitives::{Circle, Rectangle},
    style::{PrimitiveStyle, TextStyle},
};
use keyboard::KeyboardDevice;
use rusb::{Context, Hotplug, UsbContext};
use tracing_subscriber::EnvFilter;

use crate::keyboard::KeyboardInfo;

fn main() -> Result<()> {
    ensure!(rusb::has_hotplug(), "No hotplug functionality available");

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_err| EnvFilter::from("INFO")),
        )
        .pretty()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let context = rusb::Context::new()?;
    let keyboard_info = KeyboardInfo {
        vendor_id: 0x1038,
        product_id: 0x1614,
        screen_size: Size::new(128, 40),
    };

    let mut keyboard = KeyboardDevice::new(&context, keyboard_info)?;

    Text::new(
        hostname::get()?
            .to_str()
            .ok_or_else(|| anyhow!("Invalid hostname {:?}", hostname::get()))?,
        Point::new(0, 0),
    )
    .into_styled(TextStyle::new(Font12x16, BinaryColor::On))
    .draw(&mut keyboard)?;

    keyboard.flush_screen()?;

    let watcher = Box::new(KeyboardWatcher { keyboard_info });
    let _reg = context.register_callback(
        Some(keyboard_info.product_id),
        Some(keyboard_info.vendor_id),
        None,
        watcher,
    )?;

    tracing::info!("Watching for events");
    loop {
        context.handle_events(None)?;
    }
}

#[derive(Debug)]
struct KeyboardWatcher {
    keyboard_info: KeyboardInfo,
}

impl<C: UsbContext> Hotplug<C> for KeyboardWatcher {
    #[tracing::instrument]
    fn device_arrived(&mut self, device: rusb::Device<C>) {
        tracing::info!("Device arrived");

        let inner = || {
            // the device is marked as busy during this callback. Ideally we'd send a signal to asynchronously
            let context = rusb::Context::new()?;
            let mut keyboard =
                KeyboardDevice::new(&context, self.keyboard_info).context("getting kb")?;

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
        };

        let res: Result<()> = inner();
        if let Err(error) = res {
            tracing::error!(?error, "Error handling newly arrived device")
        }
    }

    #[tracing::instrument]
    fn device_left(&mut self, device: rusb::Device<C>) {
        tracing::info!(?device, "Device left");
    }
}
