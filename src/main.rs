#![allow(dead_code, unused_imports)]

mod keyboard;
mod manager;

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

use crate::{keyboard::KeyboardInfo, manager::KeyboardManager};

fn main() -> Result<()> {
    ensure!(rusb::has_hotplug(), "No hotplug functionality available");

    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_err| EnvFilter::from("INFO")),
        )
        .pretty()
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let keyboard_info = KeyboardInfo {
        vendor_id: 0x1038,
        product_id: 0x1614,
        screen_size: Size::new(128, 40),
    };

    let context = rusb::Context::new()?;
    let manager = KeyboardManager::new(context.clone(), keyboard_info)?;
    manager.sender.send(manager::Message::RefreshScreen)?;
    let manager_handle = manager.spawn()?;

    tracing::info!("Watching for USB events");
    loop {
        if let Err(error) = context.handle_events(None) {
            tracing::error!(?error, "Error handling USB events");
            break;
        }
        tracing::trace!("event loop");
    }

    if let Err(error) = manager_handle.join() {
        tracing::error!(?error, "Manager thread shutdown with an error");
    }

    bail!("Unexpected shutdown")
}
