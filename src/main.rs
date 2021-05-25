#![allow(dead_code, unused_imports)]

mod keyboard;

use std::{convert::TryInto, env::args, time::Duration};

use anyhow::{bail, ensure, Context as AnyhowContext, Result};
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

    let kb = KeyboardDevice::<Keyboard, _>::new(&context)?;
    kb.checkerboard(4, 4)?;

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
            // let kb: KeyboardDevice<ApexProTkl, C> =
            //     device.try_into().context("attaching to device")?;

            let context = rusb::Context::new()?;
            let kb = KeyboardDevice::<ApexProTkl, _>::new(&context).context("getting kb")?;
            kb.checkerboard(8, 8).context("drawing image")?;
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
