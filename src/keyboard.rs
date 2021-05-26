use std::{convert::TryFrom, fmt, marker::PhantomData, time::Duration};

use anyhow::{anyhow, ensure, Context, Result};
use bitvec::prelude::*;
use bitvec::{order, prelude::BitVec, slice::BitSlice};
use embedded_graphics::{
    drawable::Pixel, geometry::Point, pixelcolor::BinaryColor, prelude::Size, DrawTarget,
};
use rusb::{Device, UsbContext};
use tracing::warn;

pub struct KeyboardDevice<K, C>
where
    K: KeyboardType,
    C: UsbContext,
{
    dev: Device<C>,
    frame_buffer: BitVec<Msb0, u8>,
    screen_dirty: bool,
    _keyboard_type: PhantomData<K>,
}

impl<K, C> KeyboardDevice<K, C>
where
    K: KeyboardType,
    C: UsbContext,
{
    pub fn new(context: &C) -> Result<Self> {
        let dev = context
            .devices()
            .context("Getting list of devices")?
            .iter()
            .map(|dev| {
                let desc = dev
                    .device_descriptor()
                    .context("getting device description")?;
                if desc.product_id() == K::PRODUCT_ID && desc.vendor_id() == K::VENDOR_ID {
                    Ok(Some(dev))
                } else {
                    Ok(None)
                }
            })
            .filter_map(|dev_res: Result<_>| match dev_res {
                Ok(opt) => opt,
                Err(error) => {
                    warn!(%error, "Error reading USB device");
                    None
                }
            })
            .next()
            .ok_or_else(|| anyhow!("Could not find keyboard"))?;

        let mut frame_buffer = BitVec::with_capacity(Self::screen_area());
        frame_buffer.resize(Self::screen_area(), false);

        Ok(Self {
            dev,
            _keyboard_type: PhantomData::default(),
            frame_buffer,
            screen_dirty: true,
        })
    }

    pub fn screen_area() -> usize {
        (K::OLED_SIZE.width * K::OLED_SIZE.height) as usize
    }

    fn send(&self, cmd: KeyboardCommand, buf: &[u8]) -> Result<()> {
        let mut handle = self.dev.open().context("Opening USB device for keyboard")?;
        const INTERFACE_NUM: u8 = 1;
        handle
            .set_auto_detach_kernel_driver(true)
            .context("settings auto-detach kernel driver")?;

        let dev_desc = self.dev.device_descriptor()?;
        for config_num in 0..(dev_desc.num_configurations()) {
            let config_desc = self.dev.config_descriptor(config_num)?;
            for iface_num in 0..(config_desc.num_interfaces()) {
                handle.claim_interface(iface_num).context(format!(
                    "claiming config {}/{}, interface {}/{}",
                    config_num,
                    dev_desc.num_configurations(),
                    iface_num,
                    config_desc.num_interfaces(),
                ))?;
            }
        }

        let request_type = rusb::request_type(
            rusb::Direction::Out,
            rusb::RequestType::Class,
            rusb::Recipient::Interface,
        );
        assert_eq!(request_type, 0x21);
        let request = 0x09; // what does this mean?
        let mut remaining_bytes = buf.len();

        let timeout = Duration::from_secs(5);

        let bytes_written = handle
            .write_control(
                request_type,
                request,
                cmd.value(),
                cmd.index(),
                buf,
                timeout,
            )
            .context(format!("sending {:?} request", cmd))?;
        remaining_bytes = remaining_bytes.saturating_sub(bytes_written);
        ensure!(remaining_bytes == 0, "entire request not written");

        Ok(())
    }

    pub fn flush_screen(&mut self) -> Result<()> {
        if self.screen_dirty {
            self.send_image(&self.frame_buffer)?;
            self.screen_dirty = false;
        }
        Ok(())
    }

    fn send_image(&self, image_data: &BitSlice<Msb0, u8>) -> Result<()> {
        let mut io_buf: BitVec<order::Msb0, u8> = BitVec::new();
        io_buf.resize(8, false);
        io_buf[..8].store(0x65_u8);
        io_buf.extend_from_bitslice(image_data);

        let buf: &[u8] = io_buf.as_raw_slice();
        self.send(KeyboardCommand::Oled, buf)
    }
}

impl<K, C> fmt::Debug for KeyboardDevice<K, C>
where
    K: KeyboardType,
    C: UsbContext,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        K::fmt_debug(f)
    }
}

#[derive(Debug, Clone, Copy)]
enum KeyboardCommand {
    #[allow(dead_code)]
    Colors,
    #[allow(dead_code)]
    Config {
        index: u16,
    },
    Oled,
}

impl KeyboardCommand {
    fn value(&self) -> u16 {
        match self {
            KeyboardCommand::Colors => 0x300,
            KeyboardCommand::Config { .. } => 0x200,
            KeyboardCommand::Oled => 0x300,
        }
    }

    fn index(&self) -> u16 {
        match self {
            KeyboardCommand::Colors => 0x01,
            KeyboardCommand::Config { index } => *index,
            KeyboardCommand::Oled => 0x01,
        }
    }
}

pub trait KeyboardType {
    const VENDOR_ID: u16;
    const PRODUCT_ID: u16;
    const OLED_SIZE: Size;

    fn fmt_debug(f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:x?}:{:x?}", Self::VENDOR_ID, Self::PRODUCT_ID)
    }
}

#[derive(Clone, Copy)]
pub struct ApexProTkl;

impl KeyboardType for ApexProTkl {
    const VENDOR_ID: u16 = 0x1038;
    const PRODUCT_ID: u16 = 0x1614;
    const OLED_SIZE: Size = Size {
        width: 128,
        height: 40,
    };
}

impl<K, Cx> DrawTarget<BinaryColor> for KeyboardDevice<K, Cx>
where
    K: KeyboardType,
    Cx: UsbContext,
{
    type Error = anyhow::Error;

    fn draw_pixel(&mut self, item: Pixel<BinaryColor>) -> Result<(), Self::Error> {
        let Pixel(coord, color) = item;
        let Size { width, height } = K::OLED_SIZE;
        let Point { x, y } = coord;

        // out of bounds drawing should be a no-op
        if x < 0 || y < 0 {
            return Ok(());
        }
        let x = x as u32;
        let y = y as u32;
        if x >= width || y >= height {
            return Ok(());
        }

        let p = x + y * width;
        let mut bit = self.frame_buffer.get_mut(p as usize).ok_or_else(|| {
            anyhow!(
                "Bug: Unexpected index out of bounds {},{} in {}x{}",
                x,
                y,
                width,
                height
            )
        })?;
        *bit = match color {
            BinaryColor::On => true,
            BinaryColor::Off => false,
        };

        Ok(())
    }

    fn size(&self) -> Size {
        K::OLED_SIZE
    }
}
