#![allow(dead_code, unused_imports)]

mod keyboard;

use std::{convert::TryInto, env::args, time::Duration};

use anyhow::{anyhow, bail, ensure, Context as AnyhowContext, Result};
use embedded_graphics::{
    drawable::Drawable,
    fonts::{Font12x16, Font24x32, Text},
    pixelcolor::BinaryColor,
    prelude::{Point, Primitive},
    primitives::{Circle, Rectangle},
    style::{PrimitiveStyle, TextStyle},
};
use keyboard::{ApexProTkl, KeyboardDevice};
use rusb::{Context, Hotplug, UsbContext};
use tracing_subscriber::EnvFilter;

use crate::keyboard::KeyboardType;

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
    type Keyboard = ApexProTkl;

    let mut keyboard = KeyboardDevice::<Keyboard, _>::new(&context)?;

    Text::new(
        hostname::get()?
            .to_str()
            .ok_or_else(|| anyhow!("Invalid hostname {:?}", hostname::get()))?,
        Point::new(0, 0),
    )
    .into_styled(TextStyle::new(Font12x16, BinaryColor::On))
    .draw(&mut keyboard)?;

    keyboard.flush_screen()?;

    let watcher = Box::new(KeyboardWatcher);
    let _reg = context.register_callback(
        Some(Keyboard::VENDOR_ID),
        Some(Keyboard::PRODUCT_ID),
        None,
        watcher,
    )?;

    tracing::info!("Watching for events");
    loop {
        context.handle_events(None)?;
    }
}

#[derive(Debug)]
struct KeyboardWatcher;

impl<C: UsbContext> Hotplug<C> for KeyboardWatcher {
    #[tracing::instrument]
    fn device_arrived(&mut self, device: rusb::Device<C>) {
        tracing::info!("Device arrived");

        let inner = || {
            // the device is marked as busy during this callback. Ideally we'd send a signal to asynchronously
            let context = rusb::Context::new()?;
            let mut keyboard =
                KeyboardDevice::<ApexProTkl, _>::new(&context).context("getting kb")?;

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
