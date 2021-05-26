use std::{convert::TryFrom, fmt, marker::PhantomData, time::Duration};

use anyhow::{anyhow, ensure, Context, Result};
use bitvec::prelude::*;
use bitvec::{order, prelude::BitVec, slice::BitSlice};
use embedded_graphics::{
    drawable::Pixel, geometry::Point, pixelcolor::BinaryColor, prelude::Size, DrawTarget,
};
use rusb::{Device, DeviceDescriptor, Language, UsbContext};
use tracing::warn;

pub struct KeyboardDevice<C>
where
    C: UsbContext,
{
    dev: Device<C>,
    frame_buffer: BitVec<Msb0, u8>,
    keyboard_info: KeyboardInfo,
    screen_dirty: bool,
}

impl<C> KeyboardDevice<C>
where
    C: UsbContext,
{
    pub fn new(context: &C, keyboard_info: KeyboardInfo) -> Result<Self> {
        let dev = context
            .devices()
            .context("Getting list of devices")?
            .iter()
            .map(|dev| {
                let desc = dev
                    .device_descriptor()
                    .context("getting device description")?;
                if keyboard_info.matches(&desc) {
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

        // Initialize the frame buffer as an empty empty screen
        let frame_buffer = bitvec![Msb0, u8; 0; keyboard_info.screen_area()];

        Ok(Self {
            dev,
            frame_buffer,
            keyboard_info,
            screen_dirty: true,
        })
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

impl<C> fmt::Debug for KeyboardDevice<C>
where
    C: UsbContext,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.keyboard_info)
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

#[derive(Debug, Clone, Copy)]
pub struct KeyboardInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub screen_size: Size,
}

impl<Cx> DrawTarget<BinaryColor> for KeyboardDevice<Cx>
where
    Cx: UsbContext,
{
    type Error = anyhow::Error;

    fn draw_pixel(&mut self, item: Pixel<BinaryColor>) -> Result<(), Self::Error> {
        let Pixel(coord, color) = item;
        let Size { width, height } = self.keyboard_info.screen_size;
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
        self.keyboard_info.screen_size
    }
}

impl KeyboardInfo {
    pub fn matches(&self, desc: &DeviceDescriptor) -> bool {
        desc.product_id() == self.product_id && desc.vendor_id() == self.vendor_id
    }

    pub fn screen_area(&self) -> usize {
        (self.screen_size.width * self.screen_size.height) as usize
    }
}
